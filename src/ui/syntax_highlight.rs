use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::OnceLock;

use syntect::highlighting::{HighlightIterator, HighlightState, Highlighter};
use syntect::parsing::{ParseState, ScopeStack};

use crate::git::conflict::{ConflictFile, Region};
use crate::git::diff::{FileDiff, LineOrigin};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HighlightedSpan {
    pub text: String,
    pub color: egui::Color32,
}

/// Per-line spans of the open diff, filled **incrementally**: syntect costs
/// ~13 µs/line, so highlighting a 3 000-line file in one pass froze the frame
/// ~40 ms — one hitch per file when navigating with the arrow keys. `extend`
/// fills what a frame's budget allows and resumes on the next one; a line not
/// reached yet renders plain (`line` ⇒ `None`), like a file with no syntax.
#[derive(Debug, Clone)]
pub struct HighlightedDiffCache {
    key: HighlightKey,
    lines: Vec<Vec<Vec<HighlightedSpan>>>,
    /// Where the fill stopped; `None` once every hunk is complete.
    pending: Option<Pending>,
}

/// Resumption point: syntect's after-line state and the hunk it applies to —
/// the next line to fill is the one after those already in `lines[hunk]`.
#[derive(Debug, Clone)]
struct Pending {
    syntax: &'static syntect::parsing::SyntaxReference,
    state: LineState,
    hunk: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HighlightKey {
    path: String,
    syntax_theme: &'static str,
    fingerprint: u64,
}

impl HighlightedDiffCache {
    /// Empty cache for `diff`, everything left to `extend`. `None` when the
    /// file has no syntax to apply (binary, oversize, unknown extension).
    pub fn new(diff: &FileDiff, syntax_theme: &'static str) -> Option<Self> {
        if diff.binary || diff.oversize {
            return None;
        }
        let syntax = syntaxes()
            .find_syntax_for_file(Path::new(&diff.path))
            .ok()
            .flatten()?;
        let highlighter = Highlighter::new(theme(syntax_theme));
        Some(Self {
            key: HighlightKey::new(diff, syntax_theme),
            lines: vec![Vec::new(); diff.hunks.len()],
            pending: Some(Pending {
                syntax,
                state: LineState::new(syntax, &highlighter),
                hunk: 0,
            }),
        })
    }

    /// Fills lines for at most `budget`, resuming where the previous call
    /// stopped. `true` while lines remain — the caller repaints to continue.
    /// `diff` must be the one the cache was opened on (`is_current`).
    pub fn extend(&mut self, diff: &FileDiff, budget: std::time::Duration) -> bool {
        let Self {
            lines,
            pending,
            key,
        } = self;
        let Some(current) = pending.as_mut() else {
            return false;
        };
        let highlighter = Highlighter::new(theme(key.syntax_theme));
        let start = std::time::Instant::now();
        while let Some(hunk) = diff.hunks.get(current.hunk) {
            let Some(line) = hunk.lines.get(lines[current.hunk].len()) else {
                current.hunk += 1;
                // A hunk shows a disjoint slice of the file: it restarts from a
                // clean state instead of continuing the previous hunk's context.
                current.state = LineState::new(current.syntax, &highlighter);
                continue;
            };
            let text = display_text(&line.content);
            let Ok(ops) = current.state.parse.parse_line(text, syntaxes()) else {
                *pending = None;
                return false;
            };
            lines[current.hunk].push(
                HighlightIterator::new(&mut current.state.highlight, &ops, text, &highlighter)
                    .filter(|(_, t)| !t.is_empty())
                    .map(|(style, t)| HighlightedSpan {
                        text: t.to_owned(),
                        color: syntect_color(style.foreground),
                    })
                    .collect(),
            );
            // Checked per line: a line costs microseconds and `Instant::now()`
            // nanoseconds, and batching the check overran the budget by 10× on
            // an unoptimised build, where a line is ~20 times slower.
            if start.elapsed() >= budget {
                return true;
            }
        }
        *pending = None;
        false
    }

    /// Cache filled in one pass — the shape the tests and the small diffs use.
    pub fn for_diff(diff: &FileDiff, syntax_theme: &'static str) -> Option<Self> {
        let mut cache = Self::new(diff, syntax_theme)?;
        cache.extend(diff, std::time::Duration::MAX);
        Some(cache)
    }

