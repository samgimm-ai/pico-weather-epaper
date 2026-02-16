use core::fmt::Write as FmtWrite;

use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{Line, PrimitiveStyle, Rectangle};
use embedded_graphics::text::Text;
use heapless::String;
use profont::{PROFONT_10_POINT, PROFONT_12_POINT};

use crate::display::DisplayBuffer;
use crate::korean_font;
use crate::settings::{Settings, CITIES, INTERVALS};
use crate::touch::TouchPoint;

// Header
const HEADER_H: i32 = 22;
const BACK_X: i32 = 40;
const BACK_Y: i32 = 22;

// Main menu: large rows with scroll
const MENU_ROW_H: i32 = 26;
const MENU_VISIBLE: usize = 4;
const MENU_ITEMS: usize = 7;

// City list: same row height as main menu
const CITY_VISIBLE: usize = 4;

pub enum MenuScreen {
    Main { scroll: u8 },
    EditCity { scroll: u8 },
    EditTimezone,
}

pub enum MenuAction {
    None,
    Redraw,
    Exit,
}

pub struct Menu {
    pub settings: Settings,
    screen: MenuScreen,
}

impl Menu {
    pub fn new(settings: Settings) -> Self {
        Self {
            settings,
            screen: MenuScreen::Main { scroll: 0 },
        }
    }

    pub fn handle_touch(&mut self, point: TouchPoint) -> MenuAction {
        match &self.screen {
            MenuScreen::Main { .. } => self.handle_main_touch(point),
            MenuScreen::EditCity { .. } => self.handle_city_touch(point),
            MenuScreen::EditTimezone => self.handle_timezone_touch(point),
        }
    }

    fn handle_main_touch(&mut self, point: TouchPoint) -> MenuAction {
        let x = point.x as i32;
        let y = point.y as i32;

        // Back button (top-left)
        if x < BACK_X && y < BACK_Y {
            return MenuAction::Exit;
        }

        // Scroll up button (top-right, left side) — page-based
        if x >= 230 && x < 262 && y < BACK_Y {
            if let MenuScreen::Main { ref mut scroll } = self.screen {
                if *scroll > 0 {
                    *scroll = 0; // Jump to page 1
                }
            }
            return MenuAction::Redraw;
        }

        // Scroll down button (top-right, right side) — page-based
        if x >= 262 && y < BACK_Y {
            if let MenuScreen::Main { ref mut scroll } = self.screen {
                if *scroll == 0 {
                    *scroll = MENU_VISIBLE as u8; // Jump to page 2
                }
            }
            return MenuAction::Redraw;
        }

        // Determine which visible row was touched
        let row_in_view = (y - HEADER_H) / MENU_ROW_H;
        if row_in_view < 0 || y < HEADER_H {
            return MenuAction::None;
        }

        let scroll = if let MenuScreen::Main { scroll } = self.screen {
            scroll
        } else {
            0
        };
        let item = scroll as i32 + row_in_view;

        match item {
            0 => {
                self.settings.language = if self.settings.language == 0 { 1 } else { 0 };
                MenuAction::Redraw
            }
            1 => {
                self.screen = MenuScreen::EditTimezone;
                MenuAction::Redraw
            }
            2 => {
                self.screen = MenuScreen::EditCity { scroll: 0 };
                MenuAction::Redraw
            }
            3 => {
                self.settings.temp_unit = if self.settings.temp_unit == 0 { 1 } else { 0 };
                MenuAction::Redraw
            }
            4 => {
                self.settings.time_format = if self.settings.time_format == 0 { 1 } else { 0 };
                MenuAction::Redraw
            }
            5 => {
                self.settings.interval_index =
                    (self.settings.interval_index + 1) % INTERVALS.len() as u8;
                MenuAction::Redraw
            }
            6 => {
                self.settings.display_mode = if self.settings.display_mode == 0 { 1 } else { 0 };
                MenuAction::Redraw
            }
            _ => MenuAction::None,
        }
    }

