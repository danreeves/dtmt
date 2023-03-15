use colors_transform::Color as _;
use colors_transform::Rgb;
use druid::Color;

pub use gruvbox_dark::*;

macro_rules! make_color {
    ($name:ident, $r:literal, $g:literal, $b:literal, $a:literal) => {
        pub const $name: Color = Color::rgba8($r, $g, $b, $a);
    };
    ($name:ident, $r:literal, $g:literal, $b:literal) => {
        pub const $name: Color = Color::rgb8($r, $g, $b);
    };
    ($name:ident, $col:expr) => {
        pub const $name: Color = $col;
    };
}

make_color!(TOP_BAR_BACKGROUND_COLOR, COLOR_BG1);

#[allow(dead_code)]
pub mod gruvbox_dark {
    use druid::Color;

    make_color!(COLOR_BG0_H, 0x1d, 0x20, 0x21);
    make_color!(COLOR_BG0_S, 0x32, 0x20, 0x2f);
    make_color!(COLOR_BG0, 0x28, 0x28, 0x28);
    make_color!(COLOR_BG1, 0x3c, 0x38, 0x36);
    make_color!(COLOR_BG2, 0x50, 0x49, 0x45);
    make_color!(COLOR_BG3, 0x66, 0x5c, 0x54);
    make_color!(COLOR_BG4, 0x7c, 0x6f, 0x64);

    make_color!(COLOR_FG0, 0xfb, 0xf1, 0xc7);
    make_color!(COLOR_FG1, 0xeb, 0xdb, 0xb2);
    make_color!(COLOR_FG2, 0xd5, 0xc4, 0xa1);
    make_color!(COLOR_FG3, 0xbd, 0xae, 0x93);
    make_color!(COLOR_FG4, 0xa8, 0x99, 0x84);

    make_color!(COLOR_BG, COLOR_BG0);
    make_color!(COLOR_GRAY_LIGHT, 0x92, 0x83, 0x74);

    make_color!(COLOR_RED_DARK, 0xcc, 0x24, 0x1d);
    make_color!(COLOR_RED_LIGHT, 0xfb, 0x49, 0x34);

    make_color!(COLOR_GREEN_DARK, 0x98, 0x97, 0x1a);
    make_color!(COLOR_GREEN_LIGHT, 0xb8, 0xbb, 0x26);

    make_color!(COLOR_YELLOW_DARK, 0xd7, 0x99, 0x21);
    make_color!(COLOR_YELLOW_LIGHT, 0xfa, 0xbd, 0x2f);

    make_color!(COLOR_BLUE_DARK, 0x45, 0x85, 0x88);
    make_color!(COLOR_BLUE_LIGHT, 0x83, 0xa5, 0x98);

    make_color!(COLOR_PURPLE_DARK, 0xb1, 0x26, 0x86);
    make_color!(COLOR_PURPLE_LIGHT, 0xd3, 0x86, 0x9b);

    make_color!(COLOR_AQUA_DARK, 0x68, 0x9d, 0x6a);
    make_color!(COLOR_AQUA_LIGHT, 0x8e, 0xc0, 0x7c);

    make_color!(COLOR_GRAY_DARK, 0xa8, 0x99, 0x84);
    make_color!(COLOR_FG, COLOR_FG1);

    make_color!(COLOR_ORANGE_DARK, 0xd6, 0x5d, 0x0e);
    make_color!(COLOR_ORANGE_LIGHT, 0xfe, 0x80, 0x19);

    make_color!(COLOR_ACCENT, COLOR_BLUE_LIGHT);
    make_color!(COLOR_ACCENT_FG, COLOR_BG0_H);
}

pub trait ColorExt {
    fn lighten(&self, fac: f32) -> Self;
    fn darken(&self, fac: f32) -> Self;
}

impl ColorExt for Color {
    fn lighten(&self, fac: f32) -> Self {
        let (r, g, b, a) = self.as_rgba();
        let rgb = Rgb::from(r as f32, g as f32, b as f32);
        let rgb = rgb.lighten(fac);
        Self::rgba(
            rgb.get_red() as f64,
            rgb.get_green() as f64,
            rgb.get_blue() as f64,
            a,
        )
    }

    fn darken(&self, fac: f32) -> Self {
        let (r, g, b, a) = self.as_rgba();
        let rgb = Rgb::from(r as f32, g as f32, b as f32);
        let rgb = rgb.lighten(-1. * fac);
        Self::rgba(
            rgb.get_red() as f64,
            rgb.get_green() as f64,
            rgb.get_blue() as f64,
            a,
        )
    }
}
