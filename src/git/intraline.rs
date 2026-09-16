//! Intra-line diff: inside a deletion paired with an addition, which columns
//! actually changed (git.md §4). Pure text work — the diff view paints the
//! ranges, it never computes them.

use std::ops::Range;
use std::time::{Duration, Instant};

use crate::git::diff::{DiffFingerprint, FileDiff, Hunk, LineOrigin};

/// Columns of a displayed line, as char indices: the diff view lays rows out in a
/// monospace grid, so a column is a cell.
pub type Columns = Range<usize>;

/// Beyond this many tokens a line falls back to trimming its common prefix and
/// suffix: the token alignment is quadratic, and a minified line is the case that
/// would pay for it.
const MAX_TOKENS: usize = 256;

/// Share of the longer line's significant characters the two must have in common
/// for the pairing to be believable. Below it the rows are two unrelated lines
/// that happen to sit next to each other: highlighting nearly all of both says
/// less than the row backgrounds already do, so nothing is highlighted.
const MIN_COMMON_PERCENT: usize = 25;

/// Changed columns of the open diff, indexed by hunk then by line, filled
/// **incrementally**: aligning a pair of lines costs ~2 µs, so a 50 000-line diff
/// would spend ~50 ms of one frame on ranges the reader cannot see yet. `extend`
/// fills what a frame's budget allows and resumes on the next one; a pair not
/// reached yet reads as unchanged (`line` ⇒ empty), like a context line.
#[derive(Debug, Clone)]
pub struct IntralineChanges {
    fingerprint: DiffFingerprint,
    lines: Vec<Vec<Vec<Columns>>>,
    /// Where the fill stopped; `None` once every hunk is complete.
    pending: Option<Pending>,
}

/// Resumption point: the hunk being filled, its line pairs, and how many of them
/// are done.
#[derive(Debug, Clone)]
struct Pending {
    hunk: usize,
    pairs: Vec<(usize, usize)>,
    filled: usize,
}

impl IntralineChanges {
    /// Empty ranges for `diff`, everything left to `extend`. The fingerprint comes
    /// from the caller: one frame hashes the diff once for all its caches.
    pub fn new(diff: &FileDiff, fingerprint: DiffFingerprint) -> Self {
        Self {
            fingerprint,
            lines: diff
                .hunks
                .iter()
                .map(|hunk| vec![Vec::new(); hunk.lines.len()])
                .collect(),
            pending: diff.hunks.first().map(|hunk| Pending {
                hunk: 0,
                pairs: pairs(hunk),
                filled: 0,
            }),
        }
    }

    pub fn is_current(&self, fingerprint: DiffFingerprint) -> bool {
        self.fingerprint == fingerprint
    }

    /// Fills pairs for at most `budget`, resuming where the previous call stopped.
    /// `true` while pairs remain — the caller repaints to continue. `diff` must be
    /// the one the ranges were opened on (`is_current`).
    pub fn extend(&mut self, diff: &FileDiff, budget: Duration) -> bool {
        let started = Instant::now();
        let Self { lines, pending, .. } = self;
        while let Some(current) = pending.as_mut() {
            let Some(hunk) = diff.hunks.get(current.hunk) else {
                *pending = None;
                break;
            };
            while let Some(&(old, new)) = current.pairs.get(current.filled) {
                let (old_columns, new_columns) =
                    line_changes(&hunk.lines[old].content, &hunk.lines[new].content);
                lines[current.hunk][old] = old_columns;
                lines[current.hunk][new] = new_columns;
                current.filled += 1;
                if started.elapsed() >= budget {
                    return true;
                }
            }
            *pending = diff.hunks.get(current.hunk + 1).map(|next| Pending {
                hunk: current.hunk + 1,
                pairs: pairs(next),
                filled: 0,
            });
        }
        false
    }

    /// Columns that line differs from its counterpart on — empty for a context
    /// line, an unpaired one, or a pair not filled yet.
    pub fn line(&self, hunk: usize, line: usize) -> &[Columns] {
        self.lines
            .get(hunk)
            .and_then(|lines| lines.get(line))
            .map_or(&[], Vec::as_slice)
    }
}

