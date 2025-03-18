use crate::consts::RFID_RETRY_INIT_MS;
use crate::state::GlobalState;
use crate::structs::AttendanceMarkedPacket;
use embassy_time::{Duration, Timer};
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
    _global_state: GlobalState,
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

    //let mut rfid_sleep = false;
    loop {
        Timer::after(Duration::from_millis(10)).await;
        // NOTE: rfid doesnt have sleep on this device
        /*
        if sleep_state() != rfid_sleep {
            rfid_sleep = sleep_state();

            match rfid_sleep {
                true => _ = mfrc522.pcd_soft_power_down().await,
                false => _ = mfrc522.pcd_soft_power_up().await,
            }
        }

        if rfid_sleep {
            Timer::after(Duration::from_millis(500)).await;
            continue;
        }
        */

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

        let resp = crate::ws::send_request::<AttendanceMarkedPacket>(
            crate::structs::TimerPacketInner::CardInfoRequest {
                card_id: card_uid as u64,
                attendance_device: Some(true),
            },
        )
        .await;

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
