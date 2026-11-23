//! A line diff: the shortest edit script between two sequences, by Myers'
//! greedy algorithm ("An O(ND) Difference Algorithm and Its Variations",
//! 1986), written from the paper. Clean-room: no code from VS Code or any
//! diff library.
//!
//! The sequences are lists of line ids (equal text, equal id). The common
//! start and end are trimmed first, so a big file with a few edits costs
//! little. The search stops once the script would need more than `max_edits`
//! inserted plus deleted lines, and returns `None`; that bounds the time
//! (`O(N·D)`) and the memory (`O(D²)`) whatever the input.

/// One step of the script, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Op {
    /// The next line is in both.
    Keep,
    /// The next line of `old` is not in `new`.
    Delete,
    /// The next line of `new` is not in `old`.
    Insert,
}

/// The shortest script turning `old` into `new`, or `None` when it needs
/// more than `max_edits` inserted plus deleted lines.
pub(super) fn diff(old: &[u32], new: &[u32], max_edits: usize) -> Option<Vec<Op>> {
    let prefix = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    let suffix = old[prefix..]
        .iter()
        .rev()
        .zip(new[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let middle_old = &old[prefix..old.len() - suffix];
    let middle_new = &new[prefix..new.len() - suffix];
    let mut ops = vec![Op::Keep; prefix];
    ops.extend(middle(middle_old, middle_new, max_edits)?);
    ops.extend(std::iter::repeat_n(Op::Keep, suffix));
    Some(ops)
}

/// Myers on sequences that share no start or end line.
fn middle(old: &[u32], new: &[u32], max_edits: usize) -> Option<Vec<Op>> {
    if old.is_empty() || new.is_empty() {
        let mut ops = vec![Op::Delete; old.len()];
        ops.extend(std::iter::repeat_n(Op::Insert, new.len()));
        return (ops.len() <= max_edits).then_some(ops);
    }
    let (old_len, new_len) = (as_isize(old.len()), as_isize(new.len()));
    let cap = as_isize(max_edits.min(old.len() + new.len()));
    // `reach[diag + offset]` is the furthest position in `old` on a
    // diagonal (position in `old` minus position in `new`).
    let offset = cap + 1;
    let mut reach = vec![0_isize; usize::try_from(2 * cap + 3).ok()?];
    // `trace[round]` is `reach` for the diagonals -round..=round after it.
    let mut trace: Vec<Vec<isize>> = Vec::new();
    for round in 0..=cap {
        for diag in (-round..=round).step_by(2) {
            let down = diag == -round
                || (diag != round && reach[idx(diag - 1, offset)] < reach[idx(diag + 1, offset)]);
            let mut at_old = if down {
                reach[idx(diag + 1, offset)]
            } else {
                reach[idx(diag - 1, offset)] + 1
            };
            let mut at_new = at_old - diag;
            while at_old < old_len && at_new < new_len && old[idx(at_old, 0)] == new[idx(at_new, 0)]
            {
                at_old += 1;
                at_new += 1;
            }
            reach[idx(diag, offset)] = at_old;
            if at_old >= old_len && at_new >= new_len {
                trace.push(snapshot(&reach, round, offset));
                return Some(backtrack(&trace, old_len, new_len));
            }
        }
        trace.push(snapshot(&reach, round, offset));
    }
    None
}

fn snapshot(reach: &[isize], round: isize, offset: isize) -> Vec<isize> {
    reach[idx(-round, offset)..=idx(round, offset)].to_vec()
}

/// Walks the trace from the end back to the start, then reverses.
fn backtrack(trace: &[Vec<isize>], old_len: isize, new_len: isize) -> Vec<Op> {
    let mut ops = Vec::new();
    let (mut at_old, mut at_new) = (old_len, new_len);
    for round in (1..trace.len()).rev() {
        let round = as_isize(round);
        // The previous round holds the diagonals -(round-1)..=(round-1).
        let before = &trace[idx(round - 1, 0)];
        let reached = |diag: isize| before[idx(diag + round - 1, 0)];
        let diag = at_old - at_new;
        let down = diag == -round || (diag != round && reached(diag - 1) < reached(diag + 1));
        let from = if down { diag + 1 } else { diag - 1 };
        let from_old = reached(from);
        let from_new = from_old - from;
        // The matching run after the edit, walked backwards.
        let edit_end = if down { from_old } else { from_old + 1 };
        while at_old > edit_end {
            ops.push(Op::Keep);
            at_old -= 1;
        }
        ops.push(if down { Op::Insert } else { Op::Delete });
        at_old = from_old;
        at_new = from_new;
    }
    ops.extend(std::iter::repeat_n(Op::Keep, idx(at_old, 0)));
    ops.reverse();
    ops
}

/// `i` as an index; the callers keep it non-negative.
fn idx(i: isize, offset: isize) -> usize {
    usize::try_from(i + offset).unwrap_or(0)
}

fn as_isize(n: usize) -> isize {
    isize::try_from(n).unwrap_or(isize::MAX)
}

#[cfg(test)]
mod tests {
    use super::{Op, diff};

    /// The script applied to `old` must give `new`, and its length is the
    /// edit distance.
    fn check(old: &[u32], new: &[u32]) -> usize {
        let ops = diff(old, new, usize::MAX / 4).expect("no limit");
        let (mut i, mut j, mut edits) = (0, 0, 0);
        let mut rebuilt = Vec::new();
        for op in ops {
            match op {
                Op::Keep => {
                    assert_eq!(old[i], new[j], "kept lines are equal");
                    rebuilt.push(old[i]);
                    i += 1;
                    j += 1;
                }
                Op::Delete => {
                    i += 1;
                    edits += 1;
                }
                Op::Insert => {
                    rebuilt.push(new[j]);
                    j += 1;
                    edits += 1;
                }
            }
        }
        assert_eq!((i, j), (old.len(), new.len()), "the script consumes both");
        assert_eq!(rebuilt, new);
        edits
    }

    #[test]
    fn the_paper_example_needs_five_edits() {
        // ABCABBA -> CBABAC, the example of the paper: D = 5.
        let a = [1, 2, 3, 1, 2, 2, 1];
        let b = [3, 2, 1, 2, 1, 3];
        assert_eq!(check(&a, &b), 5);
    }

    #[test]
    fn empty_and_identical_inputs() {
        assert_eq!(check(&[], &[]), 0);
        assert_eq!(check(&[1, 2], &[]), 2);
        assert_eq!(check(&[], &[1, 2]), 2);
        assert_eq!(check(&[1, 2, 3], &[1, 2, 3]), 0);
        assert_eq!(check(&[1, 2, 3], &[4, 5]), 5);
    }

    #[test]
    fn a_single_change_in_the_middle_is_two_edits() {
        assert_eq!(check(&[1, 2, 3, 4, 5], &[1, 2, 9, 4, 5]), 2);
    }

    #[test]
    fn random_inputs_give_valid_scripts_no_longer_than_delete_all_insert_all() {
        let mut seed = 0x2545_f491_u64;
        let mut next = move |bound: u64| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            u32::try_from(seed % bound).unwrap_or(0)
        };
        for _ in 0..300 {
            let old: Vec<u32> = (0..next(30)).map(|_| next(5)).collect();
            let new: Vec<u32> = (0..next(30)).map(|_| next(5)).collect();
            assert!(check(&old, &new) <= old.len() + new.len());
        }
    }

    #[test]
    fn the_limit_stops_the_search() {
        let old: Vec<u32> = (0..100).collect();
        let new: Vec<u32> = (1000..1100).collect();
        assert!(diff(&old, &new, 199).is_none());
        assert!(diff(&old, &new, 200).is_some());
        assert!(diff(&old, &[], 99).is_none());
    }
}
