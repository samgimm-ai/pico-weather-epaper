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

// ═══ 24-Hour (Today) Forecast ═══

pub struct HourlyEntry {
    pub hour: u8,
    pub temp: i16,
    pub icon_code: String<8>,
}

impl HourlyEntry {
    const fn empty() -> Self {
        Self {
            hour: 0,
            temp: 0,
            icon_code: String::new(),
        }
    }
}

pub struct TodayForecast {
    pub entries: [HourlyEntry; 8],
    pub count: u8,
}

/// Fetch 24-hour forecast (next 8 × 3-hour entries) from OpenWeatherMap
pub async fn get_today_forecast(
    stack: Stack<'_>,
    lat: &str,
    lon: &str,
    utc_offset_secs: i32,
) -> Result<TodayForecast, ()> {
    info!("Fetching 24-hour forecast...");
    let addrs = stack
        .dns_query("api.openweathermap.org", DnsQueryType::A)
        .await
        .map_err(|e| {
            error!("Today forecast DNS failed: {}", e);
        })?;
    let server_addr = *addrs.first().ok_or(())?;

    let mut request: String<512> = String::new();
    {
        use core::fmt::Write as FmtWrite;
        FmtWrite::write_fmt(
            &mut request,
            format_args!(
                "GET /data/2.5/forecast?lat={}&lon={}&units=metric&appid={} HTTP/1.1\r\n\
                 Host: api.openweathermap.org\r\n\
                 Connection: close\r\n\
                 \r\n",
                lat,
                lon,
                crate::config::OPENWEATHER_API_KEY,
            ),
        )
        .map_err(|_| {
            error!("Today forecast request string overflow");
        })?;
    }

    let mut rx_buf = [0u8; 4096];
    let mut tx_buf = [0u8; 1024];
    let mut socket = TcpSocket::new(stack, &mut rx_buf, &mut tx_buf);
    socket.set_timeout(Some(Duration::from_secs(20)));

    socket.connect((server_addr, 80)).await.map_err(|e| {
        error!("Today forecast TCP connect failed: {}", e);
    })?;

    socket.write_all(request.as_bytes()).await.map_err(|e| {
        error!("Today forecast TCP write failed: {}", e);
    })?;

    let mut response = [0u8; 20480];
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
                error!("Today forecast TCP read failed: {}", e);
                return Err(());
            }
        }
    }

    info!("Today forecast received {} bytes", pos);

    if pos >= response.len() {
        error!("Today forecast buffer full ({} bytes)", response.len());
        return Err(());
    }

    let status_code = parse_status_code(&response[..pos]).ok_or_else(|| {
        error!("Today forecast HTTP status not found");
    })?;

    if status_code != 200 {
        error!("Today forecast HTTP error: {}", status_code);
        return Err(());
    }

    let body_start = find_body_start(&response[..pos]).ok_or_else(|| {
        error!("Today forecast HTTP header end not found");
    })?;

    let json = &response[body_start..pos];
    info!("Today forecast JSON length: {} bytes", json.len());

    parse_today(json, utc_offset_secs)
}

