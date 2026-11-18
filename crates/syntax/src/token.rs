//! Token kinds and their Dark Modern colors.
//!
//! Each kind stands for the grammar scope that VS Code's Rust grammar gives
//! the same construct (`extensions/rust/syntaxes/rust.tmLanguage.json`), and
//! its color is what Dark Modern resolves that scope to. The resolution was
//! studied in VS Code (Code - OSS) and reimplemented in the test below, see
//! `ATTRIBUTION.md`:
//!
//! - Dark Modern includes Dark+, which includes Dark (Visual Studio)
//!   (`extensions/theme-defaults/themes/dark_modern.json:4`,
//!   `dark_plus.json:4`); an included theme's rules come first
//!   (`src/vs/workbench/services/themes/common/colorThemeData.ts:784-785`,
//!   `:807-810`), after a default rule with the editor foreground
//!   (`:108-116`), `#CCCCCC` in Dark Modern (`dark_modern.json:35`);
//! - a rule matches a scope that equals its selector or extends it after a
//!   dot; the longer selector wins, and among equal ones the later rule
//!   (`colorThemeData.ts:358-375`, `:882-908`).
//!
//! | Kind | Scope (Rust grammar line) | Rule that wins | Color |
//! |---|---|---|---|
//! | `Comment` | `comment.line.double-slash.rust` (`:267`) | `comment` (`dark_vs.json:80-82`) | `#6A9955` |
//! | `String` | `string.quoted.double.rust` (`:1098`) | `string` (`dark_vs.json:282-287`) | `#CE9178` |
//! | `Escape` | `constant.character.escape.rust` (`:388`) | `constant.character.escape` (`dark_plus.json:186-191`) | `#D7BA7D` |
//! | `Number` | `constant.numeric.decimal.rust` (`:329`) | `constant.numeric` (`dark_vs.json:92-99`) | `#B5CEA8` |
//! | `Boolean` | `constant.language.bool.rust` (`:381`) | `constant.language` (`dark_vs.json:86-88`) | `#569CD6` |
//! | `Keyword` | `keyword.other.rust`, `storage.type.rust`, `storage.modifier.rust`, `variable.language.self.rust` (`:688-738`, `:1178`) | `keyword`, `storage`, `variable.language` (`dark_vs.json:261-278`, `:340-342`, `:413-416`) | `#569CD6` |
//! | `ControlKeyword` | `keyword.control.rust` (`:683-684`) | `keyword.control` of Dark+ (`dark_plus.json:75-87`) | `#C586C0` |
//! | `Function` | `entity.name.function.rust`, `entity.name.function.macro.rust` (`:485`, `:112`) | `entity.name.function` (`dark_plus.json:7-17`) | `#DCDCAA` |
//! | `Type` | `entity.name.type.rust`, `entity.name.namespace.rust` (`:947`, `:922`) | `entity.name.type`, `entity.name.namespace` (`dark_plus.json:20-57`) | `#4EC9B0` |
//! | `Variable` | `variable.other.rust` (`:1192`) | `variable` (`dark_plus.json:90-100`) | `#9CDCFE` |
//! | `Constant` | `constant.other.caps.rust` (`:312`) | none: the default | `#CCCCCC` |
//! | `Operator` | `keyword.operator.namespace.rust`, `keyword.operator.access.dot.rust` (`:800`, `:819`) | `keyword.operator` (`dark_vs.json:352-354`) | `#D4D4D4` |
//! | `Punctuation` | `punctuation.brackets.round.rust`, `punctuation.comma.rust` (`:1074`, `:1064`) | none: the default | `#CCCCCC` |
//! | `Attribute` | `meta.attribute.rust` (`:410`) | none: the default | `#CCCCCC` |

/// What a run of highlighted text is, for choosing its color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TokenKind {
    /// Line, block and doc comments.
    Comment,
    /// String and char literals.
    String,
    /// Escape sequences inside literals, such as `\n`.
    Escape,
    /// Integer and float literals.
    Number,
    /// `true` and `false`.
    Boolean,
    /// Declaration and other keywords (`fn`, `let`, `pub`, `mut`, `self`).
    Keyword,
    /// Control flow keywords (`if`, `match`, `return`, `loop`).
    ControlKeyword,
    /// Function and method names, and macros.
    Function,
    /// Types, enum variants, lifetimes and path segments.
    Type,
    /// Variables, parameters and fields.
    Variable,
    /// All-caps names, taken to be constants.
    Constant,
    /// `::`, `:`, `.`, `*` and `&`.
    Operator,
    /// Brackets, commas and semicolons.
    Punctuation,
    /// Attributes such as `#[derive(Debug)]`, where nothing inside is more
    /// specific.
    Attribute,
}

impl TokenKind {
    /// Every kind, in declaration order.
    pub const ALL: [Self; 14] = [
        Self::Comment,
        Self::String,
        Self::Escape,
        Self::Number,
        Self::Boolean,
        Self::Keyword,
        Self::ControlKeyword,
        Self::Function,
        Self::Type,
        Self::Variable,
        Self::Constant,
        Self::Operator,
        Self::Punctuation,
        Self::Attribute,
    ];