    fn handle_city_touch(&mut self, point: TouchPoint) -> MenuAction {
        let x = point.x as i32;
        let y = point.y as i32;

        // Back button
        if x < BACK_X && y < BACK_Y {
            self.screen = MenuScreen::Main { scroll: 0 };
            return MenuAction::Redraw;
        }

        // Scroll up button (top-right, left side) — page-based
        if x >= 230 && x < 262 && y < BACK_Y {
            if let MenuScreen::EditCity { ref mut scroll } = self.screen {
                if *scroll >= CITY_VISIBLE as u8 {
                    *scroll -= CITY_VISIBLE as u8;
                }
            }
            return MenuAction::Redraw;
        }

        // Scroll down button (top-right, right side) — page-based
        if x >= 262 && y < BACK_Y {
            if let MenuScreen::EditCity { ref mut scroll } = self.screen {
                if (*scroll as usize) + CITY_VISIBLE < CITIES.len() {
                    *scroll += CITY_VISIBLE as u8;
                }
            }
            return MenuAction::Redraw;
        }

        // City selection (4 visible rows)
        let row = (y - HEADER_H) / MENU_ROW_H;
        if row >= 0 && (row as usize) < CITY_VISIBLE && y >= HEADER_H {
            if let MenuScreen::EditCity { scroll } = self.screen {
                let idx = scroll as usize + row as usize;
                if idx < CITIES.len() {
                    self.settings.city_index = idx as u8;
                    self.screen = MenuScreen::Main { scroll: 0 };
                    return MenuAction::Redraw;
                }
            }
        }

        MenuAction::None
    }

    fn handle_timezone_touch(&mut self, point: TouchPoint) -> MenuAction {
        let x = point.x as i32;
        let y = point.y as i32;

        // Back button
        if x < BACK_X && y < BACK_Y {
            self.screen = MenuScreen::Main { scroll: 0 };
            return MenuAction::Redraw;
        }

        // "-" button (left half, y=75-125)
        if x <= 148 && y >= 75 && y <= 125 {
            if self.settings.utc_offset > -12 {
                self.settings.utc_offset -= 1;
            }
            return MenuAction::Redraw;
        }

        // "+" button (right half, y=75-125)
        if x > 148 && y >= 75 && y <= 125 {
            if self.settings.utc_offset < 14 {
                self.settings.utc_offset += 1;
            }
            return MenuAction::Redraw;
        }

        MenuAction::None
    }

    // ─── Rendering ───

    pub fn render(&self, fb: &mut DisplayBuffer) {
        fb.clear();
        match &self.screen {
            MenuScreen::Main { scroll } => self.render_main(fb, *scroll),
            MenuScreen::EditCity { scroll } => self.render_city(fb, *scroll),
            MenuScreen::EditTimezone => self.render_timezone(fb),
        }
    }

    fn render_header(&self, fb: &mut DisplayBuffer, kr_title: &str, en_title: &str) {
        let style_12 = MonoTextStyle::new(&PROFONT_12_POINT, BinaryColor::On);
        let line_style = PrimitiveStyle::with_stroke(BinaryColor::On, 1);
        let btn_style = PrimitiveStyle::with_stroke(BinaryColor::On, 1);

        // Back button [<]
        let _ = Rectangle::new(Point::new(0, 0), Size::new(20, 20))
            .into_styled(btn_style)
            .draw(fb);
        let _ = Text::new("<", Point::new(5, 15), style_12).draw(fb);

        // Title
        if self.settings.language == 0 {
            korean_font::draw_korean_text(fb, kr_title, Point::new(24, 3), &PROFONT_12_POINT);
        } else {
            let _ = Text::new(en_title, Point::new(24, 15), style_12).draw(fb);
        }

        // Divider
        let _ = Line::new(Point::new(0, HEADER_H - 1), Point::new(295, HEADER_H - 1))
            .into_styled(line_style)
            .draw(fb);
    }

