//! The fuzzy score the suggest widget filters and sorts with: a
//! reimplementation of `fuzzyScore` in VS Code's `src/vs/base/common/filters.ts`
//! (`:521-919`, with `FuzzyScoreOptions.default`), which is not the scorer
//! quick open uses (`crate::fuzzy`, VS Code's `fuzzyScorer.ts`).
//!
//! Every pattern char must occur in the word, in order, ignoring case. The
//! match is scored on a table of the best score of each pattern prefix at
//! each word position (`:683-800`): a char scores 7 (or 5 when the case
//! differs) at the start of the word, 5 when it starts an upper-case run or
//! follows a separator, 1 elsewhere; a contiguous run gains 1 per char
//! unless it sits on a gap location, a match that starts after a gap on a
//! gap location gains 2, the first pattern char loses 3 or 5 when it does
//! not match the word's first char, a gap loses 5 and the end of the word
//! counts as a gap (`_doScore`, `:847-917`). A match whose first char is not
//! at a "strong" place (word start, upper-case or separator) is refused
//! (`firstMatchCanBeWeak: false`, `:812`), a word the pattern covers fully
//! gets 2 more (`boostFullMatch`, `:896`) and each skipped char takes 1 off.
//! Words and patterns are cut at 128 chars (`_maxLen`).

/// The longest pattern or word scored.
const MAX_LEN: usize = 128;

/// How a pattern matched a word.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Match {
    /// Higher is better.
    pub score: i32,
    /// The word's matched chars, ascending.
    pub positions: Vec<usize>,
}