    /// The kind's foreground in Dark Modern, as `0xRRGGBB` (see the module
    /// docs for where each one comes from).
    #[must_use]
    pub const fn dark_modern(self) -> u32 {
        match self {
            Self::Comment => 0x6A_99_55,
            Self::String => 0xCE_91_78,
            Self::Escape => 0xD7_BA_7D,
            Self::Number => 0xB5_CE_A8,
            Self::Boolean | Self::Keyword => 0x56_9C_D6,
            Self::ControlKeyword => 0xC5_86_C0,
            Self::Function => 0xDC_DC_AA,
            Self::Type => 0x4E_C9_B0,
            Self::Variable => 0x9C_DC_FE,
            Self::Operator => 0xD4_D4_D4,
            Self::Constant | Self::Punctuation | Self::Attribute => DEFAULT_FOREGROUND,
        }
    }

    /// The grammar scope VS Code's Rust grammar gives this kind of text
    /// (one of them, when several share the kind).
    #[must_use]
    pub const fn scope(self) -> &'static str {
        match self {
            Self::Comment => "comment.line.double-slash.rust",
            Self::String => "string.quoted.double.rust",
            Self::Escape => "constant.character.escape.rust",
            Self::Number => "constant.numeric.decimal.rust",
            Self::Boolean => "constant.language.bool.rust",
            Self::Keyword => "keyword.other.rust",
            Self::ControlKeyword => "keyword.control.rust",
            Self::Function => "entity.name.function.rust",
            Self::Type => "entity.name.type.rust",
            Self::Variable => "variable.other.rust",
            Self::Constant => "constant.other.caps.rust",
            Self::Operator => "keyword.operator.namespace.rust",
            Self::Punctuation => "punctuation.brackets.round.rust",
            Self::Attribute => "meta.attribute.rust",
        }
    }
}

/// Dark Modern's `editor.foreground` (`dark_modern.json:35`), the color of
/// text that no rule matches.
pub const DEFAULT_FOREGROUND: u32 = 0xCC_CC_CC;

#[cfg(test)]
mod tests {
    use super::*;

    /// The Dark (Visual Studio) and Dark+ rules that match Rust scopes, in
    /// the order Dark Modern loads them (Dark (Visual Studio) first), with
    /// their colors as written in the theme files.
    const RULES: &[(&str, u32)] = &[
        ("comment", 0x6A_99_55),                   // dark_vs.json:80-82
        ("constant.language", 0x56_9C_D6),         // :86-88
        ("constant.numeric", 0xB5_CE_A8),          // :92-99
        ("storage", 0x56_9C_D6),                   // :261-263
        ("storage.type", 0x56_9C_D6),              // :267-269
        ("storage.modifier", 0x56_9C_D6),          // :273-278
        ("string", 0xCE_91_78),                    // :282-287
        ("keyword", 0x56_9C_D6),                   // :340-342
        ("keyword.control", 0x56_9C_D6),           // :346-348
        ("keyword.operator", 0xD4_D4_D4),          // :352-354
        ("variable.language", 0x56_9C_D6),         // :413-416
        ("entity.name.function", 0xDC_DC_AA),      // dark_plus.json:7-17
        ("entity.name.type", 0x4E_C9_B0),          // :20-57
        ("entity.name.namespace", 0x4E_C9_B0),     // :20-57
        ("keyword.control", 0xC5_86_C0),           // :75-87
        ("variable", 0x9C_DC_FE),                  // :90-100
        ("variable.other.constant", 0x4F_C1_FF),   // :103-111
        ("constant.character", 0x56_9C_D6),        // :178-184
        ("constant.character.escape", 0xD7_BA_7D), // :186-191
    ];

    /// The winning color for one scope: the longest matching selector, the
    /// later rule among equals, the editor foreground when none matches.
    fn resolve(scope: &str) -> u32 {
        let matches = |selector: &str| {
            scope == selector
                || (scope.starts_with(selector) && scope.as_bytes()[selector.len()] == b'.')
        };
        let mut best = (0, DEFAULT_FOREGROUND);
        for &(selector, color) in RULES {
            if matches(selector) && selector.len() >= best.0 {
                best = (selector.len(), color);
            }
        }
        best.1
    }

    #[test]
    fn each_kind_has_the_color_dark_modern_gives_its_scope() {
        for kind in TokenKind::ALL {
            assert_eq!(
                kind.dark_modern(),
                resolve(kind.scope()),
                "{kind:?} ({})",
                kind.scope()
            );
        }
    }

    #[test]
    fn the_scopes_sharing_a_kind_resolve_to_its_color() {
        let shared = [
            ("storage.type.rust", TokenKind::Keyword),
            ("storage.modifier.mut.rust", TokenKind::Keyword),
            ("variable.language.self.rust", TokenKind::Keyword),
            ("entity.name.function.macro.rust", TokenKind::Function),
            ("entity.name.namespace.rust", TokenKind::Type),
            ("entity.name.type.lifetime.rust", TokenKind::Type),
            ("entity.name.type.numeric.rust", TokenKind::Type),
            ("comment.block.documentation.rust", TokenKind::Comment),
            ("string.quoted.single.char.rust", TokenKind::String),
            ("keyword.operator.access.dot.rust", TokenKind::Operator),
            ("punctuation.semi.rust", TokenKind::Punctuation),
        ];
        for (scope, kind) in shared {
            assert_eq!(resolve(scope), kind.dark_modern(), "{scope}");
        }
    }
}
