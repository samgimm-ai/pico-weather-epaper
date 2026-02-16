use core::fmt::Write as FmtWrite;

use defmt::*;
use embassy_rp::gpio::{Input, Output};
use embassy_rp::spi::Spi;
use embassy_time::{Duration, Timer};
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{Line, PrimitiveStyle};
use embedded_graphics::text::Text;
use embedded_hal::spi::SpiBus;
use heapless::String;
use profont::{PROFONT_10_POINT, PROFONT_12_POINT, PROFONT_24_POINT};

use crate::icons;
use crate::korean_font;
use crate::ntp::DateTime;
use crate::settings::Settings;
use crate::weather::WeatherData;

// Display dimensions (landscape orientation)
pub const WIDTH: u32 = 296;
pub const HEIGHT: u32 = 128;

// Internal buffer dimensions (portrait, matching SSD1680 native)
const BUF_W: u32 = 136;
const BUF_H: u32 = 296;
const BUF_BYTES_PER_ROW: u32 = BUF_W / 8; // 17
const BUF_SIZE: usize = (BUF_BYTES_PER_ROW * BUF_H) as usize; // 5032

/// Framebuffer for the e-paper display with landscape coordinate mapping
pub struct DisplayBuffer {
    buf: [u8; BUF_SIZE],
}

impl DisplayBuffer {
    pub fn new() -> Self {
        Self {
            buf: [0xFF; BUF_SIZE],
        }
    }

    pub fn clear(&mut self) {
        self.buf.fill(0xFF);
    }

    pub fn buffer(&self) -> &[u8] {
        &self.buf
    }

    /// Invert all pixels (for inverted display mode)
    pub fn invert(&mut self) {
        for b in self.buf.iter_mut() {
            *b = !*b;
        }
    }
}

impl DrawTarget for DisplayBuffer {
    type Color = BinaryColor;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<BinaryColor>>,
    {
        for Pixel(coord, color) in pixels {
            let x = coord.x;
            let y = coord.y;

            if x < 0 || y < 0 || x >= WIDTH as i32 || y >= HEIGHT as i32 {
                continue;
            }

            let buf_x = (BUF_W as i32 - 1 - y - 3) as u32;
            let buf_y = x as u32;

            let byte_idx = (buf_y * BUF_BYTES_PER_ROW + buf_x / 8) as usize;
            let bit_mask = 0x80 >> (buf_x % 8);

            if byte_idx < BUF_SIZE {
                match color {
                    BinaryColor::Off => self.buf[byte_idx] |= bit_mask,
                    BinaryColor::On => self.buf[byte_idx] &= !bit_mask,
                }
            }
        }
        Ok(())
    }
}

impl OriginDimensions for DisplayBuffer {
    fn size(&self) -> Size {
        Size::new(WIDTH, HEIGHT)
    }
}

// --- SSD1680 e-Paper Driver ---

/// SSD1680 e-paper display driver (Waveshare 2.9" V2, 296x128)
pub struct Epd<'a> {
    spi: Spi<'a, embassy_rp::peripherals::SPI1, embassy_rp::spi::Blocking>,
    cs: Output<'a>,
    dc: Output<'a>,
    rst: Output<'a>,
    busy: Input<'a>,
}

// Partial refresh LUT from Waveshare reference (159 bytes)
// Bytes 0-152: cmd 0x32 (LUT register)
// Byte 153: cmd 0x3F, Byte 154: cmd 0x03, Bytes 155-157: cmd 0x04, Byte 158: cmd 0x2C
#[rustfmt::skip]
const PARTIAL_LUT: [u8; 159] = [
    // VS waveform (60 bytes, 5 phases x 12)
    0x00,0x40,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,
    0x80,0x80,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,
    0x40,0x40,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,
    0x00,0x80,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,
    0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,
    // TP timing (84 bytes, 12 groups x 7)
    0x0A,0x00,0x00,0x00,0x00,0x00,0x00,
    0x01,0x00,0x00,0x00,0x00,0x00,0x00,
    0x01,0x00,0x00,0x00,0x00,0x00,0x00,
    0x00,0x00,0x00,0x00,0x00,0x00,0x00,
    0x00,0x00,0x00,0x00,0x00,0x00,0x00,
    0x00,0x00,0x00,0x00,0x00,0x00,0x00,
    0x00,0x00,0x00,0x00,0x00,0x00,0x00,
    0x00,0x00,0x00,0x00,0x00,0x00,0x00,
    0x00,0x00,0x00,0x00,0x00,0x00,0x00,
    0x00,0x00,0x00,0x00,0x00,0x00,0x00,
    0x00,0x00,0x00,0x00,0x00,0x00,0x00,
    0x00,0x00,0x00,0x00,0x00,0x00,0x00,
    // FR/XON (9 bytes)
    0x22,0x22,0x22,0x22,0x22,0x22,0x00,0x00,0x00,
    // Extra registers: 0x3F, 0x03, 0x04(3), 0x2C (6 bytes)
    0x22,0x17,0x41,0xB0,0x32,0x36,
];

