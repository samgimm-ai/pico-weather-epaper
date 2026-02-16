# Pico Weather e-Paper

Raspberry Pi Pico W + Waveshare 2.9" Touch e-Paper weather display.

NTP time and OpenWeatherMap data displayed on an e-Paper screen with a touch-based settings menu. Built with Embassy async Rust (`no_std`).

## Hardware

- **MCU**: Raspberry Pi Pico W (RP2040 + CYW43 WiFi)
- **Display**: Waveshare Pico-CapTouch-ePaper-2.9 (SSD1680, 296x128)
- **Touch**: ICNT86 capacitive touch controller (I2C)

### Pin Map

| Function | Pin |
|----------|-----|
| SPI1 SCK | GP10 |
| SPI1 MOSI | GP11 |
| e-Paper CS | GP9 |
| e-Paper DC | GP8 |
| e-Paper RST | GP12 |
| e-Paper BUSY | GP13 |
| I2C1 SDA | GP6 |
| I2C1 SCL | GP7 |
| Touch INT | GP17 |
| Touch RST | GP16 |

## Features

- Real-time clock via NTP (partial refresh every 60s)
- Weather data from OpenWeatherMap API (configurable interval: 1-12hr)
- Korean / English bilingual UI with custom 16x16 bitmap font
- Touch settings menu: language, timezone (UTC-12 to +14), city (10 presets), temperature unit, time format, update interval, display inversion
- Settings persisted to Flash (last 4KB sector)
- SSD1680 partial refresh for fast time updates without full screen flicker

## Build

### Prerequisites

- Rust nightly with `thumbv6m-none-eabi` target
- `elf2uf2-rs` for UF2 conversion

```bash
rustup target add thumbv6m-none-eabi
cargo install elf2uf2-rs
```

### Firmware

Download CYW43 WiFi firmware and place in `firmware/`:

- `43439A0.bin`
- `43439A0_clm.bin`

These can be obtained from the [embassy-rs cyw43 firmware releases](https://github.com/embassy-rs/embassy/tree/main/cyw43-firmware).

### Configuration

Copy the example config and fill in your credentials:

```bash
cp src/config.rs.example src/config.rs
```

Edit `src/config.rs` with your WiFi SSID, password, and OpenWeatherMap API key.

### Build & Flash

```bash
cargo build --release
elf2uf2-rs convert --family rp2040 target/thumbv6m-none-eabi/release/rasp-pico-w-e-paper-2-9 firmware.uf2
```

Hold BOOTSEL on the Pico W, connect USB, then copy:

```bash
cp firmware.uf2 /Volumes/RPI-RP2/
```

## Project Structure

```
src/
  main.rs          - Entry point, main loop (timer + touch event handling)
  display.rs       - SSD1680 driver, framebuffer, full/partial refresh, weather layout
  touch.rs         - ICNT86 capacitive touch driver (I2C)
  menu.rs          - Touch settings menu (state machine + rendering)
  settings.rs      - Settings struct, city presets, Flash load/save
  weather.rs       - OpenWeatherMap API client (HTTP over TCP)
  ntp.rs           - NTP time client (UDP)
  wifi.rs          - CYW43 WiFi connection
  korean_font.rs   - 16x16 Korean bitmap font (42 glyphs)
  icons.rs         - Weather icons, degree symbol, wind arrow
  config.rs        - WiFi credentials & API key (not committed)
scripts/
  gen_korean_font.py - Korean bitmap font generator (Apple SD Gothic Neo)
```

## License

MIT
