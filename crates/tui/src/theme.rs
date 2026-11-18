//! Dark Modern for the terminal, in whatever color the terminal has.
//!
//! Colors come from VS Code's Dark Modern theme
//! (`extensions/theme-defaults/themes/dark_modern.json`) and, for roles it
//! leaves to the defaults, from the color registry: selection `#264F78`
//! (`src/vs/platform/theme/common/colors/editorColors.ts:121-123`), list
//! selection `#04395E` and match highlight `#2AAAFF`
//! (`src/vs/platform/theme/common/colors/listColors.ts:33-36`, `:81-84`),
//! cursor `#AEAFAD` (`src/vs/editor/common/core/editorColorRegistry.ts:22`),
//! error, warning, info and hint squiggles `#F14C4C`, `#CCA700`, `#59A4F9`
//! and `#EEEEEE` at 70% over the background (`src/vs/platform/theme/common/colors/editorColors.ts:70-107`).
//!
//! Diagnostics are underlined in their color, where the terminal colors
//! underlines; under `NO_COLOR` they are underlined (errors also bold), and
//! the gutter and the problems panel name the severity with a letter: E, W
//! or I.
//!
//! The mode follows the terminal: `NO_COLOR` (any value, see no-color.org)
//! turns colors off; `COLORTERM=truecolor` or `24bit` gives 24-bit color;
//! a `TERM` with `256color` gives the xterm 256-color palette; anything else
//! gets 16 colors. `RUSTODE_COLOR` (`truecolor`, `256`, `16`, `none`)
//! overrides the guess. Meaning never rests on color alone: the active tab,
//! the selected row and matches are also bold or reversed.
//!
//! Syntax colors are Dark Modern's token colors, which the workspace gives
//! for each token kind (`TokenKind::dark_modern`); with 16 colors each kind
//! takes the nearest basic color, and with `NO_COLOR` the text is plain.
//! They only decorate the text, which reads the same without them. Selected
//! text keeps its token color on the selection background, as in VS Code,
//! except with 16 colors, where a basic blue token would vanish on the blue
//! selection.

use ratatui::style::{Color, Modifier, Style};
use rustode_workspace::MarkerSeverity;
use rustode_workspace::TokenKind;

/// How many colors the terminal shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    /// 24-bit color.
    TrueColor,
    /// The xterm 256-color palette.
    Ansi256,
    /// The 16 basic colors.
    Ansi16,
    /// No colors (`NO_COLOR`): only bold, dim and reversed.
    None,
}

impl ColorMode {
    /// The mode for this environment, as described in the module docs.
    #[must_use]
    pub fn detect(env: impl Fn(&str) -> Option<String>) -> Self {
        let chosen = env("RUSTODE_COLOR").map(|v| v.to_ascii_lowercase());
        match chosen.as_deref() {
            Some("truecolor" | "24bit") => return Self::TrueColor,
            Some("256") => return Self::Ansi256,
            Some("16") => return Self::Ansi16,
            Some("none") => return Self::None,
            _ => {}
        }
        if env("NO_COLOR").is_some_and(|v| !v.is_empty()) {
            return Self::None;
        }
        if env("COLORTERM").is_some_and(|v| matches!(v.as_str(), "truecolor" | "24bit")) {
            return Self::TrueColor;
        }
        if env("TERM").is_some_and(|v| v.contains("256color")) {
            return Self::Ansi256;
        }
        Self::Ansi16
    }
}

/// The styles of every part of the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Theme {
    mode: ColorMode,
    pub editor: Style,
    pub gutter: Style,
    pub gutter_active: Style,
    pub selection: Style,
    pub secondary_cursor: Style,
    pub tab_bar: Style,
    pub tab_active: Style,
    pub tab_inactive: Style,
    pub separator: Style,
    pub sidebar: Style,
    pub sidebar_header: Style,
    pub explorer_selected: Style,
    pub explorer_active: Style,
    pub status: Style,
    pub quick: Style,
    pub quick_border: Style,
    pub quick_selected: Style,
    pub highlight: Style,
    pub description: Style,
    /// Underlines for diagnostics, by severity: error, warning, info, hint.
    pub markers: [Style; 4],
    /// The severity letters (E, W, I) of the gutter and the problems panel.
    pub letters: [Style; 3],
}

/// A palette color, as the mode can show it; `basic` stands in for 16 colors.
fn color(mode: ColorMode, rgb: u32, basic: Color) -> Color {
    match mode {
        ColorMode::TrueColor => {
            let [_, r, g, b] = rgb.to_be_bytes();
            Color::Rgb(r, g, b)
        }
        ColorMode::Ansi256 => Color::Indexed(nearest_256(rgb)),
        ColorMode::Ansi16 => basic,
        ColorMode::None => Color::Reset,
    }
}

