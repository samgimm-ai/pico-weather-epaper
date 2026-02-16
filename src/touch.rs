use defmt::*;
use embassy_rp::gpio::{Input, Output};
use embassy_rp::i2c::{self, I2c};
use embassy_rp::peripherals::I2C1;
use embassy_time::{Duration, Timer};

const ICNT86_ADDR: u16 = 0x48;
const REG_VERSION: u16 = 0x000A;
const REG_TOUCH_NUM: u16 = 0x1001;
const REG_TOUCH_DATA: u16 = 0x1002;

pub struct TouchPoint {
    pub x: u16, // 0-295 (landscape width)
    pub y: u16, // 0-127 (landscape height)
}

pub struct Touch<'d> {
    i2c: I2c<'d, I2C1, i2c::Async>,
    int: Input<'d>,
    trst: Output<'d>,
}

impl<'d> Touch<'d> {
    pub fn new(
        i2c: I2c<'d, I2C1, i2c::Async>,
        int: Input<'d>,
        trst: Output<'d>,
    ) -> Self {
        Self { i2c, int, trst }
    }

    /// Initialize ICNT86 touch controller with TRST reset sequence + version check
    pub async fn init(&mut self) {
        // Hardware reset: HIGH → LOW → HIGH
        self.trst.set_high();
        Timer::after(Duration::from_millis(100)).await;
        self.trst.set_low();
        Timer::after(Duration::from_millis(100)).await;
        self.trst.set_high();
        Timer::after(Duration::from_millis(100)).await;

        // Wait for controller to boot
        Timer::after(Duration::from_millis(200)).await;

        // Read version to verify I2C communication
        let mut ver = [0u8; 4];
        match self.read_reg(REG_VERSION, &mut ver).await {
            Ok(()) => {
                info!(
                    "ICNT86: IC={:02x}{:02x} FW={:02x}{:02x}",
                    ver[0], ver[1], ver[2], ver[3]
                );
            }
            Err(e) => {
                error!("ICNT86: version read failed: {}", e);
            }
        }

        info!("ICNT86 touch controller initialized");
    }

    /// Read touch point if available (non-blocking check via INT pin)
    pub async fn read_touch(&mut self) -> Option<TouchPoint> {
        if self.int.is_high() {
            return None;
        }

        // Read touch count from register 0x1001
        let mut count_buf = [0u8; 1];
        if self.read_reg(REG_TOUCH_NUM, &mut count_buf).await.is_err() {
            return None;
        }

        let touch_count = count_buf[0] & 0x0F;
        if touch_count == 0 {
            let _ = self.write_reg(REG_TOUCH_NUM, &[0]).await;
            return None;
        }

        // Read first touch point (7 bytes per point)
        // Format: [pad, x_lo, x_hi, y_lo, y_hi, pressure, event_id]
        let mut data = [0u8; 7];
        if self.read_reg(REG_TOUCH_DATA, &mut data).await.is_err() {
            let _ = self.write_reg(REG_TOUCH_NUM, &[0]).await;
            return None;
        }

        // Clear touch count register
        let _ = self.write_reg(REG_TOUCH_NUM, &[0]).await;

        // Parse coordinates (little-endian)
        let raw_x = ((data[2] as u16) << 8) | data[1] as u16;
        let raw_y = ((data[4] as u16) << 8) | data[3] as u16;

        // ICNT86 reports landscape coordinates directly:
        // raw_x = 0-295 (long axis, left to right)
        // raw_y = 0-127 (short axis, top to bottom)
        info!("Touch: ({},{})", raw_x, raw_y);

        Some(TouchPoint {
            x: raw_x,
            y: raw_y,
        })
    }

    /// Wait for a touch event (blocks until INT falling edge)
    pub async fn wait_for_touch(&mut self) -> TouchPoint {
        loop {
            self.int.wait_for_falling_edge().await;
            Timer::after(Duration::from_millis(20)).await;
            if let Some(point) = self.read_touch().await {
                return point;
            }
        }
    }

    /// Read register using separate write + read transactions (no repeated start)
    async fn read_reg(&mut self, reg: u16, buf: &mut [u8]) -> Result<(), i2c::Error> {
        let reg_bytes = reg.to_be_bytes();
        // Write register address (2 bytes)
        self.i2c.write_async(ICNT86_ADDR, reg_bytes).await?;
        // Separate read transaction
        self.i2c.read_async(ICNT86_ADDR, buf).await
    }

    /// Write register: address (2 bytes) + data in single transaction
    async fn write_reg(&mut self, reg: u16, data: &[u8]) -> Result<(), i2c::Error> {
        let reg_bytes = reg.to_be_bytes();
        match data.len() {
            0 => self.i2c.write_async(ICNT86_ADDR, reg_bytes).await,
            1 => {
                self.i2c
                    .write_async(ICNT86_ADDR, [reg_bytes[0], reg_bytes[1], data[0]])
                    .await
            }
            _ => {
                let mut buf = [0u8; 10];
                buf[0] = reg_bytes[0];
                buf[1] = reg_bytes[1];
                let n = data.len().min(8);
                buf[2..2 + n].copy_from_slice(&data[..n]);
                self.i2c
                    .write_async(ICNT86_ADDR, buf[..2 + n].iter().copied())
                    .await
            }
        }
    }
}