fn parse_today(json: &[u8], utc_offset_secs: i32) -> Result<TodayForecast, ()> {
    let list_start = find_bytes(json, b"\"list\"", 0).ok_or_else(|| {
        error!("Today forecast: \"list\" not found");
    })?;
    let arr_start = find_bytes(json, b"[", list_start).ok_or_else(|| {
        error!("Today forecast: list array not found");
    })?;

    let arr_end = {
        let mut depth = 0u32;
        let mut end = None;
        for i in arr_start..json.len() {
            match json[i] {
                b'[' => depth += 1,
                b']' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }
        end.ok_or_else(|| {
            error!("Today forecast: list array end not found");
        })?
    };

    let mut entries: [HourlyEntry; 8] = [
        HourlyEntry::empty(), HourlyEntry::empty(),
        HourlyEntry::empty(), HourlyEntry::empty(),
        HourlyEntry::empty(), HourlyEntry::empty(),
        HourlyEntry::empty(), HourlyEntry::empty(),
    ];
    let mut count: u8 = 0;

    let mut cursor = arr_start + 1;
    loop {
        if count >= 8 {
            break;
        }

        let obj_start = match find_bytes(json, b"{", cursor) {
            Some(p) if p < arr_end => p,
            _ => break,
        };

        // Extract "dt"
        let dt_key = match find_bytes(json, b"\"dt\"", obj_start) {
            Some(p) => p,
            None => break,
        };
        let colon = match find_bytes(json, b":", dt_key + 4) {
            Some(p) => p,
            None => break,
        };
        let (dt, _) = match parse_u32_at(json, colon + 1) {
            Some(v) => v,
            None => break,
        };

        // Extract "temp" from "main"
        let main_key = match find_bytes(json, b"\"main\"", dt_key) {
            Some(p) => p,
            None => break,
        };
        let temp_key = match find_bytes(json, b"\"temp\"", main_key) {
            Some(p) => p,
            None => break,
        };
        let temp_colon = match find_bytes(json, b":", temp_key + 6) {
            Some(p) => p,
            None => break,
        };
        let (temp_f, _) = match parse_float_at(json, temp_colon + 1) {
            Some(v) => v,
            None => break,
        };
        let temp = if temp_f >= 0.0 {
            (temp_f + 0.5) as i16
        } else {
            (temp_f - 0.5) as i16
        };

        // Extract icon
        let weather_key = match find_bytes(json, b"\"weather\"", main_key) {
            Some(p) => p,
            None => break,
        };
        let (icon_bytes, after_icon) = match extract_string_after(json, b"\"icon\"", weather_key) {
            Some(v) => v,
            None => break,
        };

        // Compute local hour from dt
        let local_secs = dt.wrapping_add(utc_offset_secs as u32);
        let dt_info = crate::ntp::unix_to_datetime(local_secs);

        let idx = count as usize;
        entries[idx].hour = dt_info.hour;
        entries[idx].temp = temp;
        let icon_str = core::str::from_utf8(icon_bytes).unwrap_or("03d");
        let _ = entries[idx].icon_code.push_str(icon_str);

        count += 1;
        cursor = after_icon;
    }

    info!("Today forecast parsed: {} entries", count);
    Ok(TodayForecast { entries, count })
}

// ═══ 5-Day Forecast ═══

pub struct ForecastDay {
    pub weekday: u8,
    pub month: u8,
    pub day: u8,
    pub temp_min: i16,
    pub temp_max: i16,
    pub icon_code: String<8>,
}

pub struct ForecastData {
    pub days: [ForecastDay; 5],
    pub count: u8,
}

impl ForecastDay {
    const fn empty() -> Self {
        Self {
            weekday: 0,
            month: 0,
            day: 0,
            temp_min: 0,
            temp_max: 0,
            icon_code: String::new(),
        }
    }
}

/// Fetch 5-day forecast from OpenWeatherMap
pub async fn get_forecast(
    stack: Stack<'_>,
    lat: &str,
    lon: &str,
    utc_offset_secs: i32,
) -> Result<ForecastData, ()> {
    info!("Fetching 5-day forecast...");
    let addrs = stack
        .dns_query("api.openweathermap.org", DnsQueryType::A)
        .await
        .map_err(|e| {
            error!("Forecast DNS failed: {}", e);
        })?;
    let server_addr = *addrs.first().ok_or(())?;

    let mut request: String<512> = String::new();
    {
        use core::fmt::Write as FmtWrite;
        FmtWrite::write_fmt(
            &mut request,
            format_args!(
                "GET /data/2.5/forecast?lat={}&lon={}&units=metric&appid={} HTTP/1.1\r\n\
                 Host: api.openweathermap.org\r\n\
                 Connection: close\r\n\
                 \r\n",
                lat,
                lon,
                crate::config::OPENWEATHER_API_KEY,
            ),
        )
        .map_err(|_| {
            error!("Forecast request string overflow");
        })?;
    }

    let mut rx_buf = [0u8; 4096];
    let mut tx_buf = [0u8; 1024];
    let mut socket = TcpSocket::new(stack, &mut rx_buf, &mut tx_buf);
    socket.set_timeout(Some(Duration::from_secs(20)));

    socket.connect((server_addr, 80)).await.map_err(|e| {
        error!("Forecast TCP connect failed: {}", e);
    })?;

    socket.write_all(request.as_bytes()).await.map_err(|e| {
        error!("Forecast TCP write failed: {}", e);
    })?;

    let mut response = [0u8; 20480];
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
                error!("Forecast TCP read failed: {}", e);
                return Err(());
            }
        }
    }

    info!("Forecast received {} bytes", pos);

    if pos >= response.len() {
        error!("Forecast buffer full ({} bytes)", response.len());
        return Err(());
    }

    let status_code = parse_status_code(&response[..pos]).ok_or_else(|| {
        error!("Forecast HTTP status not found");
    })?;

    if status_code != 200 {
        error!("Forecast HTTP error: {}", status_code);
        return Err(());
    }

    let body_start = find_body_start(&response[..pos]).ok_or_else(|| {
        error!("Forecast HTTP header end not found");
    })?;

    let json = &response[body_start..pos];
    info!("Forecast JSON length: {} bytes", json.len());

    parse_forecast(json, utc_offset_secs)
}

