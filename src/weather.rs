use defmt::*;
use embassy_net::dns::DnsQueryType;
use embassy_net::tcp::TcpSocket;
use embassy_net::Stack;
use embassy_time::Duration;
use embedded_io_async::Write as AsyncWrite;
use heapless::String;
use serde::Deserialize;

/// Parsed weather data
pub struct WeatherData {
    /// Temperature as integer (rounded from float)
    pub temp_int: i16,
    /// Weather main description (e.g. "Clear", "Clouds", "Rain")
    pub description: String<32>,
    /// Icon code from API (e.g. "01d", "02n", "10d")
    pub icon_code: String<8>,
    /// Humidity percentage (0-100)
    pub humidity: u8,
    /// Wind speed × 10 as integer (e.g. 2.5 m/s → 25), avoids f32 formatting
    pub wind_speed_10x: u16,
}

// Serde structs for JSON parsing
#[derive(Deserialize)]
struct ApiResponse<'a> {
    #[serde(borrow)]
    weather: [WeatherEntry<'a>; 1],
    main: MainEntry,
    wind: WindEntry,
}

#[derive(Deserialize)]
struct WeatherEntry<'a> {
    main: &'a str,
    icon: &'a str,
}

#[derive(Deserialize)]
struct MainEntry {
    temp: f32,
    humidity: u8,
}

#[derive(Deserialize)]
struct WindEntry {
    speed: f32,
}

/// Fetch current weather from OpenWeatherMap API
pub async fn get_weather(stack: Stack<'_>, lat: &str, lon: &str) -> Result<WeatherData, ()> {
    info!("Resolving weather API host...");
    let addrs = stack
        .dns_query("api.openweathermap.org", DnsQueryType::A)
        .await
        .map_err(|e| {
            error!("DNS failed: {}", e);
        })?;
    let server_addr = *addrs.first().ok_or(())?;
    info!("Weather API addr: {}", server_addr);

    // Build HTTP request
    let mut request: String<512> = String::new();
    {
        use core::fmt::Write as FmtWrite;
        FmtWrite::write_fmt(
            &mut request,
            format_args!(
                "GET /data/2.5/weather?lat={}&lon={}&units=metric&appid={} HTTP/1.1\r\n\
                 Host: api.openweathermap.org\r\n\
                 Connection: close\r\n\
                 \r\n",
                lat,
                lon,
                crate::config::OPENWEATHER_API_KEY,
            ),
        )
        .map_err(|_| {
            error!("Request string overflow");
        })?;
    }

    // TCP connection
    let mut rx_buf = [0u8; 4096];
    let mut tx_buf = [0u8; 1024];
    let mut socket = TcpSocket::new(stack, &mut rx_buf, &mut tx_buf);
    socket.set_timeout(Some(Duration::from_secs(15)));

    socket.connect((server_addr, 80)).await.map_err(|e| {
        error!("TCP connect failed: {}", e);
    })?;

    // Send request
    socket.write_all(request.as_bytes()).await.map_err(|e| {
        error!("TCP write failed: {}", e);
    })?;

    // Read response
    let mut response = [0u8; 4096];
    let mut pos = 0;
    loop {
        if pos >= response.len() {
            break;
        }
        match socket.read(&mut response[pos..]).await {
            Ok(0) => break,
            Ok(n) => pos += n,
            Err(e) => {
                if pos > 0 {
                    break;
                }
                error!("TCP read failed: {}", e);
                return Err(());
            }
        }
    }

    info!("Received {} bytes", pos);

    if pos >= response.len() {
        error!("Response buffer full ({} bytes) — response likely truncated", response.len());
        return Err(());
    }

    // Parse HTTP status code from first line (e.g. "HTTP/1.1 200 OK\r\n")
    let status_code = parse_status_code(&response[..pos]).ok_or_else(|| {
        error!("HTTP status line not found");
    })?;

    if status_code != 200 {
        match status_code {
            401 => error!("HTTP 401: Invalid API key"),
            404 => error!("HTTP 404: City not found"),
            429 => error!("HTTP 429: Rate limit exceeded"),
            _ => error!("HTTP error: {}", status_code),
        }
        return Err(());
    }

    // Find JSON body (after \r\n\r\n)
    let body_start = find_body_start(&response[..pos]).ok_or_else(|| {
        error!("HTTP header end not found");
    })?;

    let json = &response[body_start..pos];
    info!("JSON length: {} bytes", json.len());

    // Parse JSON
    let (resp, _): (ApiResponse, _) = serde_json_core::from_slice(json).map_err(|_| {
        error!("JSON parse failed");
    })?;

    let mut description: String<32> = String::new();
    let _ = description.push_str(resp.weather[0].main);

    let mut icon_code: String<8> = String::new();
    let _ = icon_code.push_str(resp.weather[0].icon);

    // Manual rounding (f32::round() not available in no_std without libm)
    let temp_int = if resp.main.temp >= 0.0 {
        (resp.main.temp + 0.5) as i16
    } else {
        (resp.main.temp - 0.5) as i16
    };

    let humidity = resp.main.humidity;

    // Store wind speed as integer × 10 to avoid f32 formatting
    let wind_speed_10x = (resp.wind.speed * 10.0 + 0.5) as u16;

    info!(
        "Weather: {}C, {}, icon={}, humidity={}%, wind={}.{}m/s",
        temp_int,
        description.as_str(),
        icon_code.as_str(),
        humidity,
        wind_speed_10x / 10,
        wind_speed_10x % 10,
    );

    Ok(WeatherData {
        temp_int,
        description,
        icon_code,
        humidity,
        wind_speed_10x,
    })
}

/// Parse HTTP status code from response (e.g. "HTTP/1.1 200 OK" → 200)
fn parse_status_code(data: &[u8]) -> Option<u16> {
    // Format: "HTTP/1.x SSS ..."
    if data.len() < 12 || &data[..5] != b"HTTP/" {
        return None;
    }
    // Status code starts at byte 9 (after "HTTP/1.1 ")
    let code_bytes = &data[9..12];
    let mut code: u16 = 0;
    for &b in code_bytes {
        if !b.is_ascii_digit() {
            return None;
        }
        code = code * 10 + (b - b'0') as u16;
    }
    Some(code)
}

/// Find the start of HTTP body (after \r\n\r\n)
fn find_body_start(data: &[u8]) -> Option<usize> {
    for i in 0..data.len().saturating_sub(3) {
        if &data[i..i + 4] == b"\r\n\r\n" {
            return Some(i + 4);
        }
    }
    None
}