    fn render_main(&self, fb: &mut DisplayBuffer, scroll: u8) {
        let style_12 = MonoTextStyle::new(&PROFONT_12_POINT, BinaryColor::On);
        let line_style = PrimitiveStyle::with_stroke(BinaryColor::On, 1);
        let btn_style = PrimitiveStyle::with_stroke(BinaryColor::On, 1);

        self.render_header(fb, "설정", "Settings");

        // Scroll up [^] button (top-right, left)
        let _ = Rectangle::new(Point::new(230, 0), Size::new(32, 20))
            .into_styled(btn_style)
            .draw(fb);
        let _ = Text::new("^", Point::new(242, 15), style_12).draw(fb);

        // Scroll down [v] button (top-right, right)
        let _ = Rectangle::new(Point::new(264, 0), Size::new(32, 20))
            .into_styled(btn_style)
            .draw(fb);
        let _ = Text::new("v", Point::new(276, 15), style_12).draw(fb);

        // Menu items (larger font, 26px rows)
        let items: [(&str, &str); 7] = [
            ("Language", "언어"),
            ("Timezone", "시간대"),
            ("City", "도시"),
            ("Temp Unit", "온도"),
            ("Time Fmt", "시간"),
            ("Interval", "주기"),
            ("Display", "반전"),
        ];

        for vi in 0..MENU_VISIBLE {
            let item_idx = scroll as usize + vi;
            if item_idx >= MENU_ITEMS {
                break;
            }

            let y = HEADER_H + (vi as i32) * MENU_ROW_H;
            let text_y = y + 19; // baseline for 12pt in 26px row

            let (en_label, kr_label) = items[item_idx];

            // Label (left)
            if self.settings.language == 0 {
                korean_font::draw_korean_text(
                    fb,
                    kr_label,
                    Point::new(8, y + 5),
                    &PROFONT_12_POINT,
                );
            } else {
                let _ = Text::new(en_label, Point::new(8, text_y), style_12).draw(fb);
            }

            // Value (right)
            let val_str = self.item_value_str(item_idx);
            let _ = Text::new(val_str.as_str(), Point::new(180, text_y), style_12).draw(fb);

            // Underline
            let line_y = y + MENU_ROW_H - 1;
            let _ = Line::new(Point::new(0, line_y), Point::new(295, line_y))
                .into_styled(line_style)
                .draw(fb);
        }

        // Page indicator text
        let mut pos: String<8> = String::new();
        let page = if scroll >= MENU_VISIBLE as u8 { 2 } else { 1 };
        let _ = core::write!(pos, "{}/2", page);
        let style_10 = MonoTextStyle::new(&PROFONT_10_POINT, BinaryColor::On);
        let _ = Text::new(pos.as_str(), Point::new(264, 118), style_10).draw(fb);
    }

    /// Get the display value for a settings item by index
    fn item_value_str(&self, idx: usize) -> String<16> {
        let mut s: String<16> = String::new();
        match idx {
            0 => { let _ = s.push_str(self.language_val()); }
            1 => {
                let off = self.settings.utc_offset;
                if off >= 0 {
                    let _ = core::write!(s, "UTC+{}", off);
                } else {
                    let _ = core::write!(s, "UTC{}", off);
                }
            }
            2 => { let _ = s.push_str(self.settings.city().name); }
            3 => { let _ = s.push_str(self.temp_unit_val()); }
            4 => { let _ = s.push_str(self.time_format_val()); }
            5 => { let _ = s.push_str(self.interval_val()); }
            6 => { let _ = s.push_str(self.display_mode_val()); }
            _ => {}
        }
        s
    }

