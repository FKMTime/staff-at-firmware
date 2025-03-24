#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use consts::{LOG_SEND_INTERVAL_MS, PRINT_HEAP_INTERVAL_MS};
use embassy_executor::Spawner;
use embassy_sync::signal::Signal;
use embassy_time::{Instant, Timer};
use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::gpio::{Level, Output, Pin};
use esp_hal::rng::Rng;
use esp_hal::timer::systimer::SystemTimer;
use esp_hal::timer::timg::TimerGroup;
use esp_hal_wifimanager::{Nvs, WIFI_NVS_KEY};
use esp_storage::FlashStorage;
use state::{deeper_sleep_state, ota_state, sleep_state, GlobalState, GlobalStateInner};
use structs::ConnSettings;
use utils::logger::FkmLogger;
use utils::set_brownout_detection;
use ws_framer::{WsUrl, WsUrlOwned};

mod battery;
mod consts;
mod mdns;
mod rfid;
mod state;
mod structs;
mod utils;
mod version;
mod ws;

extern crate alloc;

pub fn custom_rng(buf: &mut [u8]) -> Result<(), getrandom::Error> {
    for chunk in buf.chunks_mut(4) {
        let random_u32 = unsafe { &*esp_hal::peripherals::RNG::PTR }
            .data()
            .read()
            .bits();

        let len = chunk.len();
        chunk[..].copy_from_slice(&random_u32.to_be_bytes()[..len]);
    }

    Ok(())
}
getrandom::register_custom_getrandom!(custom_rng);