impl Theme {
    pub(crate) fn new(mode: ColorMode) -> Self {
        let c = |rgb: u32, basic: Color| color(mode, rgb, basic);
        let plain = |fg: Color, bg: Color| Style::new().fg(fg).bg(bg);
        let none = mode == ColorMode::None;
        let sixteen = mode == ColorMode::Ansi16;
        let reversed = Style::new().add_modifier(Modifier::REVERSED);
        let bold = Style::new().add_modifier(Modifier::BOLD);
        let editor = plain(c(0xCC_CC_CC, Color::Reset), c(0x1F_1F_1F, Color::Reset));
        let dark = plain(c(0xCC_CC_CC, Color::Reset), c(0x18_18_18, Color::Reset));
        let pick = |colored: Style, fallback: Style| if none { fallback } else { colored };
        Self {
            mode,
            editor,
            gutter: pick(
                Style::new().fg(c(0x6E_76_81, Color::DarkGray)),
                Style::new().add_modifier(Modifier::DIM),
            ),
            gutter_active: Style::new()
                .fg(c(0xCC_CC_CC, Color::Reset))
                .add_modifier(Modifier::BOLD),
            selection: pick(Style::new().bg(c(0x26_4F_78, Color::Blue)), reversed),
            secondary_cursor: pick(
                Style::new()
                    .bg(c(0xAE_AF_AD, Color::Gray))
                    .fg(c(0x1F_1F_1F, Color::Black)),
                reversed,
            ),
            tab_bar: dark,
            tab_active: if none || sixteen {
                reversed.add_modifier(Modifier::BOLD)
            } else {
                plain(c(0xFF_FF_FF, Color::White), c(0x1F_1F_1F, Color::Reset))
                    .add_modifier(Modifier::BOLD)
            },
            tab_inactive: plain(c(0x9D_9D_9D, Color::Gray), c(0x18_18_18, Color::Reset)),
            separator: plain(c(0x2B_2B_2B, Color::DarkGray), c(0x18_18_18, Color::Reset)),
            sidebar: dark,
            sidebar_header: dark.add_modifier(Modifier::BOLD),
            explorer_selected: pick(
                plain(c(0xFF_FF_FF, Color::White), c(0x04_39_5E, Color::Blue))
                    .add_modifier(Modifier::BOLD),
                reversed,
            ),
            explorer_active: bold,
            status: if none || sixteen {
                reversed
            } else {
                plain(c(0xCC_CC_CC, Color::Reset), c(0x18_18_18, Color::Reset))
            },
            quick: plain(c(0xCC_CC_CC, Color::Reset), c(0x22_22_22, Color::Black)),
            quick_border: plain(c(0x31_31_31, Color::DarkGray), c(0x22_22_22, Color::Black)),
            quick_selected: pick(
                plain(c(0xFF_FF_FF, Color::White), c(0x04_39_5E, Color::Blue)),
                reversed,
            ),
            highlight: pick(
                Style::new().fg(c(0x2A_AA_FF, Color::LightBlue)),
                Style::new(),
            )
            .add_modifier(Modifier::BOLD),
            description: pick(
                Style::new().fg(c(0x9D_9D_9D, Color::DarkGray)),
                Style::new().add_modifier(Modifier::DIM),
            ),
            markers: severity_colors(mode).map(|color| {
                let underline = Style::new().add_modifier(Modifier::UNDERLINED);
                if none {
                    underline
                } else {
                    underline.underline_color(color)
                }
            }),
            letters: {
                let [error, warning, info, _] = severity_colors(mode);
                [error, warning, info].map(|color| pick(bold.fg(color), bold))
            },
        }
        .with_bold_errors(none)
    }

    /// Without colors, errors are bold as well as underlined.
    fn with_bold_errors(mut self, none: bool) -> Self {
        if none {
            self.markers[0] = self.markers[0].add_modifier(Modifier::BOLD);
        }
        self
    }

    /// The underline of a diagnostic of `severity`.
    pub(crate) fn marker(&self, severity: MarkerSeverity) -> Style {
        self.markers[severity_index(severity)]
    }

    /// The letter naming a problem's severity, and its style.
    pub(crate) fn letter(&self, severity: MarkerSeverity) -> (&'static str, Style) {
        match severity {
            MarkerSeverity::Error => ("E", self.letters[0]),
            MarkerSeverity::Warning => ("W", self.letters[1]),
            MarkerSeverity::Info => ("I", self.letters[2]),
            MarkerSeverity::Hint => ("H", self.description),
        }
    }
}

fn severity_index(severity: MarkerSeverity) -> usize {
    match severity {
        MarkerSeverity::Error => 0,
        MarkerSeverity::Warning => 1,
        MarkerSeverity::Info => 2,
        MarkerSeverity::Hint => 3,
    }
}

/// Error, warning, info and hint colors in `mode`.
fn severity_colors(mode: ColorMode) -> [Color; 4] {
    [
        color(mode, 0xF1_4C_4C, Color::LightRed),
        color(mode, 0xCC_A7_00, Color::Yellow),
        color(mode, 0x59_A4_F9, Color::LightBlue),
        color(mode, 0xB0_B0_B0, Color::Gray),
    ]
}