/// Lines of `hunk` that answer each other, as `(deletion, addition)` indices.
/// Rewritten lines keep their order on both sides, so the n-th deletion of a block
/// answers its n-th addition; a block with more of one than the other leaves its
/// extra lines unpaired, as whole insertions or removals.
fn pairs(hunk: &Hunk) -> Vec<(usize, usize)> {
    let mut pairs = Vec::new();
    let mut idx = 0;
    while idx < hunk.lines.len() {
        if hunk.lines[idx].origin != LineOrigin::Deletion {
            idx += 1;
            continue;
        }
        let deletions = idx;
        while hunk.lines.get(idx).is_some_and(is_deletion) {
            idx += 1;
        }
        let additions = idx;
        while hunk.lines.get(idx).is_some_and(is_addition) {
            idx += 1;
        }
        for offset in 0..(additions - deletions).min(idx - additions) {
            pairs.push((deletions + offset, additions + offset));
        }
    }
    pairs
}

/// Columns that changed on each side of a rewritten line: `(old, new)`. Empty on
/// both sides when the two lines are too far apart to pair meaningfully.
fn line_changes(old: &str, new: &str) -> (Vec<Columns>, Vec<Columns>) {
    let (old, new) = (displayed(old), displayed(new));
    if old == new {
        return (Vec::new(), Vec::new());
    }
    let (old_tokens, new_tokens) = (tokens(old), tokens(new));
    if old_tokens.len() > MAX_TOKENS || new_tokens.len() > MAX_TOKENS {
        return affix_changes(old, new);
    }
    let common = common_tokens(&old_tokens, &new_tokens);
    if !believable(&old_tokens, &new_tokens, &common) {
        return (Vec::new(), Vec::new());
    }
    let (old_common, new_common): (Vec<_>, Vec<_>) = common.into_iter().unzip();
    (
        changed_columns(&old_tokens, &old_common),
        changed_columns(&new_tokens, &new_common),
    )
}

fn is_deletion(line: &crate::git::diff::DiffLine) -> bool {
    line.origin == LineOrigin::Deletion
}

fn is_addition(line: &crate::git::diff::DiffLine) -> bool {
    line.origin == LineOrigin::Addition
}

fn displayed(content: &str) -> &str {
    content.trim_end_matches(['\n', '\r'])
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    Word,
    Space,
    Symbol,
}

fn class(c: char) -> Class {
    if c.is_alphanumeric() || c == '_' {
        Class::Word
    } else if c.is_whitespace() {
        Class::Space
    } else {
        Class::Symbol
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Token<'a> {
    columns: Columns,
    text: &'a str,
}

/// Words and whitespace runs, every symbol on its own: a changed argument then
/// reads as one highlighted word, not as the whole call it sits in.
fn tokens(line: &str) -> Vec<Token<'_>> {
    let mut tokens = Vec::new();
    let mut open: Option<(usize, usize, Class)> = None;
    for (column, (byte, c)) in line.char_indices().enumerate() {
        let class = class(c);
        match open {
            Some((start_byte, start_column, previous))
                if previous != class || class == Class::Symbol =>
            {
                tokens.push(Token {
                    columns: start_column..column,
                    text: &line[start_byte..byte],
                });
                open = Some((byte, column, class));
            }
            Some(_) => {}
            None => open = Some((byte, column, class)),
        }
    }
    if let Some((start_byte, start_column, _)) = open {
        tokens.push(Token {
            columns: start_column..line.chars().count(),
            text: &line[start_byte..],
        });
    }
    tokens
}

