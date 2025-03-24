use crate::consts::{DEEPER_SLEEP_AFTER_MS, RFID_RETRY_INIT_MS, SLEEP_AFTER_MS};
use crate::state::{deeper_sleep_state, sleep_state, GlobalState, SLEEP_STATE};
use crate::structs::AttendanceMarkedPacket;
use crate::utils::deeper_sleep;
use alloc::rc::Rc;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use embassy_time::{Duration, Instant, Timer};
use esp_hal::time::Rate;
use esp_hal::{
    dma::{DmaRxBuf, DmaTxBuf},
    dma_buffers,
    gpio::AnyPin,
    spi::{master::Spi, Mode},
};
use esp_hal_mfrc522::consts::UidSize;

#[embassy_executor::task]
pub async fn rfid_task(
    miso: AnyPin,
    mosi: AnyPin,
    sck: AnyPin,
    cs_pin: esp_hal::gpio::Output<'static>,
    spi: esp_hal::peripherals::SPI2,
    dma_chan: esp_hal::dma::DmaChannel0,
    global_state: GlobalState,
    ws_connect_signal: Rc<Signal<CriticalSectionRawMutex, ()>>,
) {
    let (rx_buffer, rx_descriptors, tx_buffer, tx_descriptors) = dma_buffers!(512);
    let dma_tx_buf = DmaTxBuf::new(tx_descriptors, tx_buffer).expect("Dma tx buf failed");
    let dma_rx_buf = DmaRxBuf::new(rx_descriptors, rx_buffer).expect("Dma rx buf failed");

    let spi = Spi::new(
        spi,
        esp_hal::spi::master::Config::default()
            .with_frequency(Rate::from_khz(400))
            .with_mode(Mode::_0),
    )
    .expect("Spi init failed")
    .with_sck(sck)
    .with_miso(miso)
    .with_mosi(mosi)
    .with_dma(dma_chan)
    .with_buffers(dma_rx_buf, dma_tx_buf)
    .into_async();

    let mut mfrc522 = {
        let spi = embedded_hal_bus::spi::ExclusiveDevice::new(spi, cs_pin, embassy_time::Delay)
            .expect("Spi bus init failed (cs set high failed)");

        esp_hal_mfrc522::MFRC522::new(spi)
    };

    loop {
        _ = mfrc522.pcd_init().await;
        if mfrc522.pcd_is_init().await {
            break;
        }

        log::error!("MFRC522 init failed! Try to power cycle to module! Retrying...");
        Timer::after(Duration::from_millis(RFID_RETRY_INIT_MS)).await;
    }
    log::debug!("PCD ver: {:?}", mfrc522.pcd_get_version().await);

    // wait for ws connection
    ws_connect_signal.wait().await;

    let mut key_buf = [0; 16];
    if global_state
        .nvs
        .get_key(b"DEEP_SLEEP_CARD", &mut key_buf)
        .await
        .is_ok()
    {
        let card_uid = u128::from_be_bytes(key_buf.into());
        log::info!("Card UID: {card_uid}");
        global_state.led_blink(2, 100).await;

        let resp = crate::ws::send_request::<AttendanceMarkedPacket>(
            crate::structs::TimerPacketInner::CardInfoRequest {
                card_id: card_uid as u64,
                attendance_device: Some(true),
            },
        )
        .await;
        global_state.led(true).await;

        match resp {
            Ok(resp) => {
                log::info!("Attendance card response: {resp:?}");
            }
            Err(e) => {
                log::error!(
                    "[RFID] Resp_error: ({}): {:?}",
                    e.should_reset_time,
                    e.error
                );
            }
        }

        _ = global_state.nvs.invalidate_key(b"DEEP_SLEEP_CARD").await;
    }

    let mut last_scan = Instant::now();
    //let mut rfid_sleep = false;
    loop {
        Timer::after(Duration::from_millis(10)).await;
        if (Instant::now() - last_scan).as_millis() >= SLEEP_AFTER_MS && !sleep_state() {
            log::info!("Going into sleep!");
            unsafe {
                SLEEP_STATE = true;
            }
        }

        if (Instant::now() - last_scan).as_millis() >= DEEPER_SLEEP_AFTER_MS
            && !deeper_sleep_state()
        {
            log::info!("Going into depper sleep!");
            deeper_sleep();
        }

        if mfrc522.picc_is_new_card_present().await.is_err() {
            continue;
        }

        let Ok(card_uid) = mfrc522
            .get_card(UidSize::Four)
            .await
            .map(|c| c.get_number())
        else {
            continue;
        };

        log::info!("Card UID: {card_uid}");
        global_state.led_blink(2, 100).await;
        last_scan = Instant::now();

        if sleep_state() {
            log::info!("Sleep done!");
            unsafe {
                SLEEP_STATE = false;
            }
        }

        if deeper_sleep_state() {
            log::info!("Deeper sleep done!");
            _ = global_state.nvs.invalidate_key(b"DEEP_SLEEP_CARD").await;
            _ = global_state
                .nvs
                .append_key(b"DEEP_SLEEP_CARD", &card_uid.to_be_bytes())
                .await;

            Timer::after_millis(100).await;
            esp_hal::system::software_reset();
        }

        let resp = crate::ws::send_request::<AttendanceMarkedPacket>(
            crate::structs::TimerPacketInner::CardInfoRequest {
                card_id: card_uid as u64,
                attendance_device: Some(true),
            },
        )
        .await;
        global_state.led(true).await;

        match resp {
            Ok(resp) => {
                log::info!("Attendance card response: {resp:?}");
            }
            Err(e) => {
                log::error!(
                    "[RFID] Resp_error: ({}): {:?}",
                    e.should_reset_time,
                    e.error
                );
            }
        }

        _ = mfrc522.picc_halta().await;
    }
}