    pub fn is_current(&self, diff: &FileDiff, syntax_theme: &'static str) -> bool {
        self.key == HighlightKey::new(diff, syntax_theme)
    }

    pub fn line(&self, hunk: usize, line: usize) -> Option<&[HighlightedSpan]> {
        self.lines
            .get(hunk)
            .and_then(|lines| lines.get(line))
            .map(Vec::as_slice)
    }
}

impl HighlightKey {
    fn new(diff: &FileDiff, syntax_theme: &'static str) -> Self {
        Self {
            path: diff.path.clone(),
            syntax_theme,
            fingerprint: fingerprint(diff),
        }
    }
}

/// Syntax highlighting of a single conflicted file (conflicts.md §5), the editor's
/// counterpart of [`HighlightedDiffCache`]: each side of every region is highlighted
/// once into spans, indexed by region then line — the culled rows just index in,
/// never re-highlight. `None` when the path has no known syntax (plain fallback).
#[derive(Debug, Clone)]
pub struct ConflictHighlight {
    key: ConflictHighlightKey,
    regions: Vec<RegionSpans>,
}

/// Highlighted lines of one region, aligned to `ConflictFile.regions`: a `Stable`
/// region fills `stable`; a `Conflict` region fills `ours` / `theirs` / `base`.
#[derive(Debug, Clone, Default)]
pub struct RegionSpans {
    pub stable: Vec<Vec<HighlightedSpan>>,
    pub ours: Vec<Vec<HighlightedSpan>>,
    pub theirs: Vec<Vec<HighlightedSpan>>,
    pub base: Vec<Vec<HighlightedSpan>>,
}

#[derive(Debug, Clone)]
struct ConflictHighlightKey {
    path: String,
    syntax_theme: &'static str,
}

impl ConflictHighlight {
    pub fn for_file(file: &ConflictFile, syntax_theme: &'static str) -> Option<Self> {
        let syntax = syntaxes()
            .find_syntax_for_file(Path::new(&file.path))
            .ok()
            .flatten()?;
        let theme = theme(syntax_theme);
        let mut regions = Vec::with_capacity(file.regions.len());
        for region in &file.regions {
            regions.push(match region {
                Region::Stable(lines) => RegionSpans {
                    stable: highlight_block(syntax, theme, lines)?,
                    ..Default::default()
                },
                Region::Conflict { ours, theirs, base } => RegionSpans {
                    stable: Vec::new(),
                    ours: highlight_block(syntax, theme, ours)?,
                    theirs: highlight_block(syntax, theme, theirs)?,
                    base: highlight_block(syntax, theme, base)?,
                },
            });
        }
        Some(Self {
            key: ConflictHighlightKey::new(file, syntax_theme),
            regions,
        })
    }

    pub fn is_current(&self, file: &ConflictFile, syntax_theme: &'static str) -> bool {
        self.key.path == file.path && self.key.syntax_theme == syntax_theme
    }

