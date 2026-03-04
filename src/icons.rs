use embedded_graphics::prelude::*;
use embedded_graphics::primitives::*;
use embedded_graphics::pixelcolor::BinaryColor;

const ON: BinaryColor = BinaryColor::On;

/// Draw weather icon at given position based on OpenWeatherMap icon code
pub fn draw_weather_icon<D: DrawTarget<Color = BinaryColor>>(
    display: &mut D,
    pos: Point,
    icon_code: &str,
) {
    let code = if icon_code.len() >= 2 {
        &icon_code[..2]
    } else {
        "03" // default: cloud
    };

    match code {
        "01" => draw_sun(display, pos),
        "02" => draw_partial_cloud(display, pos),
        "03" => draw_cloud(display, pos),
        "04" => draw_overcast(display, pos),
        "09" | "10" => draw_rain(display, pos),
        "11" => draw_thunder(display, pos),
        "13" => draw_snow(display, pos),
        "50" => draw_mist(display, pos),
        _ => draw_cloud(display, pos),
    }
}

fn draw_sun<D: DrawTarget<Color = BinaryColor>>(d: &mut D, p: Point) {
    let style = PrimitiveStyle::with_fill(ON);
    // Center circle (sun body)
    let _ = Circle::new(p + Point::new(8, 8), 16).into_styled(style).draw(d);
    // Rays (short lines extending from center)
    let ray = PrimitiveStyle::with_stroke(ON, 2);
    let cx = p.x + 16;
    let cy = p.y + 16;
    // Top
    let _ = Line::new(Point::new(cx, p.y), Point::new(cx, p.y + 4)).into_styled(ray).draw(d);
    // Bottom
    let _ = Line::new(Point::new(cx, p.y + 28), Point::new(cx, p.y + 32)).into_styled(ray).draw(d);
    // Left
    let _ = Line::new(Point::new(p.x, cy), Point::new(p.x + 4, cy)).into_styled(ray).draw(d);
    // Right
    let _ = Line::new(Point::new(p.x + 28, cy), Point::new(p.x + 32, cy)).into_styled(ray).draw(d);
    // Diagonals
    let _ = Line::new(Point::new(p.x + 3, p.y + 3), Point::new(p.x + 6, p.y + 6)).into_styled(ray).draw(d);
    let _ = Line::new(Point::new(p.x + 26, p.y + 3), Point::new(p.x + 23, p.y + 6)).into_styled(ray).draw(d);
    let _ = Line::new(Point::new(p.x + 3, p.y + 29), Point::new(p.x + 6, p.y + 26)).into_styled(ray).draw(d);
    let _ = Line::new(Point::new(p.x + 26, p.y + 29), Point::new(p.x + 23, p.y + 26)).into_styled(ray).draw(d);
}

fn draw_cloud_shape<D: DrawTarget<Color = BinaryColor>>(d: &mut D, p: Point) {
    let style = PrimitiveStyle::with_fill(ON);
    // Cloud = overlapping circles + rectangle base
    let _ = Circle::new(p + Point::new(4, 10), 14).into_styled(style).draw(d);
    let _ = Circle::new(p + Point::new(10, 4), 16).into_styled(style).draw(d);
    let _ = Circle::new(p + Point::new(18, 8), 14).into_styled(style).draw(d);
    let _ = Rectangle::new(p + Point::new(4, 16), Size::new(24, 8))
        .into_styled(style)
        .draw(d);
}

/// Draw outline-only cloud shape (stroke, no fill)
fn draw_cloud_outline<D: DrawTarget<Color = BinaryColor>>(d: &mut D, p: Point) {
    let style = PrimitiveStyle::with_stroke(ON, 2);
    let _ = Circle::new(p + Point::new(4, 10), 14).into_styled(style).draw(d);
    let _ = Circle::new(p + Point::new(10, 4), 16).into_styled(style).draw(d);
    let _ = Circle::new(p + Point::new(18, 8), 14).into_styled(style).draw(d);
    let _ = Line::new(p + Point::new(5, 23), p + Point::new(27, 23))
        .into_styled(style)
        .draw(d);
}

fn draw_cloud<D: DrawTarget<Color = BinaryColor>>(d: &mut D, p: Point) {
    draw_cloud_outline(d, p);
}

fn draw_overcast<D: DrawTarget<Color = BinaryColor>>(d: &mut D, p: Point) {
    // Small cloud behind (upper-right)
    draw_cloud_outline(d, p + Point::new(6, -2));
    // Main cloud in front (lower-left)
    draw_cloud_outline(d, p + Point::new(-2, 6));
}

fn draw_partial_cloud<D: DrawTarget<Color = BinaryColor>>(d: &mut D, p: Point) {
    // Small sun behind cloud
    let style = PrimitiveStyle::with_fill(ON);
    let _ = Circle::new(p + Point::new(16, 0), 14).into_styled(style).draw(d);
    // Cloud in front (lower-left)
    draw_cloud_shape(d, p + Point::new(-2, 4));
}