fn lower(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// `isSeparatorAtPos` (`:584-612`), without the emoji case.
fn separator(word: &[char], at: isize) -> bool {
    usize::try_from(at)
        .ok()
        .and_then(|at| word.get(at))
        .is_some_and(|c| {
            matches!(
                c,
                '_' | '-'
                    | '.'
                    | ' '
                    | '/'
                    | '\\'
                    | '\''
                    | '"'
                    | ':'
                    | '$'
                    | '<'
                    | '>'
                    | '('
                    | ')'
                    | '['
                    | ']'
                    | '{'
                    | '}'
            )
        })
}

fn whitespace(word: &[char], at: isize) -> bool {
    usize::try_from(at)
        .ok()
        .and_then(|at| word.get(at))
        .is_some_and(|c| matches!(c, ' ' | '\t'))
}

/// The word and pattern as the scorer reads them.
struct Text {
    pattern: Vec<char>,
    pattern_low: Vec<char>,
    word: Vec<char>,
    word_low: Vec<char>,
}

impl Text {
    fn upper(&self, at: usize) -> bool {
        self.word[at] != self.word_low[at]
    }

    /// The first part of `_doScore`: the score of a match at `wp` by where
    /// it falls in the word, and whether that place is a gap location.
    fn place(&self, pp: usize, start: usize, wp: usize) -> (i32, bool) {
        let at = isize::try_from(wp).unwrap_or(isize::MAX);
        let exact = self.pattern[pp] == self.word[wp];
        let case = if exact { 7 } else { 5 };
        if wp == pp - start {
            (case, false)
        } else if self.upper(wp) && (wp == 0 || !self.upper(wp - 1)) {
            (case, true)
        } else if separator(&self.word_low, at) && (wp == 0 || !separator(&self.word_low, at - 1)) {
            (5, false)
        } else if separator(&self.word_low, at - 1) || whitespace(&self.word_low, at - 1) {
            (5, true)
        } else {
            (1, false)
        }
    }

    /// `_doScore`: the score of matching pattern char `pp` with word char
    /// `wp`, or `None` when they differ. `strong` is set when the first
    /// pattern char matches a strong place.
    fn char_score(
        &self,
        pp: usize,
        start: usize,
        wp: usize,
        new_match: bool,
        strong: &mut bool,
    ) -> Option<i32> {
        if self.pattern_low[pp] != self.word_low[wp] {
            return None;
        }
        let at = isize::try_from(wp).unwrap_or(isize::MAX);
        let (mut score, mut gap) = self.place(pp, start, wp);
        if score > 1 && pp == start {
            *strong = true;
        }
        if !gap {
            gap = self.upper(wp)
                || separator(&self.word_low, at - 1)
                || whitespace(&self.word_low, at - 1);
        }
        if pp == start {
            if wp > 0 {
                score -= if gap { 3 } else { 5 };
            }
        } else if new_match {
            score += if gap { 2 } else { 0 };
        } else {
            score += i32::from(!gap);
        }
        if wp + 1 == self.word.len() {
            score -= if gap { 3 } else { 5 };
        }
        Some(score)
    }
}

/// Where each pattern char first and last can match the word.
fn bounds(text: &Text, start: usize) -> Option<(Vec<usize>, Vec<usize>)> {
    let (plen, wlen) = (text.pattern.len(), text.word.len());
    let mut min = vec![0; plen];
    let (mut pp, mut wp) = (start, 0);
    while pp < plen && wp < wlen {
        if text.pattern_low[pp] == text.word_low[wp] {
            min[pp] = wp;
            pp += 1;
        }
        wp += 1;
    }
    if pp != plen {
        return None;
    }
    let mut max = vec![0; plen];
    let (mut pp, mut wp) = (plen, wlen);
    while pp > start && wp > 0 {
        if text.pattern_low[pp - 1] == text.word_low[wp - 1] {
            max[pp - 1] = wp - 1;
            pp -= 1;
        }
        wp -= 1;
    }
    Some((min, max))
}

/// The tables `fuzzyScore` fills: best score, the arrow it came by (1
/// diagonal, 2 left, 3 left twice) and the length of the diagonal run.
struct Tables {
    score: Vec<Vec<i32>>,
    arrow: Vec<Vec<u8>>,
    diag: Vec<Vec<usize>>,
    /// Whether the first pattern char matched a strong place.
    strong: bool,
}

/// Scores `pattern` (from char `start`, which skips leading blanks) against
/// `word`; `None` unless every char matches in order and the first one sits
/// at a strong place.
pub(crate) fn fuzzy_score(pattern: &str, word: &str, start: usize) -> Option<Match> {
    let pattern: Vec<char> = pattern.chars().take(MAX_LEN).collect();
    let word: Vec<char> = word.chars().take(MAX_LEN).collect();
    let (plen, wlen) = (pattern.len(), word.len());
    if start >= plen || wlen == 0 || plen - start > wlen {
        return None;
    }
    let text = Text {
        pattern_low: pattern.iter().map(|&c| lower(c)).collect(),
        word_low: word.iter().map(|&c| lower(c)).collect(),
        pattern,
        word,
    };
    let (min, max) = bounds(&text, start)?;
    let rows = plen - start;
    let mut tables = Tables {
        score: vec![vec![0; wlen + 1]; rows + 1],
        arrow: vec![vec![0; wlen + 1]; rows + 1],
        diag: vec![vec![0; wlen + 1]; rows + 1],
        strong: false,
    };
    for row in 1..=rows {
        fill_row(&text, (&min, &max), start, row, &mut tables);
    }
    if !tables.strong {
        return None;
    }
    Some(walk_back(&text, &tables, start, rows))
}

fn fill_row(
    text: &Text,
    (min, max): (&[usize], &[usize]),
    start: usize,
    row: usize,
    tables: &mut Tables,
) {
    let pp = start + row - 1;
    let next_max = if pp + 1 < text.pattern.len() {
        max[pp + 1]
    } else {
        text.word.len()
    };
    for wp in min[pp]..next_max {
        let column = wp + 1;
        let scored = if wp <= max[pp] {
            text.char_score(
                pp,
                start,
                wp,
                tables.diag[row - 1][column - 1] == 0,
                &mut tables.strong,
            )
        } else {
            None
        };
        let diag_score = scored.map(|s| s + tables.score[row - 1][column - 1]);
        let can_left = wp > min[pp];
        let left_score = if can_left {
            tables.score[row][column - 1]
                - if tables.diag[row][column - 1] > 0 {
                    5
                } else {
                    0
                }
        } else {
            0
        };
        let can_left_left = wp > min[pp] + 1 && tables.diag[row][column - 1] > 0;
        let left_left_score = if can_left_left {
            tables.score[row][column - 2]
                - if tables.diag[row][column - 2] > 0 {
                    5
                } else {
                    0
                }
        } else {
            0
        };
        let (value, arrow, run) = if can_left_left
            && (!can_left || left_left_score >= left_score)
            && diag_score.is_none_or(|d| left_left_score >= d)
        {
            (left_left_score, 3, 0)
        } else if can_left && diag_score.is_none_or(|d| left_score >= d) {
            (left_score, 2, 0)
        } else if let Some(d) = diag_score {
            (d, 1, tables.diag[row - 1][column - 1] + 1)
        } else {
            continue;
        };
        tables.score[row][column] = value;
        tables.arrow[row][column] = arrow;
        tables.diag[row][column] = run;
    }
}

/// The matched positions, from the arrows (`:822-880`), and the final score.
fn walk_back(text: &Text, tables: &Tables, start: usize, rows: usize) -> Match {
    let (plen, wlen) = (text.pattern.len(), text.word.len());
    let mut row = rows;
    let mut column = wlen;
    let mut score = tables.score[row][column];
    let mut positions = Vec::with_capacity(rows);
    let mut backwards = 0_usize;
    let mut max_match_column = 0;
    while row >= 1 {
        let mut diag_column = column;
        while diag_column >= 1 {
            match tables.arrow[row][diag_column] {
                3 => diag_column = diag_column.saturating_sub(2),
                2 => diag_column -= 1,
                _ => break,
            }
        }
        if backwards > 1
            && diag_column >= 1
            && text.pattern_low[start + row - 1] == text.word_low[column - 1]
            && !text.upper(diag_column - 1)
            && backwards + 1 > tables.diag[row][diag_column]
        {
            diag_column = column;
        }
        backwards = if diag_column == column {
            backwards + 1
        } else {
            1
        };
        if max_match_column == 0 {
            max_match_column = diag_column;
        }
        row -= 1;
        column = diag_column.saturating_sub(1);
        positions.push(column);
    }
    if wlen == plen {
        score += 2;
    }
    let skipped = i32::try_from(max_match_column).unwrap_or(0) - i32::try_from(plen).unwrap_or(0);
    score -= skipped;
    positions.reverse();
    Match { score, positions }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn score(pattern: &str, word: &str) -> Option<i32> {
        fuzzy_score(pattern, word, 0).map(|m| m.score)
    }

    #[test]
    fn every_char_must_match_in_order_at_a_strong_start() {
        assert!(score("fo", "foo").is_some());
        assert_eq!(score("fx", "foo"), None);
        assert_eq!(score("of", "foo"), None, "in order");
        assert_eq!(score("oo", "foo"), None, "the first char is weak");
        assert!(score("fb", "foo_bar").is_some());
        assert!(score("fb", "fooBar").is_some());
        assert_eq!(score("toolong", "to"), None);
        assert_eq!(score("", "x"), None);
    }

    #[test]
    fn prefixes_and_word_starts_outscore_scattered_matches() {
        let prefix = score("pri", "println").expect("prefix");
        let scattered = score("pri", "pair_index").expect("scattered");
        assert!(prefix > scattered, "{prefix} > {scattered}");
        let exact = score("print", "print").expect("exact");
        let longer = score("print", "println").expect("longer");
        assert!(exact > longer, "a full match gets the boost");
        assert!(score("pk", "PrimaryKey").is_some(), "camel humps match");
    }

    #[test]
    fn the_matched_positions_are_reported() {
        let matched = fuzzy_score("pk", "PrimaryKey", 0).expect("match");
        assert_eq!(matched.positions, vec![0, 7]);
        let matched = fuzzy_score("fb", "foo_bar", 0).expect("match");
        assert_eq!(matched.positions, vec![0, 4]);
        let prefix = fuzzy_score("pri", "println", 0).expect("match");
        assert_eq!(prefix.positions, vec![0, 1, 2]);
    }

    #[test]
    fn case_is_ignored_but_exact_case_scores_more() {
        let same = score("Po", "Point").expect("same case");
        let other = score("po", "Point").expect("other case");
        assert!(same > other);
    }

    #[test]
    fn leading_blanks_of_the_pattern_are_skipped() {
        let skipped = fuzzy_score("  fo", "foo", 2).expect("match");
        assert_eq!(skipped.positions, vec![0, 1]);
        assert_eq!(fuzzy_score("  ", "foo", 2), None);
    }
}
