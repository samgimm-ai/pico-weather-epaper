use cyw43::JoinOptions;
use defmt::*;
use embassy_time::{Duration, Timer};

/// Connect to WiFi network with retry logic
pub async fn connect(control: &mut cyw43::Control<'_>) {
    info!("Connecting to WiFi: {}...", crate::config::WIFI_SSID);

    loop {
        let opts = JoinOptions::new(crate::config::WIFI_PASSWORD.as_bytes());
        match control
            .join(crate::config::WIFI_SSID, opts)
            .await
        {
            Ok(_) => {
                info!("WiFi connected!");
                return;
            }
            Err(e) => {
                warn!("WiFi join failed (status={}), retrying in 5s...", e.status);
                Timer::after(Duration::from_secs(5)).await;
            }
        }
    }
}
