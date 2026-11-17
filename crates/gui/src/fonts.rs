//! The GUI's fonts: Go Mono for the text and Go Regular for the interface.
//!
//! Bigelow & Holmes made them for the Go project, which publishes them under
//! Go's BSD-3-Clause license (`assets/fonts/LICENSE`, `assets/fonts/README`;
//! from `golang.org/x/image/font/gofont/ttfs`). They replace egui's default
//! fonts, whose OFL-1.1 and Ubuntu-font-1.0 licenses `deny.toml` does not
//! allow. They cover Latin, Greek and Cyrillic; there is no CJK or emoji font.

use eframe::egui::{Context, FontData, FontDefinitions, FontFamily};
use std::sync::Arc;

const GO_MONO: &[u8] = include_bytes!("../assets/fonts/Go-Mono.ttf");
const GO_REGULAR: &[u8] = include_bytes!("../assets/fonts/Go-Regular.ttf");

/// The font set: each family falls back to the other font.
#[must_use]
pub fn definitions() -> FontDefinitions {
    let mut fonts = FontDefinitions::empty();
    fonts.font_data.insert(
        "go-mono".to_owned(),
        Arc::new(FontData::from_static(GO_MONO)),
    );
    fonts.font_data.insert(
        "go-regular".to_owned(),
        Arc::new(FontData::from_static(GO_REGULAR)),
    );
    fonts.families.insert(
        FontFamily::Monospace,
        vec!["go-mono".to_owned(), "go-regular".to_owned()],
    );
    fonts.families.insert(
        FontFamily::Proportional,
        vec!["go-regular".to_owned(), "go-mono".to_owned()],
    );
    fonts
}

/// Installs the fonts in `ctx`; call it when the window is created, before
/// the first frame draws text.
pub fn install(ctx: &Context) {
    ctx.set_fonts(definitions());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_families_use_the_go_fonts() {
        let fonts = definitions();
        assert_eq!(fonts.families[&FontFamily::Monospace][0], "go-mono");
        assert_eq!(fonts.families[&FontFamily::Proportional][0], "go-regular");
        assert_eq!(fonts.font_data.len(), 2);
    }
}
