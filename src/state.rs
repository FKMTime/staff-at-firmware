use crate::utils::signaled_mutex::SignaledMutex;
use alloc::rc::Rc;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};
use embassy_time::{Instant, Timer};
use esp_hal::gpio::Output;
use esp_hal_wifimanager::Nvs;

pub static mut EPOCH_BASE: u64 = 0;
pub static mut SLEEP_STATE: bool = false;
pub static mut DEEPER_SLEEP: bool = false;
pub static mut OTA_STATE: bool = false;

#[inline(always)]
pub fn current_epoch() -> u64 {
    unsafe { EPOCH_BASE + Instant::now().as_secs() }
}

#[inline(always)]
pub fn sleep_state() -> bool {
    unsafe { SLEEP_STATE }
}

#[inline(always)]
pub fn deeper_sleep_state() -> bool {
    unsafe { DEEPER_SLEEP }
}

#[inline(always)]
pub fn ota_state() -> bool {
    unsafe { OTA_STATE }
}

pub type GlobalState = Rc<GlobalStateInner>;
pub struct GlobalStateInner {
    pub state: SignaledMutex<CriticalSectionRawMutex, SignaledGlobalStateInner>,
    pub nvs: Nvs,

    pub output_led: Mutex<CriticalSectionRawMutex, Output<'static>>,
}

impl GlobalStateInner {
    pub fn new(nvs: &Nvs, output_led: Output<'static>) -> Self {
        Self {
            state: SignaledMutex::new(SignaledGlobalStateInner::new()),
            nvs: nvs.clone(),
            output_led: Mutex::new(output_led),
        }
    }

    pub async fn led(&self, state: bool) {
        let mut output_led = self.output_led.lock().await;
        output_led.set_level(if state {
            esp_hal::gpio::Level::High
        } else {
            esp_hal::gpio::Level::Low
        });
    }

    pub async fn led_blink(&self, count: usize, length: u64) {
        let mut output_led = self.output_led.lock().await;

        for _ in 0..count {
            output_led.set_high();
            Timer::after_millis(length).await;
            output_led.set_low();
            Timer::after_millis(length).await;
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SignaledGlobalStateInner {
    pub device_added: Option<bool>,
    pub server_connected: Option<bool>,
}

impl SignaledGlobalStateInner {
    pub fn new() -> Self {
        Self {
            device_added: None,
            server_connected: None,
        }
    }
}