    fn render_city(&self, fb: &mut DisplayBuffer, scroll: u8) {
        let style_12 = MonoTextStyle::new(&PROFONT_12_POINT, BinaryColor::On);
        let style_10 = MonoTextStyle::new(&PROFONT_10_POINT, BinaryColor::On);
        let line_style = PrimitiveStyle::with_stroke(BinaryColor::On, 1);
        let btn_style = PrimitiveStyle::with_stroke(BinaryColor::On, 1);

        self.render_header(fb, "도시", "City");

        // Scroll up [^] button (top-right, left)
        let _ = Rectangle::new(Point::new(230, 0), Size::new(32, 20))
            .into_styled(btn_style)
            .draw(fb);
        let _ = Text::new("^", Point::new(242, 15), style_12).draw(fb);

        // Scroll down [v] button (top-right, right)
        let _ = Rectangle::new(Point::new(264, 0), Size::new(32, 20))
            .into_styled(btn_style)
            .draw(fb);
        let _ = Text::new("v", Point::new(276, 15), style_12).draw(fb);

        // City list (4 visible rows, 26px each, 12pt font)
        for i in 0..CITY_VISIBLE {
            let idx = scroll as usize + i;
            if idx >= CITIES.len() {
                break;
            }

            let y = HEADER_H + (i as i32) * MENU_ROW_H;
            let text_y = y + 19;
            let selected = idx == self.settings.city_index as usize;
            let marker = if selected { "> " } else { "  " };

            let mut s: String<32> = String::new();
            let _ = core::write!(s, "{}{}", marker, CITIES[idx].name);
            let _ = Text::new(s.as_str(), Point::new(16, text_y), style_12).draw(fb);

            // Underline
            let line_y = y + MENU_ROW_H - 1;
            let _ = Line::new(Point::new(0, line_y), Point::new(295, line_y))
                .into_styled(line_style)
                .draw(fb);
        }

        // Page indicator
        let total_pages = (CITIES.len() + CITY_VISIBLE - 1) / CITY_VISIBLE;
        let current_page = (scroll as usize) / CITY_VISIBLE + 1;
        let mut pos: String<8> = String::new();
        let _ = core::write!(pos, "{}/{}", current_page, total_pages);
        let _ = Text::new(pos.as_str(), Point::new(264, 118), style_10).draw(fb);
    }

    fn render_timezone(&self, fb: &mut DisplayBuffer) {
        let style_24 = MonoTextStyle::new(&profont::PROFONT_24_POINT, BinaryColor::On);

        self.render_header(fb, "시간대", "Timezone");

        // Current value — large centered
        let mut s: String<16> = String::new();
        let off = self.settings.utc_offset;
        if off >= 0 {
            let _ = core::write!(s, "UTC+{}", off);
        } else {
            let _ = core::write!(s, "UTC{}", off);
        }
        let _ = Text::new(s.as_str(), Point::new(90, 60), style_24).draw(fb);

        // "-" button (left, larger)
        let btn_style = PrimitiveStyle::with_stroke(BinaryColor::On, 2);
        let _ = Rectangle::new(Point::new(10, 75), Size::new(120, 45))
            .into_styled(btn_style)
            .draw(fb);
        let _ = Text::new("-", Point::new(60, 105), style_24).draw(fb);

        // "+" button (right, larger)
        let _ = Rectangle::new(Point::new(166, 75), Size::new(120, 45))
            .into_styled(btn_style)
            .draw(fb);
        let _ = Text::new("+", Point::new(214, 105), style_24).draw(fb);
    }

    // ─── Value helpers ───

    fn language_val(&self) -> &'static str {
        match self.settings.language {
            0 => "Korean",
            _ => "English",
        }
    }

    fn temp_unit_val(&self) -> &'static str {
        match self.settings.temp_unit {
            0 => "C",
            _ => "F",
        }
    }

    fn time_format_val(&self) -> &'static str {
        match self.settings.time_format {
            0 => "24h",
            _ => "12h",
        }
    }

    fn display_mode_val(&self) -> &'static str {
        match self.settings.display_mode {
            0 => "Normal",
            _ => "Inverted",
        }
    }

    fn interval_val(&self) -> &'static str {
        let idx = (self.settings.interval_index as usize).min(INTERVALS.len() - 1);
        match INTERVALS[idx] {
            3600 => "1hr",
            7200 => "2hr",
            10800 => "3hr",
            21600 => "6hr",
            43200 => "12hr",
            _ => "??",
        }
    }
}