#[esp_hal_embassy::main]
async fn main(spawner: Spawner) {
    esp_println::logger::init_logger_from_env();

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::_80MHz);
    let peripherals = esp_hal::init(config);

    esp_alloc::heap_allocator!(size: 120 * 1024);
    {
        const HEAP_SIZE: usize = 60 * 1024;

        #[link_section = ".dram2_uninit"]
        static mut HEAP2: core::mem::MaybeUninit<[u8; HEAP_SIZE]> =
            core::mem::MaybeUninit::uninit();

        #[allow(static_mut_refs)]
        unsafe {
            esp_alloc::HEAP.add_region(esp_alloc::HeapRegion::new(
                HEAP2.as_mut_ptr() as *mut u8,
                core::mem::size_of_val(&*core::ptr::addr_of!(HEAP2)),
                esp_alloc::MemoryCapability::Internal.into(),
            ));
        }
    }

    set_brownout_detection(false);
    let timer0 = SystemTimer::new(peripherals.SYSTIMER);
    esp_hal_embassy::init(timer0.alarm0);
    FkmLogger::set_logger();
    log::info!("Firmware Version: {}", version::VERSION);

    let led = Output::new(peripherals.GPIO3, Level::Low, Default::default());
    let nvs = Nvs::new_from_part_table().expect("Wrong partition configuration!");
    let global_state = Rc::new(GlobalStateInner::new(&nvs, led));
    let wifi_setup_sig = Rc::new(Signal::new());
    let ws_connect_signal = Rc::new(Signal::new());

    global_state.led_blink(3, 100).await;

    spawner.must_spawn(battery::battery_read_task(
        peripherals.GPIO2,
        peripherals.ADC1,
        global_state.clone(),
    ));

    let sck = peripherals.GPIO4.degrade();
    let miso = peripherals.GPIO5.degrade();
    let mosi = peripherals.GPIO6.degrade();
    let cs = Output::new(peripherals.GPIO7, Level::High, Default::default());
    spawner.must_spawn(rfid::rfid_task(
        miso,
        mosi,
        sck,
        cs,
        peripherals.SPI2,
        peripherals.DMA_CH0,
        global_state.clone(),
        ws_connect_signal.clone(),
    ));

    let mut wm_settings = esp_hal_wifimanager::WmSettings::default();
    wm_settings.ssid.clear();
    _ = core::fmt::write(
        &mut wm_settings.ssid,
        format_args!("FKM-SA-{:X}", crate::utils::get_efuse_u32()),
    );

    // mark ota as valid
    {
        if let Ok(mut ota) = esp_hal_ota::Ota::new(FlashStorage::new()) {
            let res = ota.ota_mark_app_valid();
            if let Err(e) = res {
                log::error!("Ota mark app valid failed: {e:?}");
            }
        }
    }

    let rng = Rng::new(peripherals.RNG);
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let wifi_res = esp_hal_wifimanager::init_wm(
        wm_settings,
        &spawner,
        &nvs,
        rng,
        timg0.timer0,
        peripherals.RADIO_CLK,
        peripherals.WIFI,
        peripherals.BT,
        Some(wifi_setup_sig),
    )
    .await;

    let Ok(mut wifi_res) = wifi_res else {
        log::error!("WifiManager failed!!! Restarting in 1s!");
        Timer::after_millis(1000).await;
        esp_hal::system::software_reset();
    };

    let conn_settings: ConnSettings = wifi_res
        .data
        .take()
        .and_then(|d| serde_json::from_value(d).ok())
        .unwrap_or_default();

    let mut parse_retry_count = 0;
    let ws_url = loop {
        let url = if conn_settings.mdns || conn_settings.ws_url.is_none() || parse_retry_count > 0 {
            log::info!("Start mdns lookup...");
            let mdns_res = mdns::mdns_query(wifi_res.sta_stack).await;
            log::info!("Mdns result: {:?}", mdns_res);

            mdns_res.to_string()
        } else {
            conn_settings.ws_url.clone().expect("")
        };

        let ws_url = WsUrl::from_str(&url);
        match ws_url {
            Some(ws_url) => break WsUrlOwned::new(&ws_url),
            None => {
                parse_retry_count += 1;
                log::error!("Mdns parse failed! Retry ({parse_retry_count})..");
                Timer::after_millis(1000).await;
                if parse_retry_count > 3 {
                    log::error!("Cannot parse wsurl! Reseting wifi configuration!");
                    _ = nvs.invalidate_key(WIFI_NVS_KEY).await;
                    Timer::after_millis(1000).await;

                    esp_hal::system::software_reset();
                }

                continue;
            }
        }
    };

    utils::backtrace_store::read_saved_backtrace().await;
    let ws_sleep_sig = Rc::new(Signal::new());
    spawner.must_spawn(ws::ws_task(
        wifi_res.sta_stack,
        ws_url,
        global_state.clone(),
        ws_sleep_sig.clone(),
        ws_connect_signal,
    ));

    spawner.must_spawn(logger_task(global_state.clone()));
    set_brownout_detection(true);

    let mut last_led_blink = Instant::now();
    let mut last_sleep = false;
    loop {
        Timer::after_millis(100).await;
        if sleep_state() != last_sleep {
            last_sleep = sleep_state();
            ws_sleep_sig.signal(last_sleep);

            match last_sleep {
                true => wifi_res.stop_radio(),
                false => wifi_res.restart_radio(),
            }
        }

        if deeper_sleep_state() && (Instant::now() - last_led_blink).as_millis() >= 5000 {
            let mut led = global_state.output_led.lock().await;
            let initial_level = led.output_level();

            for _ in 0..2 {
                led.set_high();
                Timer::after_millis(25).await;
                led.set_low();
                Timer::after_millis(25).await;
            }

            led.set_level(initial_level);
            last_led_blink = Instant::now();
        }
    }
}

#[embassy_executor::task]
async fn logger_task(global_state: GlobalState) {
    let mut heap_start = Instant::now();
    loop {
        Timer::after_millis(LOG_SEND_INTERVAL_MS).await;

        let mut tmp_logs: Vec<String> = Vec::new();
        while let Ok(msg) = utils::logger::LOGS_CHANNEL.try_receive() {
            tmp_logs.push(msg);
        }

        if ota_state() || sleep_state() {
            continue;
        }

        if !tmp_logs.is_empty() {
            tmp_logs.reverse();

            ws::send_packet(structs::TimerPacket {
                tag: None,
                data: structs::TimerPacketInner::Logs { logs: tmp_logs },
            })
            .await;
        }

        if (Instant::now() - heap_start).as_millis() >= PRINT_HEAP_INTERVAL_MS {
            if global_state.state.lock().await.server_connected == Some(true) {
                log::info!("{}", esp_alloc::HEAP.stats());
            }

            heap_start = Instant::now();
        }
    }
}