impl Theme {
    /// The style of text of token kind `kind`.
    pub(crate) fn token(&self, kind: TokenKind) -> Style {
        if self.mode == ColorMode::None {
            return Style::new();
        }
        let basic = match kind {
            TokenKind::Comment => Color::Green,
            TokenKind::String => Color::LightRed,
            TokenKind::Escape => Color::LightYellow,
            TokenKind::Number => Color::LightGreen,
            TokenKind::Boolean | TokenKind::Keyword => Color::Blue,
            TokenKind::ControlKeyword => Color::Magenta,
            TokenKind::Function => Color::Yellow,
            TokenKind::Type => Color::Cyan,
            TokenKind::Variable => Color::LightBlue,
            _ => Color::Reset,
        };
        Style::new().fg(color(self.mode, kind.dark_modern(), basic))
    }

    /// Selected text whose token style is `token`.
    pub(crate) fn selected(&self, token: Style) -> Style {
        match self.mode {
            ColorMode::TrueColor | ColorMode::Ansi256 => token.patch(self.selection),
            ColorMode::Ansi16 | ColorMode::None => self.selection,
        }
    }
}

/// The closest xterm 256-color index: the 6×6×6 cube or the gray ramp.
fn nearest_256(rgb: u32) -> u8 {
    const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    let [_, r, g, b] = rgb.to_be_bytes();
    let distance = |x: (u8, u8, u8)| {
        let d = |p: u8, q: u8| (i32::from(p) - i32::from(q)).pow(2);
        d(x.0, r) + d(x.1, g) + d(x.2, b)
    };
    let level = |v: u8| {
        (0..6u8)
            .min_by_key(|&i| (i32::from(LEVELS[usize::from(i)]) - i32::from(v)).abs())
            .unwrap_or(0)
    };
    let (ri, gi, bi) = (level(r), level(g), level(b));
    let cube = (
        LEVELS[usize::from(ri)],
        LEVELS[usize::from(gi)],
        LEVELS[usize::from(bi)],
    );
    let cube_index = 16 + 36 * ri + 6 * gi + bi;
    let gray = (0..24u8)
        .min_by_key(|&i| distance((8 + 10 * i, 8 + 10 * i, 8 + 10 * i)))
        .unwrap_or(0);
    let gray_level = 8 + 10 * gray;
    if distance((gray_level, gray_level, gray_level)) < distance(cube) {
        232 + gray
    } else {
        cube_index
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let vars: Vec<(String, String)> = vars
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |key| vars.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    }

    #[test]
    fn the_mode_follows_no_color_colorterm_and_term() {
        assert_eq!(
            ColorMode::detect(env(&[("NO_COLOR", "1"), ("COLORTERM", "truecolor")])),
            ColorMode::None
        );
        assert_eq!(
            ColorMode::detect(env(&[("NO_COLOR", "")])),
            ColorMode::Ansi16,
            "empty NO_COLOR is unset"
        );
        assert_eq!(
            ColorMode::detect(env(&[("COLORTERM", "truecolor")])),
            ColorMode::TrueColor
        );
        assert_eq!(
            ColorMode::detect(env(&[("TERM", "xterm-256color")])),
            ColorMode::Ansi256
        );
        assert_eq!(
            ColorMode::detect(env(&[("TERM", "linux")])),
            ColorMode::Ansi16
        );
        assert_eq!(
            ColorMode::detect(env(&[
                ("RUSTODE_COLOR", "none"),
                ("COLORTERM", "truecolor")
            ])),
            ColorMode::None
        );
    }

    #[test]
    fn token_colors_follow_the_mode() {
        let fg = |mode, kind| Theme::new(mode).token(kind).fg;
        assert_eq!(
            fg(ColorMode::TrueColor, TokenKind::Keyword),
            Some(Color::Rgb(0x56, 0x9C, 0xD6))
        );
        assert_eq!(
            fg(ColorMode::Ansi256, TokenKind::Function),
            Some(Color::Indexed(nearest_256(0xDC_DC_AA)))
        );
        assert_eq!(
            fg(ColorMode::Ansi16, TokenKind::Comment),
            Some(Color::Green)
        );
        assert_eq!(
            fg(ColorMode::Ansi16, TokenKind::Punctuation),
            Some(Color::Reset)
        );
        assert_eq!(fg(ColorMode::None, TokenKind::String), None);
        let sixteen = Theme::new(ColorMode::Ansi16);
        assert_eq!(
            sixteen.selected(sixteen.token(TokenKind::Keyword)),
            sixteen.selection,
            "no blue on blue"
        );
    }

    #[test]
    fn colors_map_to_the_256_palette() {
        assert_eq!(nearest_256(0x00_00_00), 16);
        assert_eq!(nearest_256(0xFF_FF_FF), 231);
        assert_eq!(
            nearest_256(0x1F_1F_1F),
            234,
            "dark gray goes to the gray ramp"
        );
        assert_eq!(nearest_256(0x26_4F_78), 24);
    }
}
