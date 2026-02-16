#![no_std]
#![no_main]

mod config;
mod display;
mod icons;
mod korean_font;
mod menu;
mod ntp;
mod settings;
mod touch;
mod weather;
mod wifi;

use cyw43_pio::PioSpi;
use defmt::*;
use embassy_executor::Spawner;
use embassy_futures::select::{select, Either};
use embassy_net::StackResources;
use embassy_rp::bind_interrupts;
use embassy_rp::flash::{self, Flash};
use embassy_rp::gpio::{Input, Level, Output, Pull};
use embassy_rp::i2c::{self, I2c};
use embassy_rp::peripherals::{DMA_CH0, I2C1, PIO0};
use embassy_rp::pio::{InterruptHandler as PioInterruptHandler, Pio};
use embassy_rp::spi;
use embassy_time::{Duration, Timer};
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};

use display::DisplayBuffer;
use menu::{Menu, MenuAction};

bind_interrupts!(struct Irqs {
    PIO0_IRQ_0 => PioInterruptHandler<PIO0>;
    I2C1_IRQ => i2c::InterruptHandler<I2C1>;
});

// ─── Background tasks ───

#[embassy_executor::task]
async fn cyw43_task(
    runner: cyw43::Runner<'static, Output<'static>, PioSpi<'static, PIO0, 0, DMA_CH0>>,
) -> ! {
    runner.run().await
}

#[embassy_executor::task]
async fn net_task(mut runner: embassy_net::Runner<'static, cyw43::NetDriver<'static>>) -> ! {
    runner.run().await
}

// City name touch area on main weather screen (bottom of left panel)
fn is_gear_touch(x: u16, y: u16) -> bool {
    x <= 148 && y >= 100
}

