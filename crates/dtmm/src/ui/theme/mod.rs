use druid::{Env, Insets};

use crate::state::State;

mod colors;
pub mod icons;
pub mod keys;

pub use colors::*;

pub const TOP_BAR_INSETS: Insets = Insets::uniform(5.0);
pub const DISABLED_ALPHA: f64 = 0.65;

pub(crate) fn set_theme_env(env: &mut Env, _: &State) {
    env.set(druid::theme::TEXT_COLOR, COLOR_FG);
    env.set(druid::theme::BUTTON_BORDER_RADIUS, 2.);
    env.set(keys::KEY_BUTTON_BG, COLOR_ACCENT);
    env.set(keys::KEY_BUTTON_BG_HOT, COLOR_ACCENT.darken(0.03));
    env.set(keys::KEY_BUTTON_BG_ACTIVE, COLOR_ACCENT.darken(0.1));
    env.set(
        keys::KEY_BUTTON_BG_DISABLED,
        COLOR_ACCENT.with_alpha(DISABLED_ALPHA),
    );
    env.set(keys::KEY_BUTTON_FG, COLOR_ACCENT_FG);
    env.set(
        keys::KEY_BUTTON_FG_DISABLED,
        COLOR_ACCENT_FG.with_alpha(DISABLED_ALPHA),
    );
    env.set(keys::KEY_BUTTON_PADDING, Insets::uniform_xy(8., 2.));
}