    pub fn region(&self, index: usize) -> Option<&RegionSpans> {
        self.regions.get(index)
    }
}

impl ConflictHighlightKey {
    fn new(file: &ConflictFile, syntax_theme: &'static str) -> Self {
        Self {
            path: file.path.clone(),
            syntax_theme,
        }
    }
}

/// Highlights free buffer text (the editable Output, conflicts.md §5) into per-line
/// spans, splitting on `\n` like the region/diff paths. `None` when the path has no
/// known syntax — the caller renders the buffer flat.
pub fn highlight_buffer(
    path: &str,
    syntax_theme: &'static str,
    text: &str,
) -> Option<Vec<Vec<HighlightedSpan>>> {
    let syntax = syntaxes()
        .find_syntax_for_file(Path::new(path))
        .ok()
        .flatten()?;
    let theme = theme(syntax_theme);
    let mut highlighter = syntect::easy::HighlightLines::new(syntax, theme);
    let mut out = Vec::new();
    for line in text.split('\n') {
        out.push(highlight_line(&mut highlighter, line)?);
    }
    Some(out)
}

/// syntect's after-line state, cached per buffer line so an edit re-parses only what it
/// touched. Comparing it tells the incremental pass when a recolour has reconverged.
#[derive(Debug, Clone, PartialEq)]
struct LineState {
    parse: ParseState,
    highlight: HighlightState,
}

impl LineState {
    fn new(syntax: &syntect::parsing::SyntaxReference, highlighter: &Highlighter) -> Self {
        Self {
            parse: ParseState::new(syntax),
            highlight: HighlightState::new(highlighter, ScopeStack::new()),
        }
    }
}

/// Incremental syntax highlighter for the editable Output (conflicts.md §5). syntect is
/// not incremental, so re-highlighting the whole buffer on every keystroke froze large
/// files; this keeps per-line spans + syntect state and, on each call, re-parses only the
/// run of lines an edit changed — from the first differing line until the parse state
/// reconverges with the cached run, splicing the untouched prefix/suffix back in. Output
/// is identical to [`highlight_buffer`] (verified in tests), but a keystroke costs roughly
/// one line instead of the whole file, so it can run every frame with no flicker.
#[derive(Clone, Default)]
pub struct IncrementalHighlighter {
    path: String,
    syntax_theme: &'static str,
    available: bool,
    texts: Vec<String>,
    states: Vec<LineState>,
    spans: Vec<Vec<HighlightedSpan>>,
}

impl IncrementalHighlighter {
    /// Returns per-line spans for `text`, re-parsing only the changed lines. `None` when
    /// the path has no known syntax (the caller renders the buffer flat).
    pub fn highlight(
        &mut self,
        path: &str,
        syntax_theme: &'static str,
        text: &str,
    ) -> Option<&[Vec<HighlightedSpan>]> {
        let Some(syntax) = syntaxes()
            .find_syntax_for_file(Path::new(path))
            .ok()
            .flatten()
        else {
            self.reset(path, syntax_theme);
            return None;
        };
        let highlighter = Highlighter::new(theme(syntax_theme));

        if self.path != path || self.syntax_theme != syntax_theme {
            self.reset(path, syntax_theme);
        }
        let make_initial = || LineState {
            parse: ParseState::new(syntax),
            highlight: HighlightState::new(&highlighter, ScopeStack::new()),
        };

        let new: Vec<&str> = text.split('\n').collect();
        let old_len = self.texts.len();

        let mut prefix = 0;
        while prefix < old_len && prefix < new.len() && self.texts[prefix] == new[prefix] {
            prefix += 1;
        }
        let max_suffix = old_len.min(new.len()) - prefix;
        let mut suffix = 0;
        while suffix < max_suffix && self.texts[old_len - 1 - suffix] == new[new.len() - 1 - suffix]
        {
            suffix += 1;
        }

        let suffix_start = new.len() - suffix;
        // old line i corresponds to new line i - delta inside the shared suffix.
        let delta = old_len as isize - new.len() as isize;

        let mut state = if prefix == 0 {
            make_initial()
        } else {
            self.states[prefix - 1].clone()
        };
        let mut new_texts = Vec::new();
        let mut new_states = Vec::new();
        let mut new_spans = Vec::new();
        let mut old_end = old_len;
        let mut j = prefix;
        while j < new.len() {
            if j >= suffix_start {
                let old_j = (j as isize + delta) as usize;
                let entering_old = if old_j == 0 {
                    make_initial()
                } else {
                    self.states[old_j - 1].clone()
                };
                if state == entering_old {
                    old_end = old_j;
                    break;
                }
            }
            let line = new[j];
            let Ok(ops) = state.parse.parse_line(line, syntaxes()) else {
                self.reset(path, syntax_theme);
                return None;
            };
            let line_spans = HighlightIterator::new(&mut state.highlight, &ops, line, &highlighter)
                .filter(|(_, t)| !t.is_empty())
                .map(|(style, t)| HighlightedSpan {
                    text: t.to_owned(),
                    color: syntect_color(style.foreground),
                })
                .collect();
            new_texts.push(line.to_owned());
            new_states.push(state.clone());
            new_spans.push(line_spans);
            j += 1;
        }

        self.texts.splice(prefix..old_end, new_texts);
        self.states.splice(prefix..old_end, new_states);
        self.spans.splice(prefix..old_end, new_spans);
        self.available = true;
        Some(&self.spans)
    }