// ─── Main ───

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    info!("Booting weather display...");
    let p = embassy_rp::init(Default::default());

    // ── Flash + Settings ──
    let mut flash =
        Flash::<_, flash::Blocking, { settings::FLASH_SIZE }>::new_blocking(p.FLASH);
    let mut settings = settings::load(&mut flash);
    info!(
        "Settings: lang={} tz={} city={} temp={} fmt={} interval={} disp={}",
        settings.language,
        settings.utc_offset,
        settings.city_index,
        settings.temp_unit,
        settings.time_format,
        settings.interval_index,
        settings.display_mode,
    );

    // ── CYW43 WiFi chip initialization ──
    let fw = include_bytes!("../firmware/43439A0.bin");
    let clm = include_bytes!("../firmware/43439A0_clm.bin");

    let pwr = Output::new(p.PIN_23, Level::Low);
    let cs = Output::new(p.PIN_25, Level::High);
    let mut pio = Pio::new(p.PIO0, Irqs);
    let pio_spi = PioSpi::new(
        &mut pio.common,
        pio.sm0,
        cyw43_pio::DEFAULT_CLOCK_DIVIDER,
        pio.irq0,
        cs,
        p.PIN_24,
        p.PIN_29,
        p.DMA_CH0,
    );

    static STATE: StaticCell<cyw43::State> = StaticCell::new();
    let state = STATE.init(cyw43::State::new());
    let (net_device, mut control, runner) = cyw43::new(state, pwr, pio_spi, fw).await;
    unwrap!(spawner.spawn(cyw43_task(runner)));

    control.init(clm).await;
    control
        .set_power_management(cyw43::PowerManagementMode::PowerSave)
        .await;

    // ── Network stack ──
    let net_config = embassy_net::Config::dhcpv4(Default::default());
    let seed = 0x0a3b_c5d7_e9f1_2346_u64;

    static RESOURCES: StaticCell<StackResources<5>> = StaticCell::new();
    let (stack, net_runner) = embassy_net::new(
        net_device,
        net_config,
        RESOURCES.init(StackResources::new()),
        seed,
    );
    unwrap!(spawner.spawn(net_task(net_runner)));

    // ── e-Paper display initialization (after CYW43) ──
    let mut spi_config = spi::Config::default();
    spi_config.frequency = 4_000_000;

    let display_spi = spi::Spi::new_blocking_txonly(
        p.SPI1,
        p.PIN_10, // SCK
        p.PIN_11, // MOSI
        spi_config,
    );

    let display_cs = Output::new(p.PIN_9, Level::High);
    let display_dc = Output::new(p.PIN_8, Level::Low);
    let display_rst = Output::new(p.PIN_12, Level::Low);
    let display_busy = Input::new(p.PIN_13, Pull::Up);

    let mut epd = display::Epd::new(display_spi, display_cs, display_dc, display_rst, display_busy);
    epd.init().await;

    // ── Touch controller initialization ──
    let i2c_config = i2c::Config::default(); // 100kHz, pull-ups enabled
    let i2c1 = I2c::new_async(
        p.I2C1,
        p.PIN_7, // SCL
        p.PIN_6, // SDA
        Irqs,
        i2c_config,
    );
    let touch_int = Input::new(p.PIN_17, Pull::Up);
    let touch_rst = Output::new(p.PIN_16, Level::Low);
    let mut touch = touch::Touch::new(i2c1, touch_int, touch_rst);
    touch.init().await;

    // ── WiFi connect + DHCP ──
    display::render_boot(&mut epd, "Connecting WiFi...").await;
    wifi::connect(&mut control).await;

    info!("Waiting for DHCP...");
    loop {
        if stack.is_config_up() {
            break;
        }
        Timer::after(Duration::from_millis(100)).await;
    }
    if let Some(cfg) = stack.config_v4() {
        info!("IP: {}", cfg.address.address());
    }

    // ── Initial data fetch + display ──
    let mut fb = DisplayBuffer::new();
    let mut last_time = ntp::get_time(stack, settings.utc_offset_seconds()).await.ok();
    let city = settings.city();
    let mut last_weather = weather::get_weather(stack, city.lat, city.lon).await.ok();

    display::render_to_buffer(&mut fb, last_time.as_ref(), last_weather.as_ref(), &settings);
    epd.update(fb.buffer()).await;
    epd.init_partial().await; // prepare for partial refresh (don't sleep)

    // ── Main loop ──
    info!("Entering main loop");
    let mut minute_counter: u32 = 0;
    let mut in_menu = false;
    let mut menu = Menu::new(settings.clone());

    loop {
        if !in_menu {
            // ═══ Normal mode ═══
            // Time: partial refresh every 60s
            // Weather: full refresh every interval_secs (min 1hr)

            match select(
                Timer::after(Duration::from_secs(60)),
                touch.wait_for_touch(),
            )
            .await
            {
                Either::First(_) => {
                    // 60s tick — always fetch time
                    minute_counter += 1;
                    last_time =
                        ntp::get_time(stack, settings.utc_offset_seconds()).await.ok();

                    // Weather update? (interval_secs / 60 ticks)
                    let weather_ticks = (settings.interval_secs() / 60).max(1) as u32;
                    let full_update = minute_counter >= weather_ticks;

                    if full_update {
                        minute_counter = 0;
                        let city = settings.city();
                        last_weather =
                            weather::get_weather(stack, city.lat, city.lon).await.ok();

                        display::render_to_buffer(
                            &mut fb,
                            last_time.as_ref(),
                            last_weather.as_ref(),
                            &settings,
                        );
                        epd.init().await;
                        epd.update(fb.buffer()).await;
                        epd.init_partial().await; // ready for next partial
                    } else {
                        // Partial refresh — time changed
                        display::render_to_buffer(
                            &mut fb,
                            last_time.as_ref(),
                            last_weather.as_ref(),
                            &settings,
                        );
                        epd.update_partial(fb.buffer()).await;
                        // No sleep — stay in partial mode, RAMs must persist
                    }
                }
                Either::Second(point) => {
                    // Touch event — check gear area
                    if is_gear_touch(point.x, point.y) {
                        info!("Gear touched — entering menu");
                        in_menu = true;
                        menu = Menu::new(settings.clone());

                        menu.render(&mut fb);
                        epd.init().await;
                        epd.update(fb.buffer()).await;
                    }
                }
            }
        } else {
            // ═══ Menu mode: wait for touch, no timers, no weather fetch ═══
            let point = touch.wait_for_touch().await;

            match menu.handle_touch(point) {
                MenuAction::None => {}
                MenuAction::Redraw => {
                    menu.render(&mut fb);
                    epd.update(fb.buffer()).await;
                }
                MenuAction::Exit => {
                    info!("Menu exit — saving settings");
                    settings = menu.settings.clone();
                    settings::save(&mut flash, &settings);

                    // Fetch fresh data with new settings
                    last_time =
                        ntp::get_time(stack, settings.utc_offset_seconds()).await.ok();
                    let city = settings.city();
                    last_weather =
                        weather::get_weather(stack, city.lat, city.lon).await.ok();
                    minute_counter = 0;

                    // Full refresh with new data
                    display::render_to_buffer(
                        &mut fb,
                        last_time.as_ref(),
                        last_weather.as_ref(),
                        &settings,
                    );
                    epd.init().await;
                    epd.update(fb.buffer()).await;
                    epd.init_partial().await; // ready for partial

                    in_menu = false;
                }
            }
        }
    }
}