/// Find byte pattern in slice, starting from `from`
fn find_bytes(data: &[u8], pattern: &[u8], from: usize) -> Option<usize> {
    if pattern.is_empty() || from + pattern.len() > data.len() {
        return None;
    }
    for i in from..=data.len() - pattern.len() {
        if &data[i..i + pattern.len()] == pattern {
            return Some(i);
        }
    }
    None
}

/// Parse a u32 from decimal digits at position in byte slice
fn parse_u32_at(data: &[u8], pos: usize) -> Option<(u32, usize)> {
    let mut val: u32 = 0;
    let mut i = pos;
    // Skip whitespace
    while i < data.len() && (data[i] == b' ' || data[i] == b'\t') {
        i += 1;
    }
    let start = i;
    while i < data.len() && data[i].is_ascii_digit() {
        val = val * 10 + (data[i] - b'0') as u32;
        i += 1;
    }
    if i == start {
        None
    } else {
        Some((val, i))
    }
}

/// Parse a float (possibly negative) at position, return as f32
fn parse_float_at(data: &[u8], pos: usize) -> Option<(f32, usize)> {
    let mut i = pos;
    while i < data.len() && (data[i] == b' ' || data[i] == b'\t') {
        i += 1;
    }
    let negative = if i < data.len() && data[i] == b'-' {
        i += 1;
        true
    } else {
        false
    };
    let start = i;
    let mut int_part: i32 = 0;
    while i < data.len() && data[i].is_ascii_digit() {
        int_part = int_part * 10 + (data[i] - b'0') as i32;
        i += 1;
    }
    let mut frac: f32 = 0.0;
    if i < data.len() && data[i] == b'.' {
        i += 1;
        let mut divisor: f32 = 10.0;
        while i < data.len() && data[i].is_ascii_digit() {
            frac += (data[i] - b'0') as f32 / divisor;
            divisor *= 10.0;
            i += 1;
        }
    }
    if i == start {
        return None;
    }
    let val = int_part as f32 + frac;
    Some((if negative { -val } else { val }, i))
}

/// Extract a quoted string value after a key like `"icon":"01d"`
fn extract_string_after<'a>(data: &'a [u8], key: &[u8], from: usize) -> Option<(&'a [u8], usize)> {
    let key_pos = find_bytes(data, key, from)?;
    // Find the opening quote of the value
    let colon = find_bytes(data, b":", key_pos + key.len())?;
    let open_q = find_bytes(data, b"\"", colon + 1)?;
    let close_q = find_bytes(data, b"\"", open_q + 1)?;
    Some((&data[open_q + 1..close_q], close_q + 1))
}

/// Track per-day aggregation during parsing
struct DayAccum {
    day_number: u32, // (dt + offset) / 86400
    temp_min: i16,
    temp_max: i16,
    best_icon: [u8; 8],
    best_icon_len: usize,
    best_noon_dist: u32,
    dt_first: u32, // first dt in this day (used for weekday/date)
}

