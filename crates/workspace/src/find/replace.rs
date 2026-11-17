//! Replace strings, as VS Code's `ReplacePattern` reads and builds them
//! (`src/vs/editor/contrib/find/browser/replacePattern.ts`).

use super::search::Hit;

/// What replaces each match: text, and for regex queries the groups of
/// the match.
pub(super) struct ReplacePattern {
    pieces: Vec<Piece>,
}

enum Piece {
    Text(String),
    /// Group `index` of the match (0 for all of it), with the case changes
    /// to apply to its chars, in order.
    Group {
        index: usize,
        case: Vec<Case>,
    },
}

/// `\u`, `\l` change one char; `\U`, `\L` every char left.
#[derive(Clone, Copy)]
enum Case {
    Upper,
    Lower,
    AllUpper,
    AllLower,
}

impl ReplacePattern {
    /// The replace string of a query: taken literally for plain text, read
    /// with VS Code's syntax for a regex (`findModel.ts:474-479`).
    pub fn new(replacement: &str, regex: bool) -> Self {
        if regex {
            Self::parse(replacement)
        } else {
            Self {
                pieces: vec![Piece::Text(replacement.to_owned())],
            }
        }
    }

    /// `parseReplaceString` (`replacePattern.ts:215-347`): `\n` is a LF,
    /// `\t` a tab, `\\` a backslash; `\u \l \U \L` change the case of the
    /// next group; `$$` is a `$`; `$&` and `$0` are the whole match; `$n`
    /// and `$nn` are groups. Anything else stays as written.
    fn parse(replacement: &str) -> Self {
        let mut pieces = Vec::new();
        let mut text = String::new();
        let mut case = Vec::new();
        let mut chars = replacement.chars().peekable();
        while let Some(c) = chars.next() {
            match (c, chars.next_if(|_| c == '\\' || c == '$')) {
                ('\\', Some('\\')) => text.push('\\'),
                ('\\', Some('n')) => text.push('\n'),
                ('\\', Some('t')) => text.push('\t'),
                ('\\', Some(op @ ('u' | 'l' | 'U' | 'L'))) => case.push(Case::of(op)),
                ('$', Some('$')) => text.push('$'),
                ('$', Some(first @ ('0'..='9' | '&'))) => {
                    let mut index = digit(first);
                    if index > 0
                        && let Some(second) = chars.next_if(char::is_ascii_digit)
                    {
                        index = index * 10 + digit(second);
                    }
                    if !text.is_empty() {
                        pieces.push(Piece::Text(std::mem::take(&mut text)));
                    }
                    let case = std::mem::take(&mut case);
                    pieces.push(Piece::Group { index, case });
                }
                (c, next) => {
                    text.push(c);
                    text.extend(next);
                }
            }
        }
        if !text.is_empty() {
            pieces.push(Piece::Text(text));
        }
        Self { pieces }
    }

    /// Whether building needs the groups of the match
    /// (`hasReplacementPatterns`, `replacePattern.ts:38-40`).
    pub fn has_groups(&self) -> bool {
        self.pieces.iter().any(|p| matches!(p, Piece::Group { .. }))
    }

    /// The text that replaces `hit` (`buildReplaceString`, `replacePattern.ts:52-106`).
    pub fn build(&self, hit: &Hit) -> String {
        let mut out = String::new();
        for piece in &self.pieces {
            match piece {
                Piece::Text(text) => out.push_str(text),
                Piece::Group { index, case } => {
                    change_case(&mut out, &substitute(*index, hit), case);
                }
            }
        }
        out
    }
}

impl Case {
    fn of(op: char) -> Self {
        match op {
            'u' => Self::Upper,
            'l' => Self::Lower,
            'U' => Self::AllUpper,
            _ => Self::AllLower,
        }
    }
}

/// The value of an ASCII digit; 0 for anything else.
fn digit(c: char) -> usize {
    "0123456789".find(c).unwrap_or(0)
}

/// The text of group `index`, as JavaScript's `replace` reads `$nn`
/// (`_substitute`, `replacePattern.ts:108-127`): a group that does not
/// exist drops its last digit, which becomes text after it, until one
/// does; with none, the `$` and the digits stay as written. A group that
/// took no part in the match is empty.
fn substitute(index: usize, hit: &Hit) -> String {
    if index == 0 {
        return hit.group(0).unwrap_or_default().to_owned();
    }
    let mut index = index;
    let mut rest = String::new();
    while index > 0 {
        if index < hit.group_count() {
            return format!("{}{rest}", hit.group(index).unwrap_or_default());
        }
        rest.insert_str(0, &(index % 10).to_string());
        index /= 10;
    }
    format!("${rest}")
}

/// Appends `group` to `out` with the case changes applied, one per char
/// until an `\U` or `\L`, which applies to every char left
/// (`replacePattern.ts:72-101`).
fn change_case(out: &mut String, group: &str, case: &[Case]) {
    let mut ops = case.iter();
    let mut op = ops.next();
    for (at, c) in group.char_indices() {
        let Some(current) = op else {
            out.push_str(&group[at..]);
            return;
        };
        match current {
            Case::Upper | Case::AllUpper => out.extend(c.to_uppercase()),
            Case::Lower | Case::AllLower => out.extend(c.to_lowercase()),
        }
        if matches!(current, Case::Upper | Case::Lower) {
            op = ops.next();
        }
    }
}