    fn reset(&mut self, path: &str, syntax_theme: &'static str) {
        self.path = path.to_owned();
        self.syntax_theme = syntax_theme;
        self.available = false;
        self.texts.clear();
        self.states.clear();
        self.spans.clear();
    }
}

/// Highlights one side's lines as a continuous block (a fresh `HighlightLines`, like
/// a diff hunk). The lines carry no trailing newline — the diff path strips it too.
fn highlight_block(
    syntax: &syntect::parsing::SyntaxReference,
    theme: &syntect::highlighting::Theme,
    lines: &[String],
) -> Option<Vec<Vec<HighlightedSpan>>> {
    let mut highlighter = syntect::easy::HighlightLines::new(syntax, theme);
    let mut out = Vec::with_capacity(lines.len());
    for line in lines {
        out.push(highlight_line(&mut highlighter, line)?);
    }
    Some(out)
}

fn highlight_line(
    highlighter: &mut syntect::easy::HighlightLines<'_>,
    line: &str,
) -> Option<Vec<HighlightedSpan>> {
    highlighter
        .highlight_line(line, syntaxes())
        .ok()
        .map(|ranges| {
            ranges
                .into_iter()
                .filter(|(_, text)| !text.is_empty())
                .map(|(style, text)| HighlightedSpan {
                    text: text.to_owned(),
                    color: syntect_color(style.foreground),
                })
                .collect()
        })
}

pub fn display_text(content: &str) -> &str {
    content.trim_end_matches(['\n', '\r'])
}

fn fingerprint(diff: &FileDiff) -> u64 {
    let mut hasher = DefaultHasher::new();
    diff.path.hash(&mut hasher);
    diff.binary.hash(&mut hasher);
    diff.oversize.hash(&mut hasher);
    for hunk in &diff.hunks {
        hunk.header.hash(&mut hasher);
        hunk.old_start.hash(&mut hasher);
        hunk.old_lines.hash(&mut hasher);
        hunk.new_start.hash(&mut hasher);
        hunk.new_lines.hash(&mut hasher);
        for line in &hunk.lines {
            line_origin_key(line.origin).hash(&mut hasher);
            line.content.hash(&mut hasher);
            line.old_lineno.hash(&mut hasher);
            line.new_lineno.hash(&mut hasher);
        }
    }
    hasher.finish()
}

fn line_origin_key(origin: LineOrigin) -> u8 {
    match origin {
        LineOrigin::Context => 0,
        LineOrigin::Addition => 1,
        LineOrigin::Deletion => 2,
    }
}

fn syntect_color(color: syntect::highlighting::Color) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(color.r, color.g, color.b, color.a)
}

fn syntaxes() -> &'static syntect::parsing::SyntaxSet {
    static SYNTAXES: OnceLock<syntect::parsing::SyntaxSet> = OnceLock::new();
    SYNTAXES.get_or_init(two_face::syntax::extra_newlines)
}

fn themes() -> &'static syntect::highlighting::ThemeSet {
    static THEMES: OnceLock<syntect::highlighting::ThemeSet> = OnceLock::new();
    // two-face set (bat themes): covers the presets' `Palette::syntax` names,
    // including Catppuccin Latte/Mocha and OneHalf exactly.
    THEMES.get_or_init(|| syntect::highlighting::ThemeSet::from(two_face::theme::extra()))
}