impl<'a> Epd<'a> {
    pub fn new(
        spi: Spi<'a, embassy_rp::peripherals::SPI1, embassy_rp::spi::Blocking>,
        cs: Output<'a>,
        dc: Output<'a>,
        rst: Output<'a>,
        busy: Input<'a>,
    ) -> Self {
        Self { spi, cs, dc, rst, busy }
    }

    /// Full initialization for full refresh mode
    pub async fn init(&mut self) {
        self.hw_reset().await;

        // Software reset
        self.cmd(0x12).await;
        self.wait_busy().await;

        // Driver Output Control: 296 lines (0x0127 = 295)
        self.cmd_data(0x01, &[0x27, 0x01, 0x00]).await;

        // Data Entry Mode: Y increment, X increment
        self.cmd_data(0x11, &[0x03]).await;

        // Set RAM X address range: 0 to 16 (136 pixels)
        self.cmd_data(0x44, &[0x00, 0x10]).await;

        // Set RAM Y address range: 0 to 295
        self.cmd_data(0x45, &[0x00, 0x00, 0x27, 0x01]).await;

        // Border waveform control (0x50 = fix level VSH1 = white border)
        self.cmd_data(0x3C, &[0x50]).await;

        // Fill both RAMs with white (0xFF)
        let white_row = [0xFFu8; 17];
        for ram_cmd in [0x24u8, 0x26u8] {
            self.cmd_data(0x4E, &[0x00]).await;
            self.cmd_data(0x4F, &[0x00, 0x00]).await;
            self.cmd(ram_cmd).await;
            self.cs.set_low();
            self.dc.set_high();
            for _ in 0..296u16 {
                let _ = SpiBus::write(&mut self.spi, &white_row);
            }
            self.cs.set_high();
        }

        // Temperature sensor control (internal)
        self.cmd_data(0x18, &[0x80]).await;

        // Display Update Control 2: load temperature + LUT
        self.cmd_data(0x22, &[0xB1]).await;
        self.cmd(0x20).await;
        self.wait_busy().await;

        // Set RAM counters to start
        self.cmd_data(0x4E, &[0x00]).await;
        self.cmd_data(0x4F, &[0x00, 0x00]).await;
        self.wait_busy().await;

        info!("SSD1680 initialized (full mode)");
    }

    /// Prepare for partial refresh mode.
    /// Call after full refresh — loads partial LUT and enables clock/analog.
    /// Display must NOT be in deep sleep.
    pub async fn init_partial(&mut self) {
        // Load partial LUT (153 bytes to cmd 0x32)
        self.cmd(0x32).await;
        self.cs.set_low();
        self.dc.set_high();
        let _ = SpiBus::write(&mut self.spi, &PARTIAL_LUT[..153]);
        self.cs.set_high();
        self.wait_busy().await;

        // Extra LUT registers
        self.cmd_data(0x3F, &[PARTIAL_LUT[153]]).await;
        self.cmd_data(0x03, &[PARTIAL_LUT[154]]).await;
        self.cmd_data(0x04, &PARTIAL_LUT[155..158]).await;
        self.cmd_data(0x2C, &[PARTIAL_LUT[158]]).await;

        // Display option register
        self.cmd_data(0x37, &[0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00, 0x00]).await;

        // Border for partial mode
        self.cmd_data(0x3C, &[0x80]).await;

        // Prepare: enable clock + analog
        self.cmd_data(0x22, &[0xC0]).await;
        self.cmd(0x20).await;
        self.wait_busy().await;

        info!("SSD1680 partial mode ready");
    }