fn draw_rain<D: DrawTarget<Color = BinaryColor>>(d: &mut D, p: Point) {
    draw_cloud_shape(d, p);
    // Rain drops
    let style = PrimitiveStyle::with_stroke(ON, 1);
    let _ = Line::new(p + Point::new(8, 26), p + Point::new(6, 31)).into_styled(style).draw(d);
    let _ = Line::new(p + Point::new(15, 26), p + Point::new(13, 31)).into_styled(style).draw(d);
    let _ = Line::new(p + Point::new(22, 26), p + Point::new(20, 31)).into_styled(style).draw(d);
}

fn draw_thunder<D: DrawTarget<Color = BinaryColor>>(d: &mut D, p: Point) {
    draw_cloud_shape(d, p);
    // Lightning bolt
    let style = PrimitiveStyle::with_stroke(ON, 2);
    let _ = Line::new(p + Point::new(16, 24), p + Point::new(13, 28)).into_styled(style).draw(d);
    let _ = Line::new(p + Point::new(13, 28), p + Point::new(17, 28)).into_styled(style).draw(d);
    let _ = Line::new(p + Point::new(17, 28), p + Point::new(14, 32)).into_styled(style).draw(d);
}

fn draw_snow<D: DrawTarget<Color = BinaryColor>>(d: &mut D, p: Point) {
    draw_cloud_shape(d, p);
    // Snowflakes (small dots)
    let style = PrimitiveStyle::with_fill(ON);
    let _ = Circle::new(p + Point::new(7, 27), 3).into_styled(style).draw(d);
    let _ = Circle::new(p + Point::new(14, 29), 3).into_styled(style).draw(d);
    let _ = Circle::new(p + Point::new(21, 27), 3).into_styled(style).draw(d);
}

fn draw_mist<D: DrawTarget<Color = BinaryColor>>(d: &mut D, p: Point) {
    // Horizontal lines representing fog/mist
    let style = PrimitiveStyle::with_stroke(ON, 2);
    let _ = Line::new(p + Point::new(2, 8), p + Point::new(28, 8)).into_styled(style).draw(d);
    let _ = Line::new(p + Point::new(4, 14), p + Point::new(30, 14)).into_styled(style).draw(d);
    let _ = Line::new(p + Point::new(2, 20), p + Point::new(28, 20)).into_styled(style).draw(d);
    let _ = Line::new(p + Point::new(4, 26), p + Point::new(30, 26)).into_styled(style).draw(d);
}

/// Draw degree symbol (°) as a small 4px circle at the given position
pub fn draw_degree_symbol<D: DrawTarget<Color = BinaryColor>>(d: &mut D, p: Point) {
    let style = PrimitiveStyle::with_stroke(ON, 1);
    let _ = Circle::new(p, 5).into_styled(style).draw(d);
}

/// Draw a 16x16 gear (⚙) settings icon at the given position
pub fn draw_settings_icon<D: DrawTarget<Color = BinaryColor>>(d: &mut D, p: Point) {
    #[rustfmt::skip]
    const GEAR: [u8; 32] = [
        0x03, 0xC0,
        0x03, 0xC0,
        0x1F, 0xF8,
        0x3F, 0xFC,
        0x71, 0x8E,
        0xE1, 0x87,
        0xE1, 0x87,
        0xFF, 0xFF,
        0xFF, 0xFF,
        0xE1, 0x87,
        0xE1, 0x87,
        0x71, 0x8E,
        0x3F, 0xFC,
        0x1F, 0xF8,
        0x03, 0xC0,
        0x03, 0xC0,
    ];
    for row in 0..16u32 {
        let hi = GEAR[(row * 2) as usize];
        let lo = GEAR[(row * 2 + 1) as usize];
        for col in 0..8u32 {
            if hi & (0x80 >> col) != 0 {
                let _ = Pixel(p + Point::new(col as i32, row as i32), ON).draw(d);
            }
            if lo & (0x80 >> col) != 0 {
                let _ = Pixel(p + Point::new(8 + col as i32, row as i32), ON).draw(d);
            }
        }
    }
}

/// Draw a small right-pointing wind arrow at the given position
pub fn draw_wind_arrow<D: DrawTarget<Color = BinaryColor>>(d: &mut D, p: Point) {
    let style = PrimitiveStyle::with_stroke(ON, 1);
    // Horizontal shaft
    let _ = Line::new(p + Point::new(0, 4), p + Point::new(10, 4)).into_styled(style).draw(d);
    // Arrowhead
    let _ = Line::new(p + Point::new(7, 1), p + Point::new(10, 4)).into_styled(style).draw(d);
    let _ = Line::new(p + Point::new(7, 7), p + Point::new(10, 4)).into_styled(style).draw(d);
}