fn theme(name: &str) -> &'static syntect::highlighting::Theme {
    themes()
        .themes
        .get(name)
        .or_else(|| themes().themes.values().next())
        .expect("two-face embeds at least one theme")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::diff::{DiffLine, Hunk};

    fn diff(path: &str, content: &str) -> FileDiff {
        FileDiff {
            path: path.to_owned(),
            binary: false,
            oversize: false,
            hunks: vec![Hunk {
                header: "@@ -1 +1 @@".to_owned(),
                old_start: 1,
                old_lines: 1,
                new_start: 1,
                new_lines: 1,
                lines: vec![DiffLine {
                    origin: LineOrigin::Context,
                    content: content.to_owned(),
                    old_lineno: Some(1),
                    new_lineno: Some(1),
                }],
            }],
            source_lines: Vec::new(),
            image: None,
            editable: false,
        }
    }

    /// Two hunks of 100 lines: more than one fill batch, and a second hunk to
    /// restart the syntect state on.
    fn long_diff(path: &str) -> FileDiff {
        let hunk = |start: u32, body: &str| Hunk {
            header: format!("@@ -{start} +{start} @@"),
            old_start: start,
            old_lines: 100,
            new_start: start,
            new_lines: 100,
            lines: (0..100)
                .map(|i| DiffLine {
                    origin: LineOrigin::Addition,
                    content: format!("{body} // {i}\n"),
                    old_lineno: None,
                    new_lineno: Some(start + i),
                })
                .collect(),
        };
        FileDiff {
            path: path.to_owned(),
            binary: false,
            oversize: false,
            hunks: vec![hunk(1, "let a = \"x\";"), hunk(500, "fn f() {}")],
            source_lines: Vec::new(),
            image: None,
            editable: false,
        }
    }

    #[test]
    fn a_budgeted_fill_converges_to_the_one_pass_result() {
        let diff = long_diff("src/main.rs");
        let full = HighlightedDiffCache::for_diff(&diff, "InspiredGitHub").unwrap();

        let mut cache = HighlightedDiffCache::new(&diff, "InspiredGitHub").unwrap();
        assert!(
            cache.line(0, 0).is_none(),
            "before the first fill every line renders plain"
        );

        let mut passes = 0;
        // A zero budget still fills one line per call: the fill always progresses.
        while cache.extend(&diff, std::time::Duration::ZERO) {
            passes += 1;
            assert!(passes < 1000, "the fill never completes");
        }
        assert!(passes > 1, "200 lines span several passes, got {passes}");
        assert_eq!(
            cache.lines, full.lines,
            "resuming between batches colours exactly like one pass"
        );
    }

    #[test]
    fn rust_extension_builds_a_highlight_cache() {
        let cache = HighlightedDiffCache::for_diff(
            &diff("src/main.rs", "fn main() {}\n"),
            "InspiredGitHub",
        )
        .expect("Rust files are supported by the two-face syntax set");
        let spans = cache.line(0, 0).unwrap();

        assert_eq!(
            spans
                .iter()
                .map(|span| span.text.as_str())
                .collect::<String>(),
            "fn main() {}"
        );
        assert!(!spans.is_empty());
    }

    #[test]
    fn common_project_extensions_build_a_highlight_cache() {
        for path in [
            "app.ts",
            "component.tsx",
            "Cargo.toml",
            "Dockerfile",
            "component.vue",
            "component.svelte",
        ] {
            let cache =
                HighlightedDiffCache::for_diff(&diff(path, "let value = 1\n"), "InspiredGitHub");
            assert!(cache.is_some(), "{path} should be supported");
        }
    }

    #[test]
    fn unknown_extension_uses_plain_rendering() {
        let cache = HighlightedDiffCache::for_diff(
            &diff("file.unknownsyntax", "plain\n"),
            "InspiredGitHub",
        );

        assert!(cache.is_none());
    }

    #[test]
    fn cache_key_changes_when_content_or_theme_changes() {
        let original = diff("src/main.rs", "fn main() {}\n");
        let changed = diff("src/main.rs", "fn changed() {}\n");
        let cache = HighlightedDiffCache::for_diff(&original, "InspiredGitHub").unwrap();

        assert!(cache.is_current(&original, "InspiredGitHub"));
        assert!(!cache.is_current(&changed, "InspiredGitHub"));
        assert!(
            !cache.is_current(&original, "Catppuccin Mocha"),
            "changing the theme must invalidate the cache"
        );
    }

    #[test]
    fn every_preset_syntax_theme_resolves_in_the_two_face_set() {
        for preset in &crate::theme::PRESETS {
            assert!(
                themes().themes.contains_key(preset.palette.syntax),
                "{}: syntect theme \"{}\" missing from the two-face set",
                preset.name,
                preset.palette.syntax
            );
        }
    }

    fn conflict(path: &str) -> ConflictFile {
        ConflictFile {
            path: path.to_owned(),
            kind: crate::git::conflict::ConflictKind::BothModified,
            ours_label: "Current · ours".to_owned(),
            theirs_label: "Incoming · theirs".to_owned(),
            regions: vec![
                Region::Stable(vec!["fn run() {".to_owned()]),
                Region::Conflict {
                    ours: vec!["    let x = 1;".to_owned()],
                    theirs: vec!["    let x = 2;".to_owned()],
                    base: vec!["    let x = 0;".to_owned()],
                },
                Region::Stable(vec!["}".to_owned()]),
            ],
            has_base: true,
            eol: crate::git::conflict::LineEnding::default(),
            disk_divergence: None,
        }
    }

    #[test]
    fn conflict_file_highlights_each_side_per_region() {
        let file = conflict("src/main.rs");
        let cache = ConflictHighlight::for_file(&file, "InspiredGitHub")
            .expect("Rust files are highlightable");

        let conflict_region = cache.region(1).expect("the conflict region");
        let ours: String = conflict_region.ours[0]
            .iter()
            .map(|span| span.text.as_str())
            .collect();
        assert_eq!(ours, "    let x = 1;");
        let theirs: String = conflict_region.theirs[0]
            .iter()
            .map(|span| span.text.as_str())
            .collect();
        assert_eq!(theirs, "    let x = 2;");
        assert!(!conflict_region.base[0].is_empty());
        // a Stable region fills `stable`, not the conflict sides.
        assert_eq!(cache.region(0).unwrap().stable.len(), 1);
    }

    #[test]
    fn conflict_highlight_falls_back_for_unknown_syntax() {
        assert!(
            ConflictHighlight::for_file(&conflict("notes.unknownsyntax"), "InspiredGitHub")
                .is_none()
        );
    }

    #[test]
    fn conflict_highlight_key_tracks_path_and_theme() {
        let file = conflict("src/main.rs");
        let cache = ConflictHighlight::for_file(&file, "InspiredGitHub").unwrap();

        assert!(cache.is_current(&file, "InspiredGitHub"));
        // A content change reaches the editor only through a fresh `adopt`, which
        // drops the cache (`syntax = None`); so the key stays path + theme and
        // never hashes the file each frame. Same path, changed content → current.
        let mut same_path = file.clone();
        same_path.regions[0] = Region::Stable(vec!["fn changed() {".to_owned()]);
        assert!(cache.is_current(&same_path, "InspiredGitHub"));
        // A rail switch (different path) or a theme change invalidates.
        assert!(!cache.is_current(&conflict("src/other.rs"), "InspiredGitHub"));
        assert!(!cache.is_current(&file, "Catppuccin Mocha"));
    }

    #[test]
    fn incremental_highlight_matches_full_across_edits() {
        let path = "src/main.rs";
        let theme = "InspiredGitHub";
        let mut inc = IncrementalHighlighter::default();
        let versions = [
            "fn main() {\n    let x = 1;\n    println!(\"{x}\");\n}\n",
            // edit a line in place (same line count)
            "fn main() {\n    let x = 42;\n    println!(\"{x}\");\n}\n",
            // insert a line
            "fn main() {\n    let x = 42;\n    let y = x + 1;\n    println!(\"{y}\");\n}\n",
            // delete lines
            "fn main() {\n    println!(\"hi\");\n}\n",
            // open a block comment that cascades over the lines below
            "fn main() {\n    /* start\n    let x = 1;\n    println!(\"{x}\");\n}\n",
            // close it again
            "fn main() {\n    let x = 1;\n    println!(\"{x}\");\n}\n",
            // empty buffer, then back to content
            "",
            "fn main() {}\n",
        ];
        for version in versions {
            let full = highlight_buffer(path, theme, version).expect("rust is highlightable");
            let got = inc
                .highlight(path, theme, version)
                .expect("rust is highlightable");
            assert_eq!(got, full.as_slice(), "incremental mismatch for:\n{version}");
        }
    }

    #[test]
    fn incremental_highlight_resets_on_path_change() {
        let theme = "InspiredGitHub";
        let mut inc = IncrementalHighlighter::default();
        let _ = inc.highlight("a.rs", theme, "fn a() {}\n");
        // switching language must not reuse the Rust line cache.
        let python = "def b():\n    return 1\n";
        let full = highlight_buffer("b.py", theme, python).unwrap();
        let got = inc.highlight("b.py", theme, python).unwrap();
        assert_eq!(got, full.as_slice());
    }

    #[test]
    fn incremental_highlight_is_plain_for_unknown_syntax() {
        let mut inc = IncrementalHighlighter::default();
        assert!(inc
            .highlight("notes.unknownsyntax", "InspiredGitHub", "plain\n")
            .is_none());
    }
}
