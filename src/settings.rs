use defmt::*;
use embassy_rp::flash::{self, Flash};
use embassy_rp::peripherals::FLASH;

pub const FLASH_SIZE: usize = 2 * 1024 * 1024; // 2MB RP2040
const SETTINGS_OFFSET: u32 = (FLASH_SIZE as u32) - 4096; // 0x1FF000
const MAGIC: u32 = 0xE1D1_5E77;
const VERSION: u8 = 1;

pub struct CityEntry {
    pub name: &'static str,
    pub lat: &'static str,
    pub lon: &'static str,
    pub utc_offset: i8,
}

pub const CITIES: &[CityEntry] = &[
    CityEntry { name: "Seoul",       lat: "37.5665",  lon: "126.9780",  utc_offset: 9 },
    CityEntry { name: "Tokyo",       lat: "35.6762",  lon: "139.6503",  utc_offset: 9 },
    CityEntry { name: "New York",    lat: "40.7128",  lon: "-74.0060",  utc_offset: -5 },
    CityEntry { name: "London",      lat: "51.5074",  lon: "-0.1278",   utc_offset: 0 },
    CityEntry { name: "Paris",       lat: "48.8566",  lon: "2.3522",    utc_offset: 1 },
    CityEntry { name: "Sydney",      lat: "-33.8688", lon: "151.2093",  utc_offset: 11 },
    CityEntry { name: "Beijing",     lat: "39.9042",  lon: "116.4074",  utc_offset: 8 },
    CityEntry { name: "Singapore",   lat: "1.3521",   lon: "103.8198",  utc_offset: 8 },
    CityEntry { name: "Dubai",       lat: "25.2048",  lon: "55.2708",   utc_offset: 4 },
    CityEntry { name: "Los Angeles", lat: "34.0522",  lon: "-118.2437", utc_offset: -8 },
];

// Weather fetch intervals (seconds). Time always refreshes every 60s.
pub const INTERVALS: &[u32] = &[3600, 7200, 10800, 21600, 43200];

#[repr(C)]
#[derive(Clone)]
pub struct Settings {
    magic: u32,
    version: u8,
    pub language: u8,        // 0=Korean, 1=English
    pub utc_offset: i8,      // -12 ~ +14
    pub city_index: u8,      // CITIES array index
    pub temp_unit: u8,       // 0=Celsius, 1=Fahrenheit
    pub time_format: u8,     // 0=24h, 1=12h
    pub interval_index: u8,  // INTERVALS array index
    pub display_mode: u8,    // 0=Normal, 1=Inverted
    _pad: [u8; 24],          // Pad to 32 bytes
}

impl Settings {
    pub fn new_default() -> Self {
        Self {
            magic: MAGIC,
            version: VERSION,
            language: 0,
            utc_offset: 9,
            city_index: 0,
            temp_unit: 0,
            time_format: 0,
            interval_index: 0,
            display_mode: 0,
            _pad: [0xFF; 24],
        }
    }

    pub fn city(&self) -> &'static CityEntry {
        let idx = (self.city_index as usize).min(CITIES.len() - 1);
        &CITIES[idx]
    }

    pub fn interval_secs(&self) -> u64 {
        let idx = (self.interval_index as usize).min(INTERVALS.len() - 1);
        INTERVALS[idx] as u64
    }

    pub fn utc_offset_seconds(&self) -> i32 {
        self.utc_offset as i32 * 3600
    }

    fn as_bytes(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(
                self as *const Self as *const u8,
                core::mem::size_of::<Self>(),
            )
        }
    }

    fn from_bytes(bytes: &[u8; 32]) -> Self {
        unsafe { core::ptr::read_unaligned(bytes.as_ptr() as *const Self) }
    }
}

pub fn load(flash: &mut Flash<'_, FLASH, flash::Blocking, FLASH_SIZE>) -> Settings {
    let mut buf = [0u8; 32];
    match flash.blocking_read(SETTINGS_OFFSET, &mut buf) {
        Ok(()) => {
            let s = Settings::from_bytes(&buf);
            if s.magic == MAGIC && s.version == VERSION {
                info!("Settings loaded from flash");
                s
            } else {
                info!("Settings: invalid magic/version, using defaults");
                Settings::new_default()
            }
        }
        Err(_) => {
            warn!("Settings: flash read error, using defaults");
            Settings::new_default()
        }
    }
}

pub fn save(flash: &mut Flash<'_, FLASH, flash::Blocking, FLASH_SIZE>, settings: &Settings) {
    if flash
        .blocking_erase(SETTINGS_OFFSET, SETTINGS_OFFSET + 4096)
        .is_err()
    {
        error!("Settings: flash erase failed");
        return;
    }

    // Write as a 256-byte page (flash page-aligned)
    let mut page = [0xFF_u8; 256];
    page[..core::mem::size_of::<Settings>()].copy_from_slice(settings.as_bytes());

    if flash.blocking_write(SETTINGS_OFFSET, &page).is_err() {
        error!("Settings: flash write failed");
        return;
    }

    info!("Settings saved to flash");
}
