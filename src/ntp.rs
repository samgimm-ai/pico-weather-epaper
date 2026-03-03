use defmt::*;
use embassy_net::dns::DnsQueryType;
use embassy_net::udp::{PacketMetadata, UdpSocket};
use embassy_net::Stack;
use embassy_time::Duration;

/// Simple date/time structure (no external chrono dependency)
#[derive(Clone, Debug)]
pub struct DateTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub weekday: u8, // 0=Sun, 1=Mon, ..., 6=Sat
}

impl DateTime {
    pub fn weekday_str(&self) -> &'static str {
        match self.weekday {
            0 => "Sun",
            1 => "Mon",
            2 => "Tue",
            3 => "Wed",
            4 => "Thu",
            5 => "Fri",
            6 => "Sat",
            _ => "???",
        }
    }
}

// NTP epoch (1900-01-01) to Unix epoch (1970-01-01) offset in seconds
const NTP_TO_UNIX: u32 = 2_208_988_800;

/// Query NTP server and return current time with given UTC offset (in seconds)
pub async fn get_time(stack: Stack<'_>, utc_offset_secs: i32) -> Result<DateTime, ()> {
    // Resolve NTP server
    info!("Resolving NTP server...");
    let addrs = stack
        .dns_query("pool.ntp.org", DnsQueryType::A)
        .await
        .map_err(|e| {
            error!("NTP DNS failed: {}", e);
        })?;
    let server_addr = *addrs.first().ok_or(())?;
    info!("NTP server: {}", server_addr);

    // Create UDP socket
    let mut rx_meta = [PacketMetadata::EMPTY; 1];
    let mut rx_buf = [0u8; 128];
    let mut tx_meta = [PacketMetadata::EMPTY; 1];
    let mut tx_buf = [0u8; 128];
    let mut socket = UdpSocket::new(stack, &mut rx_meta, &mut rx_buf, &mut tx_meta, &mut tx_buf);
    socket.bind(56123).map_err(|_| {
        error!("UDP bind failed");
    })?;

    // Build NTP request (48 bytes, version 3, client mode)
    let mut packet = [0u8; 48];
    packet[0] = 0x1B; // LI=0, VN=3, Mode=3 (Client)

    // Send request
    socket
        .send_to(&packet, (server_addr, 123))
        .await
        .map_err(|_| {
            error!("NTP send failed");
        })?;

    // Receive response with timeout
    let mut resp = [0u8; 48];
    let recv_result = embassy_time::with_timeout(Duration::from_secs(5), socket.recv_from(&mut resp)).await;

    let (n, _) = match recv_result {
        Ok(Ok(r)) => r,
        Ok(Err(_)) => {
            error!("NTP recv error");
            return Err(());
        }
        Err(_) => {
            error!("NTP recv timeout");
            return Err(());
        }
    };

    if n < 48 {
        error!("NTP response too short: {} bytes", n);
        return Err(());
    }

    // Extract transmit timestamp (seconds) from bytes 40-43
    let secs = u32::from_be_bytes([resp[40], resp[41], resp[42], resp[43]]);
    if secs < NTP_TO_UNIX {
        error!("NTP timestamp invalid");
        return Err(());
    }

    let unix_secs = secs - NTP_TO_UNIX;
    // wrapping_add handles negative offsets correctly via two's complement
    let local_secs = unix_secs.wrapping_add(utc_offset_secs as u32);

    Ok(unix_to_datetime(local_secs))
}

/// Convert Unix timestamp to DateTime
pub fn unix_to_datetime(secs: u32) -> DateTime {
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hour = (time_of_day / 3600) as u8;
    let minute = ((time_of_day % 3600) / 60) as u8;
    let second = (time_of_day % 60) as u8;

    // Day of week: Jan 1, 1970 was Thursday (4)
    let weekday = ((days + 4) % 7) as u8;

    // Calculate year, month, day
    let (year, month, day) = days_to_date(days);

    DateTime {
        year,
        month,
        day,
        hour,
        minute,
        second,
        weekday,
    }
}

/// Convert days since Unix epoch to (year, month, day)
fn days_to_date(mut days: u32) -> (u16, u8, u8) {
    let mut year = 1970u16;

    loop {
        let days_in_year = if is_leap(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }

    let leap = is_leap(year);
    let month_days: [u32; 12] = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];

    let mut month = 0u8;
    for (i, &md) in month_days.iter().enumerate() {
        if days < md {
            month = (i + 1) as u8;
            break;
        }
        days -= md;
    }

    (year, month, (days + 1) as u8)
}

fn is_leap(year: u16) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}
