use crate::utils::signaled_mutex::SignaledMutex;
use alloc::rc::Rc;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_time::Instant;
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
}

impl GlobalStateInner {
    pub fn new(nvs: &Nvs) -> Self {
        Self {
            state: SignaledMutex::new(SignaledGlobalStateInner::new()),
            nvs: nvs.clone(),
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