/// Longest common subsequence of the two token streams, as index pairs.
fn common_tokens(old: &[Token<'_>], new: &[Token<'_>]) -> Vec<(usize, usize)> {
    let width = new.len() + 1;
    let mut lengths = vec![0_u32; (old.len() + 1) * width];
    for i in (0..old.len()).rev() {
        for j in (0..new.len()).rev() {
            lengths[i * width + j] = if old[i].text == new[j].text {
                lengths[(i + 1) * width + j + 1] + 1
            } else {
                lengths[(i + 1) * width + j].max(lengths[i * width + j + 1])
            };
        }
    }
    let mut common = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < old.len() && j < new.len() {
        if old[i].text == new[j].text {
            common.push((i, j));
            i += 1;
            j += 1;
        } else if lengths[(i + 1) * width + j] >= lengths[i * width + j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    common
}

fn believable(old: &[Token<'_>], new: &[Token<'_>], common: &[(usize, usize)]) -> bool {
    let shared: usize = common
        .iter()
        .map(|(i, _)| significant_chars(old[*i].text))
        .sum();
    let longest = significant_total(old).max(significant_total(new));
    shared * 100 >= longest * MIN_COMMON_PERCENT
}

/// Whitespace is left out of the similarity measure: two unrelated lines of the
/// same block share their indentation, and that is not a resemblance.
fn significant_chars(text: &str) -> usize {
    text.chars().filter(|c| !c.is_whitespace()).count()
}

fn significant_total(tokens: &[Token<'_>]) -> usize {
    tokens.iter().map(|t| significant_chars(t.text)).sum()
}

/// Columns of the tokens left out of `common`, adjacent ones merged into one range.
fn changed_columns(tokens: &[Token<'_>], common: &[usize]) -> Vec<Columns> {
    let mut changed = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        if common.contains(&index) {
            continue;
        }
        match changed.last_mut() {
            Some(Range { end, .. }) if *end == token.columns.start => *end = token.columns.end,
            _ => changed.push(token.columns.clone()),
        }
    }
    changed
}

/// What is left of each line once the common prefix and suffix are off: the
/// fallback for a line too long to align token by token.
fn affix_changes(old: &str, new: &str) -> (Vec<Columns>, Vec<Columns>) {
    let (old, new): (Vec<char>, Vec<char>) = (old.chars().collect(), new.chars().collect());
    let prefix = old
        .iter()
        .zip(&new)
        .take_while(|(a, b)| a == b)
        .count()
        .min(old.len().min(new.len()));
    let suffix = old[prefix..]
        .iter()
        .rev()
        .zip(new[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let range = |len: usize| {
        let end = len - suffix;
        let mut columns = Vec::new();
        if prefix < end {
            columns.push(prefix..end);
        }
        columns
    };
    (range(old.len()), range(new.len()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::diff::DiffLine;

    fn line(origin: LineOrigin, content: &str) -> DiffLine {
        DiffLine {
            origin,
            content: content.to_string(),
            old_lineno: None,
            new_lineno: None,
        }
    }

    /// A one-hunk diff, filled in full: what the diff view holds once the budget
    /// has had every frame it needs.
    fn changes(lines: Vec<DiffLine>) -> IntralineChanges {
        let diff = FileDiff {
            path: "src/main.rs".to_string(),
            binary: false,
            oversize: false,
            hunks: vec![Hunk {
                header: String::new(),
                old_start: 1,
                old_lines: 1,
                new_start: 1,
                new_lines: 1,
                lines,
            }],
            source_lines: Vec::new(),
            image: None,
            editable: false,
        };
        let mut changes = IntralineChanges::new(&diff, diff.fingerprint());
        assert!(!changes.extend(&diff, Duration::from_secs(1)));
        changes
    }

    /// The two rows of a single rewritten line, as the text they highlight.
    fn rewritten(old: &str, new: &str) -> (Vec<String>, Vec<String>) {
        let changes = changes(vec![
            line(LineOrigin::Deletion, old),
            line(LineOrigin::Addition, new),
        ]);
        (text(old, changes.line(0, 0)), text(new, changes.line(0, 1)))
    }

    fn text(line: &str, columns: &[Columns]) -> Vec<String> {
        let chars: Vec<char> = displayed(line).chars().collect();
        columns
            .iter()
            .map(|c| chars[c.clone()].iter().collect())
            .collect()
    }

    #[test]
    fn a_changed_word_is_the_only_range() {
        let (old, new) = rewritten("let total = price * 2;\n", "let total = price * 3;\n");
        assert_eq!(old, ["2"]);
        assert_eq!(new, ["3"]);
    }

    #[test]
    fn several_edits_in_one_line_are_several_ranges() {
        let (old, new) = rewritten("fn draw(ui: &Ui, x: u8)", "fn paint(ui: &Ui, x: u32)");
        assert_eq!(old, ["draw", "u8"]);
        assert_eq!(new, ["paint", "u32"]);
    }

    #[test]
    fn an_insertion_inside_a_line_marks_only_the_new_side() {
        let (old, new) = rewritten("call(a, b)", "call(a, extra, b)");
        assert!(old.is_empty(), "{old:?}");
        assert_eq!(new, ["extra, "]);
    }

    #[test]
    fn two_unrelated_lines_get_no_range() {
        let (old, new) = rewritten(
            "    let repository = open(path)?;",
            "    ui.painter().rect_filled(rect, radius, bg);",
        );
        assert!(old.is_empty(), "{old:?}");
        assert!(new.is_empty(), "{new:?}");
    }

    #[test]
    fn shared_indentation_alone_is_not_a_resemblance() {
        let (old, new) = rewritten("        alpha();", "        bravo(charlie, delta);");
        assert!(old.is_empty(), "{old:?}");
        assert!(new.is_empty(), "{new:?}");
    }

    #[test]
    fn a_line_beyond_the_token_cap_falls_back_to_its_affixes() {
        let filler = "a,".repeat(MAX_TOKENS);
        let (old, new) = rewritten(&format!("[{filler}zero]"), &format!("[{filler}one]"));
        assert_eq!(old, ["zero"]);
        assert_eq!(new, ["one"]);
    }

    #[test]
    fn a_hunk_pairs_its_deletions_with_its_additions_in_order() {
        let changes = changes(vec![
            line(LineOrigin::Context, "fn main() {\n"),
            line(LineOrigin::Deletion, "    let a = 1;\n"),
            line(LineOrigin::Deletion, "    let b = 2;\n"),
            line(LineOrigin::Addition, "    let a = 10;\n"),
            line(LineOrigin::Addition, "    let b = 20;\n"),
        ]);
        assert!(changes.line(0, 0).is_empty());
        assert_eq!(text("    let a = 1;", changes.line(0, 1)), ["1"]);
        assert_eq!(text("    let b = 2;", changes.line(0, 2)), ["2"]);
        assert_eq!(text("    let a = 10;", changes.line(0, 3)), ["10"]);
        assert_eq!(text("    let b = 20;", changes.line(0, 4)), ["20"]);
    }

    #[test]
    fn an_addition_with_no_deletion_facing_it_stays_unmarked() {
        let changes = changes(vec![
            line(LineOrigin::Deletion, "    let a = 1;\n"),
            line(LineOrigin::Addition, "    let a = 2;\n"),
            line(LineOrigin::Addition, "    let c = 3;\n"),
        ]);
        assert_eq!(text("    let a = 1;", changes.line(0, 0)), ["1"]);
        assert_eq!(text("    let a = 2;", changes.line(0, 1)), ["2"]);
        assert!(changes.line(0, 2).is_empty());
    }

    #[test]
    fn a_budget_that_runs_out_leaves_the_rest_for_the_next_frame() {
        let rewrite = |n: usize| {
            vec![
                line(LineOrigin::Deletion, &format!("    let value = {n};\n")),
                line(LineOrigin::Addition, &format!("    let value = {n}0;\n")),
            ]
        };
        let diff = FileDiff {
            path: "src/main.rs".to_string(),
            binary: false,
            oversize: false,
            hunks: (0..200)
                .map(|n| Hunk {
                    header: String::new(),
                    old_start: 1,
                    old_lines: 1,
                    new_start: 1,
                    new_lines: 1,
                    lines: rewrite(n),
                })
                .collect(),
            source_lines: Vec::new(),
            image: None,
            editable: false,
        };
        let mut changes = IntralineChanges::new(&diff, diff.fingerprint());
        assert!(changes.extend(&diff, Duration::ZERO));
        assert!(changes.line(199, 1).is_empty());
        while changes.extend(&diff, Duration::from_millis(1)) {}
        assert_eq!(
            text("    let value = 1990;", changes.line(199, 1)),
            ["1990"]
        );
    }

    #[test]
    fn a_diff_that_reloaded_with_another_content_is_not_current() {
        let one = |content: &str| FileDiff {
            path: "src/main.rs".to_string(),
            binary: false,
            oversize: false,
            hunks: vec![Hunk {
                header: String::new(),
                old_start: 1,
                old_lines: 1,
                new_start: 1,
                new_lines: 1,
                lines: vec![line(LineOrigin::Addition, content)],
            }],
            source_lines: Vec::new(),
            image: None,
            editable: false,
        };
        let diff = one("let a = 1;\n");
        let changes = IntralineChanges::new(&diff, diff.fingerprint());
        assert!(changes.is_current(one("let a = 1;\n").fingerprint()));
        assert!(!changes.is_current(one("let a = 2;\n").fingerprint()));
    }
}
