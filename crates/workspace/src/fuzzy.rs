//! Fuzzy matching for quick open, reimplementing the scoring of VS Code's
//! `fuzzyScorer.ts` (`src/vs/base/common/fuzzyScorer.ts`):
//!
//! - Every query char must match, in order, case-insensitively; `/` and `\`
//!   match each other (`considerAsEqual`, :238-249).
//! - Each matched char scores 1, plus a run bonus (6 per char for the first
//!   three of a run, 3 after), plus 1 for the same case, plus 8 at the start
//!   of the target, else 5 after `/` or `\`, 4 after `_ - . space ' " :`,
//!   else 2 for an uppercase char that starts a run (`computeCharScore`,
//!   :158-236; `scoreSeparatorAtPos`, :251-267). A table keeps the best
//!   score for every query/target prefix (`doScoreFuzzy`, :52-156).
//! - Items (a file) score their label (file name) first: a prefix match gets
//!   `1 << 17` plus the share of the label the query covers, any other label
//!   match `1 << 16`; otherwise the label is scored together with its folder.
//!   A query equal to the item's path scores `1 << 18`
//!   (`doScoreItemFuzzy`, :450-560). Space-separated pieces must all match.

/// The score and the matched char positions of one target.
type Scored = (u32, Vec<usize>);

const PATH_IDENTITY_SCORE: u32 = 1 << 18;
const LABEL_PREFIX_SCORE: u32 = 1 << 17;
const LABEL_SCORE: u32 = 1 << 16;

/// Scores `query` against `target`; `None` unless every query char matches.
pub(crate) fn score(target: &str, query: &str) -> Option<Scored> {
    let target: Vec<char> = target.chars().collect();
    let query: Vec<char> = query.chars().collect();
    if query.is_empty() || target.len() < query.len() {
        return None;
    }
    let width = target.len();
    let mut scores = vec![0_u32; query.len() * width];
    let mut runs = vec![0_u32; query.len() * width];
    for (qi, &q) in query.iter().enumerate() {
        for (ti, &t) in target.iter().enumerate() {
            let index = qi * width + ti;
            let left = if ti > 0 { scores[index - 1] } else { 0 };
            let (diagonal, run) = if qi > 0 && ti > 0 {
                let previous = (qi - 1) * width + ti - 1;
                (scores[previous], runs[previous])
            } else {
                (0, 0)
            };
            let char_score = if qi > 0 && diagonal == 0 {
                0
            } else {
                char_score(q, &target, ti, t, run)
            };
            if char_score > 0 && diagonal + char_score >= left {
                runs[index] = run + 1;
                scores[index] = diagonal + char_score;
            } else {
                scores[index] = left;
            }
        }
    }
    let total = scores[query.len() * width - 1];
    if total == 0 {
        return None;
    }
    let mut positions = Vec::with_capacity(query.len());
    let (mut qi, mut ti) = (query.len(), width);
    while qi > 0 && ti > 0 {
        if runs[(qi - 1) * width + ti - 1] == 0 {
            ti -= 1;
        } else {
            positions.push(ti - 1);
            qi -= 1;
            ti -= 1;
        }
    }
    positions.reverse();
    Some((total, positions))
}

fn lower(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

fn same(q: char, t: char) -> bool {
    lower(q) == lower(t) || (matches!(q, '/' | '\\') && matches!(t, '/' | '\\'))
}

fn char_score(q: char, target: &[char], ti: usize, t: char, run: u32) -> u32 {
    if !same(q, t) {
        return 0;
    }
    let mut score = 1;
    if run > 0 {
        score += run.min(3) * 6 + run.saturating_sub(3) * 3;
    }
    if q == t {
        score += 1;
    }
    if ti == 0 {
        score += 8;
    } else {
        match target[ti - 1] {
            '/' | '\\' => score += 5,
            '_' | '-' | '.' | ' ' | '\'' | '"' | ':' => score += 4,
            _ if t.is_uppercase() && run == 0 => score += 2,
            _ => {}
        }
    }
    score
}

/// How an item matched a query: its score and the matched char positions in
/// its label and in its description.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemScore {
    /// Higher is better.
    pub score: u32,
    /// Matched chars of the label.
    pub label: Vec<usize>,
    /// Matched chars of the description.
    pub description: Vec<usize>,
}

/// Scores an item (`label`, e.g. a file name, and `description`, e.g. its
/// folder) against a query of space-separated pieces.
pub(crate) fn score_item(
    label: &str,
    description: &str,
    path: &str,
    query: &str,
) -> Option<ItemScore> {
    let normalized: String = query.chars().filter(|c| !matches!(c, '*' | '"')).collect();
    if normalized.trim() == path {
        return Some(ItemScore {
            score: PATH_IDENTITY_SCORE,
            label: (0..label.chars().count()).collect(),
            description: Vec::new(),
        });
    }
    let mut total = ItemScore {
        score: 0,
        label: Vec::new(),
        description: Vec::new(),
    };
    let mut pieces = normalized.split_whitespace().peekable();
    pieces.peek()?;
    for piece in pieces {
        let scored = score_piece(label, description, piece)?;
        total.score = total.score.saturating_add(scored.score);
        total.label.extend(scored.label);
        total.description.extend(scored.description);
    }
    total.label.sort_unstable();
    total.label.dedup();
    total.description.sort_unstable();
    total.description.dedup();
    Some(total)
}

fn score_piece(label: &str, description: &str, piece: &str) -> Option<ItemScore> {
    if let Some((label_score, positions)) = score(label, piece) {
        let prefix = label.to_lowercase().starts_with(&piece.to_lowercase());
        let base = if prefix {
            let share = piece.chars().count() * 100 / label.chars().count().max(1);
            LABEL_PREFIX_SCORE + u32::try_from(share).unwrap_or(100)
        } else {
            LABEL_SCORE
        };
        return Some(ItemScore {
            score: base + label_score,
            label: positions,
            description: Vec::new(),
        });
    }
    if description.is_empty() {
        return None;
    }
    let prefix_len = description.chars().count() + 1;
    let (combined, positions) = score(&format!("{description}/{label}"), piece)?;
    let (label_part, description_part): (Vec<usize>, Vec<usize>) =
        positions.into_iter().partition(|&p| p >= prefix_len);
    Some(ItemScore {
        score: combined,
        label: label_part.into_iter().map(|p| p - prefix_len).collect(),
        description: description_part
            .into_iter()
            .filter(|&p| p < prefix_len - 1)
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_query_chars_must_match_in_order() {
        assert!(score("window.ts", "wts").is_some());
        assert!(score("window.ts", "tw").is_none());
        assert!(score("ab", "abc").is_none());
        assert_eq!(
            score("src/main.rs", "smr").map(|s| s.1),
            Some(vec![0, 4, 9])
        );
    }

    #[test]
    fn runs_word_starts_and_case_score_higher() {
        let run = score("abcdef", "abc").expect("match").0;
        let spread = score("axbxcx", "abc").expect("match").0;
        assert!(run > spread);
        let word_start = score("foo_bar", "b").expect("match").0;
        let middle = score("foobar", "b").expect("match").0;
        assert!(word_start > middle);
    }
}
