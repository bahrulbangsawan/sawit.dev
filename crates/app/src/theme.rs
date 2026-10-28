//! Zed's "One Dark" palette as a gpui-component theme.

use gpui_kit::App;
use gpui_kit::component::{Theme, ThemeRegistry};

const ZED_ONE: &str = include_str!("../assets/themes/zed-one-dark.json");
const NAME: &str = "Zed One Dark";

pub fn apply_zed_one_dark(cx: &mut App) {
    if let Err(e) = ThemeRegistry::global_mut(cx).load_themes_from_str(ZED_ONE) {
        eprintln!("theme: {e:#}");
        return;
    }
    let Some(config) = ThemeRegistry::global(cx).themes().get(NAME).cloned() else {
        eprintln!("theme: {NAME} missing after load");
        return;
    };
    Theme::update(cx, |theme| theme.apply_config(&config));
}
