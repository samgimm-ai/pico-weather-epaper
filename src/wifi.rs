use cyw43::JoinOptions;
use defmt::*;
use embassy_time::{Duration, Timer};

const MAX_RETRIES: u8 = 10;
const DELAYS_SECS: [u64; 5] = [5, 10, 30, 60, 60];

/// Connect to WiFi network with exponential backoff.
/// Returns Ok on success, Err after MAX_RETRIES failures.
pub async fn connect(control: &mut cyw43::Control<'_>) -> Result<(), ()> {
    info!("Connecting to WiFi: {}...", crate::config::WIFI_SSID);

    for attempt in 0..MAX_RETRIES {
        let opts = JoinOptions::new(crate::config::WIFI_PASSWORD.as_bytes());
        match control.join(crate::config::WIFI_SSID, opts).await {
            Ok(_) => {
                info!("WiFi connected!");
                return Ok(());
            }
            Err(e) => {
                let delay = DELAYS_SECS[attempt.min(4) as usize];
                warn!(
                    "WiFi failed ({}/{}) status={}, retry in {}s...",
                    attempt + 1,
                    MAX_RETRIES,
                    e.status,
                    delay
                );
                Timer::after(Duration::from_secs(delay)).await;
            }
        }
    }
    error!("WiFi connect failed after {} attempts", MAX_RETRIES);
    Err(())
}