fn parse_forecast(json: &[u8], utc_offset_secs: i32) -> Result<ForecastData, ()> {
    let list_start = find_bytes(json, b"\"list\"", 0).ok_or_else(|| {
        error!("Forecast: \"list\" not found");
    })?;
    let arr_start = find_bytes(json, b"[", list_start).ok_or_else(|| {
        error!("Forecast: list array not found");
    })?;

    // Find the end of the list array by bracket-matching
    let arr_end = {
        let mut depth = 0u32;
        let mut end = None;
        for i in arr_start..json.len() {
            match json[i] {
                b'[' => depth += 1,
                b']' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }
        end.ok_or_else(|| {
            error!("Forecast: list array end not found");
        })?
    };

    let mut days: [ForecastDay; 5] = [
        ForecastDay::empty(),
        ForecastDay::empty(),
        ForecastDay::empty(),
        ForecastDay::empty(),
        ForecastDay::empty(),
    ];
    let mut accum: [Option<DayAccum>; 6] = [None, None, None, None, None, None];
    let mut day_count: usize = 0;

    // Iterate through list entries — each starts with `{` and has "dt":, "main":{"temp":}, "weather":[{"icon":}]
    let mut cursor = arr_start + 1;
    loop {
        // Find next entry object within list array bounds
        let obj_start = match find_bytes(json, b"{", cursor) {
            Some(p) if p < arr_end => p,
            _ => break,
        };

        // Extract "dt" value
        let dt_key = match find_bytes(json, b"\"dt\"", obj_start) {
            Some(p) => p,
            None => break,
        };
        let colon = match find_bytes(json, b":", dt_key + 4) {
            Some(p) => p,
            None => break,
        };
        let (dt, _) = match parse_u32_at(json, colon + 1) {
            Some(v) => v,
            None => break,
        };

        // Extract "temp" from "main" object
        let main_key = match find_bytes(json, b"\"main\"", dt_key) {
            Some(p) => p,
            None => break,
        };
        let temp_key = match find_bytes(json, b"\"temp\"", main_key) {
            Some(p) => p,
            None => break,
        };
        let temp_colon = match find_bytes(json, b":", temp_key + 6) {
            Some(p) => p,
            None => break,
        };
        let (temp_f, _) = match parse_float_at(json, temp_colon + 1) {
            Some(v) => v,
            None => break,
        };
        let temp = if temp_f >= 0.0 {
            (temp_f + 0.5) as i16
        } else {
            (temp_f - 0.5) as i16
        };

        // Extract icon from weather array
        let weather_key = match find_bytes(json, b"\"weather\"", main_key) {
            Some(p) => p,
            None => break,
        };
        let (icon_bytes, after_icon) = match extract_string_after(json, b"\"icon\"", weather_key) {
            Some(v) => v,
            None => break,
        };

        // Determine which day this entry belongs to
        let local_secs = dt.wrapping_add(utc_offset_secs as u32);
        let day_number = local_secs / 86400;
        let time_of_day = local_secs % 86400;
        let noon_dist = if time_of_day >= 43200 {
            time_of_day - 43200
        } else {
            43200 - time_of_day
        };

        // Find or create accumulator for this day
        let mut found_idx: Option<usize> = None;
        for i in 0..day_count {
            if let Some(ref a) = accum[i] {
                if a.day_number == day_number {
                    found_idx = Some(i);
                    break;
                }
            }
        }

        match found_idx {
            Some(idx) => {
                if let Some(ref mut a) = accum[idx] {
                    if temp < a.temp_min {
                        a.temp_min = temp;
                    }
                    if temp > a.temp_max {
                        a.temp_max = temp;
                    }
                    if noon_dist < a.best_noon_dist {
                        a.best_noon_dist = noon_dist;
                        let len = icon_bytes.len().min(8);
                        a.best_icon[..len].copy_from_slice(&icon_bytes[..len]);
                        a.best_icon_len = len;
                    }
                }
            }
            None => {
                if day_count < 6 {
                    let mut icon = [0u8; 8];
                    let len = icon_bytes.len().min(8);
                    icon[..len].copy_from_slice(&icon_bytes[..len]);
                    accum[day_count] = Some(DayAccum {
                        day_number,
                        temp_min: temp,
                        temp_max: temp,
                        best_icon: icon,
                        best_icon_len: len,
                        best_noon_dist: noon_dist,
                        dt_first: dt,
                    });
                    day_count += 1;
                }
            }
        }

        // Move cursor past this entry
        cursor = after_icon;
    }

    // Skip the first partial day (today) — take days[1..=5]
    let skip = if day_count > 5 { 1 } else { 0 };
    let mut result_count: u8 = 0;

    for i in skip..day_count.min(skip + 5) {
        if let Some(ref a) = accum[i] {
            let local_first = a.dt_first.wrapping_add(utc_offset_secs as u32);
            let dt = crate::ntp::unix_to_datetime(local_first);
            let idx = result_count as usize;
            days[idx].weekday = dt.weekday;
            days[idx].month = dt.month;
            days[idx].day = dt.day;
            days[idx].temp_min = a.temp_min;
            days[idx].temp_max = a.temp_max;
            let icon_str = core::str::from_utf8(&a.best_icon[..a.best_icon_len]).unwrap_or("03d");
            let _ = days[idx].icon_code.push_str(icon_str);
            result_count += 1;
        }
    }

    info!("Forecast parsed: {} days", result_count);
    Ok(ForecastData {
        days,
        count: result_count,
    })
}
