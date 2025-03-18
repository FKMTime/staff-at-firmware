use alloc::{
    string::{String, ToString},
    vec::Vec,
};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Debug)]
pub struct ConnSettings {
    pub mdns: bool,
    pub ws_url: Option<String>,
}

impl Default for ConnSettings {
    fn default() -> Self {
        Self {
            mdns: true,
            ws_url: None,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TimerPacket {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<u64>,
    pub data: TimerPacketInner,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub enum TimerPacketInner {
    StartUpdate {
        version: String,
        build_time: u64, // NOT USED
        size: u32,
        crc: u32,
        firmware: String,
    },
    ApiError(ApiError),
    CardInfoRequest {
        card_id: u64,

        #[serde(skip_serializing_if = "Option::is_none")]
        attendance_device: Option<bool>,
    },
    AttendanceMarked,
    DeviceSettings {
        added: bool,
    },
    Logs {
        logs: Vec<String>,
    },
    Battery {
        level: Option<f64>,
        voltage: Option<f64>,
    },
    Add {
        firmware: String,
    },
    EpochTime {
        current_epoch: u64,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AttendanceMarkedPacket {}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ApiError {
    pub error: String,
    pub should_reset_time: bool,
}

pub trait FromPacket: Sized {
    fn from_packet(packet: TimerPacket) -> Result<Self, ApiError>;
}

impl FromPacket for AttendanceMarkedPacket {
    fn from_packet(packet: TimerPacket) -> Result<Self, ApiError> {
        match packet.data {
            TimerPacketInner::AttendanceMarked => Ok(AttendanceMarkedPacket {}),
            TimerPacketInner::ApiError(api_error) => Err(api_error),
            _ => Err(ApiError {
                error: alloc::format!("Wrong response type! ({:?})", packet),
                should_reset_time: false,
            }),
        }
    }
}