    /// Write framebuffer to display and trigger full refresh
    pub async fn update(&mut self, buffer: &[u8]) {
        // Write to Black/White RAM (current frame)
        self.cmd_data(0x4E, &[0x00]).await;
        self.cmd_data(0x4F, &[0x00, 0x00]).await;
        self.wait_busy().await;

        self.cmd(0x24).await;
        self.cs.set_low();
        self.dc.set_high();
        for chunk in buffer.chunks(256) {
            let _ = SpiBus::write(&mut self.spi, chunk);
        }
        self.cs.set_high();

        // Write same data to Old RAM to prevent border noise
        self.cmd_data(0x4E, &[0x00]).await;
        self.cmd_data(0x4F, &[0x00, 0x00]).await;
        self.wait_busy().await;
        self.cmd(0x26).await;
        self.cs.set_low();
        self.dc.set_high();
        for chunk in buffer.chunks(256) {
            let _ = SpiBus::write(&mut self.spi, chunk);
        }
        self.cs.set_high();

        // Trigger full refresh
        self.cmd_data(0x22, &[0xF7]).await;
        self.cmd(0x20).await;
        self.wait_busy().await;

        info!("Full refresh done");
    }

    /// Partial refresh — writes new frame to RAM 0x24, triggers partial update,
    /// then copies to RAM 0x26 so next partial has correct "old" reference.
    /// Requires init_partial() to have been called first.
    /// Do NOT call sleep() between partial refreshes — RAMs must persist.
    pub async fn update_partial(&mut self, buffer: &[u8]) {
        // Set RAM window and cursor
        self.cmd_data(0x44, &[0x00, 0x10]).await;
        self.cmd_data(0x45, &[0x00, 0x00, 0x27, 0x01]).await;
        self.cmd_data(0x4E, &[0x00]).await;
        self.cmd_data(0x4F, &[0x00, 0x00]).await;
        self.wait_busy().await;

        // Write new frame to RAM 0x24 ONLY
        self.cmd(0x24).await;
        self.cs.set_low();
        self.dc.set_high();
        for chunk in buffer.chunks(256) {
            let _ = SpiBus::write(&mut self.spi, chunk);
        }
        self.cs.set_high();

        // Trigger partial update (compares RAM 0x24 new vs RAM 0x26 old)
        self.cmd_data(0x22, &[0x0F]).await;
        self.cmd(0x20).await;
        self.wait_busy().await;

        // Copy new frame to RAM 0x26 (becomes "old" for next partial refresh)
        self.cmd_data(0x4E, &[0x00]).await;
        self.cmd_data(0x4F, &[0x00, 0x00]).await;
        self.cmd(0x26).await;
        self.cs.set_low();
        self.dc.set_high();
        for chunk in buffer.chunks(256) {
            let _ = SpiBus::write(&mut self.spi, chunk);
        }
        self.cs.set_high();

        info!("Partial refresh done");
    }

    /// Put display into deep sleep mode
    pub async fn sleep(&mut self) {
        self.cmd_data(0x10, &[0x01]).await;
        Timer::after(Duration::from_millis(100)).await;
        info!("Display sleeping");
    }

    // --- Low-level SPI helpers ---

    async fn hw_reset(&mut self) {
        self.rst.set_high();
        Timer::after(Duration::from_millis(10)).await;
        self.rst.set_low();
        Timer::after(Duration::from_millis(10)).await;
        self.rst.set_high();
        Timer::after(Duration::from_millis(10)).await;
        self.wait_busy().await;
    }

    async fn cmd(&mut self, command: u8) {
        self.cs.set_low();
        self.dc.set_low();
        let _ = SpiBus::write(&mut self.spi, &[command]);
        self.cs.set_high();
    }

    async fn cmd_data(&mut self, command: u8, data: &[u8]) {
        self.cs.set_low();
        self.dc.set_low();
        let _ = SpiBus::write(&mut self.spi, &[command]);
        self.dc.set_high();
        let _ = SpiBus::write(&mut self.spi, data);
        self.cs.set_high();
    }

    async fn wait_busy(&mut self) {
        Timer::after(Duration::from_millis(1)).await;
        while self.busy.is_high() {
            Timer::after(Duration::from_millis(10)).await;
        }
    }
}

// --- Rendering ---

/// Render a simple boot/status message
pub async fn render_boot(epd: &mut Epd<'_>, msg: &str) {
    let mut fb = DisplayBuffer::new();
    let style = MonoTextStyle::new(&PROFONT_12_POINT, BinaryColor::On);
    let _ = Text::new(msg, Point::new(8, 64), style).draw(&mut fb);
    epd.update(fb.buffer()).await;
}

/// Render weather display into buffer (does NOT send to display)
///
/// Layout (296x128):
/// ┌──────────────────┬────────────────────────┐
/// │ 2026.02.13 (금)  │  ☀        8°C          │
/// │                  │       맑음              │
/// │ 02:00            │                         │
/// │                  │  습도 45%  →2.5m/s      │
/// │ ⚙ Seoul          │                         │
/// └──────────────────┴────────────────────────┘
pub fn render_to_buffer(
    fb: &mut DisplayBuffer,
    time: Option<&DateTime>,
    weather: Option<&WeatherData>,
    settings: &Settings,
) {
    fb.clear();

    let style_12 = MonoTextStyle::new(&PROFONT_12_POINT, BinaryColor::On);
    let style_24 = MonoTextStyle::new(&PROFONT_24_POINT, BinaryColor::On);
    let style_10 = MonoTextStyle::new(&PROFONT_10_POINT, BinaryColor::On);

    // ═══ Left Panel (x: 0~148, center=74) ═══
    // ProFont char widths: 24pt=14, 12pt=7, 10pt=6

    // Date line — centered
    if let Some(t) = time {
        let mut s: String<32> = String::new();
        if settings.language == 0 {
            let _ = core::write!(
                s, "{:04}.{:02}.{:02} ({})",
                t.year, t.month, t.day,
                korean_font::weekday_korean(t.weekday)
            );
            // ~12 ASCII chars × 7 + 1 Korean 16px + ")" 7px ≈ 107px → x=(148-107)/2=20
            korean_font::draw_korean_text(fb, s.as_str(), Point::new(20, 2), &PROFONT_12_POINT);
        } else {
            let _ = core::write!(
                s, "{:04}.{:02}.{:02} {}",
                t.year, t.month, t.day, t.weekday_str()
            );
            // 14 chars × 7 = 98px → x=(148-98)/2=25
            let _ = Text::new(s.as_str(), Point::new(25, 14), style_12).draw(fb);
        }
    } else {
        let _ = Text::new("----.--.-- (--)", Point::new(8, 14), style_12).draw(fb);
    }

    // Time — large font, centered
    // "HH:MM" = 5 chars × 14 = 70px → x=(148-70)/2=39
    if let Some(t) = time {
        let mut s: String<16> = String::new();
        if settings.time_format == 0 {
            let _ = core::write!(s, "{:02}:{:02}", t.hour, t.minute);
            let _ = Text::new(s.as_str(), Point::new(39, 78), style_24).draw(fb);
        } else {
            let h12 = if t.hour == 0 { 12 } else if t.hour > 12 { t.hour - 12 } else { t.hour };
            let ampm = if t.hour < 12 { "AM" } else { "PM" };
            let _ = core::write!(s, "{:02}:{:02}", h12, t.minute);
            // "HH:MM AM" ≈ 70 + 4 + 12 = 86px → x=(148-86)/2=31
            let _ = Text::new(s.as_str(), Point::new(31, 78), style_24).draw(fb);
            let _ = Text::new(ampm, Point::new(112, 78), style_10).draw(fb);
        }
    } else {
        let _ = Text::new("--:--", Point::new(39, 78), style_24).draw(fb);
    }

    // City name — centered at bottom
    let city_name = settings.city().name;
    let city_w = city_name.len() as i32 * 7;
    let city_x = (148 - city_w) / 2;
    let _ = Text::new(city_name, Point::new(city_x, 118), style_12).draw(fb);

    // ═══ Divider Line ═══
    let line_style = PrimitiveStyle::with_stroke(BinaryColor::On, 1);
    let _ = Line::new(Point::new(148, 8), Point::new(148, 120))
        .into_styled(line_style)
        .draw(fb);

    // ═══ Right Panel (x: 152~296, width=144, center=224) ═══

    if let Some(w) = weather {
        // Weather icon (32x32) + Temperature — centered as a group
        let temp = if settings.temp_unit == 0 {
            w.temp_int
        } else {
            (w.temp_int as i32 * 9 / 5 + 32) as i16
        };
        let mut temp_s: String<8> = String::new();
        let _ = core::write!(temp_s, "{}", temp);
        let unit_ch = if settings.temp_unit == 0 { "C" } else { "F" };
        // icon 32 + gap 6 + temp chars×14 + degree 7 + unit 14
        let temp_chars = temp_s.len() as i32;
        let row_w = 32 + 6 + temp_chars * 14 + 7 + 14;
        let row_x = 152 + (144 - row_w) / 2;

        icons::draw_weather_icon(fb, Point::new(row_x, 16), w.icon_code.as_str());
        let temp_x = row_x + 38;
        let next = Text::new(temp_s.as_str(), Point::new(temp_x, 42), style_24)
            .draw(fb)
            .unwrap_or(Point::new(temp_x + temp_chars * 14, 42));
        icons::draw_degree_symbol(fb, Point::new(next.x + 1, 18));
        let _ = Text::new(unit_ch, Point::new(next.x + 8, 42), style_24).draw(fb);

        // Weather description — centered
        if settings.language == 0 {
            let kr_desc = korean_font::weather_to_korean(w.description.as_str());
            let desc_w = korean_font::measure_korean_text(kr_desc) as i32;
            let desc_x = 152 + (144 - desc_w) / 2;
            korean_font::draw_korean_text(fb, kr_desc, Point::new(desc_x, 58), &PROFONT_12_POINT);
        } else {
            let desc_w = w.description.len() as i32 * 7;
            let desc_x = 152 + (144 - desc_w) / 2;
            let _ = Text::new(w.description.as_str(), Point::new(desc_x, 71), style_12).draw(fb);
        }

        // Humidity + Wind — centered as a row
        let mut hum_s: String<16> = String::new();
        let mut wind_s: String<16> = String::new();
        let _ = core::write!(
            wind_s,
            "{}.{}m/s",
            w.wind_speed_10x / 10,
            w.wind_speed_10x % 10
        );

        if settings.language == 0 {
            let _ = core::write!(hum_s, "습도 {}%", w.humidity);
            // hum: ~2 Korean(32) + digits×6 + "%"×6 ≈ ~56px, gap 8, arrow 12, wind ~42px
            let bottom_w = 56 + 8 + 12 + (wind_s.len() as i32) * 6;
            let bottom_x = 152 + (144 - bottom_w) / 2;
            korean_font::draw_korean_text(fb, hum_s.as_str(), Point::new(bottom_x, 96), &PROFONT_10_POINT);
            icons::draw_wind_arrow(fb, Point::new(bottom_x + 64, 99));
            let _ = Text::new(wind_s.as_str(), Point::new(bottom_x + 78, 109), style_10).draw(fb);
        } else {
            let _ = core::write!(hum_s, "Hum {}%", w.humidity);
            let bottom_w = (hum_s.len() as i32) * 6 + 8 + 12 + (wind_s.len() as i32) * 6;
            let bottom_x = 152 + (144 - bottom_w) / 2;
            let _ = Text::new(hum_s.as_str(), Point::new(bottom_x, 109), style_10).draw(fb);
            let hum_end = bottom_x + (hum_s.len() as i32) * 6 + 8;
            icons::draw_wind_arrow(fb, Point::new(hum_end, 99));
            let _ = Text::new(wind_s.as_str(), Point::new(hum_end + 14, 109), style_10).draw(fb);
        }
    } else {
        let _ = Text::new("No data", Point::new(185, 65), style_12).draw(fb);
    }

    // Apply display mode inversion
    if settings.display_mode == 1 {
        fb.invert();
    }
}
