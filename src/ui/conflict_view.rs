//! The in-app conflict editor (conflicts.md §3–§7): two checkbox panes (A = ours,
//! B = theirs) over an editable Output. Pure rendering: the
//! resolution choices live in [`ConflictEditorState`] (session, never domain) and
//! the view emits a [`ConflictEditorAction`] the app arbitrates — same separation
//! as `rebase_view` / `DiffViewState`.

use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use crate::git::conflict::{ConflictFile, ConflictKind, Region};
use crate::theme::Palette;
use crate::ui::syntax_highlight::{ConflictHighlight, HighlightedSpan, IncrementalHighlighter};

const PAD: f32 = 12.0;
/// Soft fill alpha of the band behind the *focused* conflict region.
const BAND_ALPHA_ACTIVE: u8 = 40;
/// Soft fill alpha of the band behind the other (dimmed) conflict regions, so the
/// focused one reads as the current target across the three zones (conflicts.md §3).
const BAND_ALPHA_DIM: u8 = 14;
/// Monospace size of a code line — matches the diff view's `LINE_SIZE`.
const CODE_SIZE: f32 = 12.0;
/// Width of the accent bar on a conflict band's left edge.
const BAR_W: f32 = 3.0;
/// Width of the take-checkbox cell reserved on every code row so lines align.
const CHECK_W: f32 = 22.0;
/// Width of the line-number gutter cell.
const GUTTER_W: f32 = 40.0;
/// Fixed height of a code row: uniform so every row can be reserved (correct
/// scroll geometry) yet only the on-screen ones build widgets (smooth scroll on
/// large files), and so a region's scrollbar position is exact.
const ROW_H: f32 = 18.0;
/// Size of the per-region tick painted on a scroll area's scrollbar track.
const MARK_W: f32 = 12.0;
const MARK_H: f32 = 4.0;
/// Placeholder line a still-unresolved region contributes to the Output buffer: a
/// single orange-banded row (not the conflict body) prompting a pick (conflicts.md §5).
const UNRESOLVED_LINE: &str = "‹ unresolved — pick A or B above ›";
/// Above this buffer size the editable Output stops syntax-highlighting: syntect would
/// re-run over the whole buffer on **every keystroke** (it is not incremental) and
/// freeze the UI on big files (lock files, generated/minified blobs). The text stays
/// fully editable as plain monospace, and the A|B panes keep their colours (highlighted
/// once at load, not per keystroke).
const MAX_LIVE_HIGHLIGHT_BYTES: usize = 64 * 1024;

/// Per-region resolution choice (rendering state). The A/B take checkboxes drive
/// it; the Output composes from it (conflicts.md §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionChoice {
    /// No side picked yet — orange placeholder, counts in "conflicts left".
    Unresolved,
    /// Take the *ours* side (stage 2 / A).
    Ours,
    /// Take the *theirs* side (stage 3 / B).
    Theirs,
    /// Take both; `ours_first` follows the order the boxes were ticked.
    Both { ours_first: bool },
    /// The hand-edited buffer.
    Manual,
}

/// Where a composed Output line comes from — drives its colour and the region band.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provenance {
    Context,
    Ours,
    Theirs,
    Both,
    Edited,
    Unresolved,
}

/// Which highlighted side + row an Output line maps to, so the pane syntax cache
/// can colour the composed file too (`None` for context / placeholder / hand edit).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowSource {
    None,
    Ours(usize),
    Theirs(usize),
}

/// One composed Output line with its provenance (painting + Save). Borrows the
/// source line so the per-frame compose allocates no strings (only owned at Save).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposedRow<'a> {
    pub text: &'a str,
    pub provenance: Provenance,
    pub source: RowSource,
}

/// The resolution a Save / card button asks the app to perform — maps 1:1 onto a
/// worker command (conflicts.md §8): `Compose`/`Delete` → `ResolveFile { path,
/// content }`, `Keep` → `Stage(path)` (the surviving file is already in the
/// working tree).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveRequest {
    Compose {
        path: String,
        content: String,
    },
    Keep {
        path: String,
    },
    Delete {
        path: String,
    },
    /// Take one whole side of a file the inline editor can't compose (binary /
    /// oversize): the worker reads that side's blob from the index.
    UseSide {
        path: String,
        ours: bool,
    },
}

/// Signals emitted within a frame, consumed by `HelmApp`.
#[derive(Default)]
pub struct ConflictEditorAction {
    /// Leave the editor (Close with nothing unsaved, or confirmed discard).
    pub close: bool,
    /// Resolve the current file.
    pub resolve: Option<ResolveRequest>,
}

/// Per-file rendering state, parallel to [`ConflictEditorState::files`].
#[derive(Debug, Clone, Default)]
struct FileResolution {
    /// One entry per `Conflict` region of the file.
    choices: Vec<RegionChoice>,
    /// Hand-edit buffer per conflict region (used when the choice is `Manual`).
    manual: Vec<String>,
    /// Saved into the index — the rail chip turns `✓`.
    saved: bool,
    /// Touched since the last open/save — drives the unsaved-close warning.
    dirty: bool,
    /// Disk file diverges from a clean reconstruction (read with the rail): the
    /// content read off disk, offered as *Load my version* (conflicts.md §5).
    disk_divergence: Option<String>,
    /// The editable Output buffer (conflicts.md §5): seeded from the composition on
    /// first render and recomposed whenever a pick changes; Save writes it verbatim.
    whole_manual: Option<String>,
    /// The buffer is a whole-file override (*Load my version*): saveable regardless of
    /// the per-region picks. Cleared as soon as a pick recomposes the Output.
    whole_override: bool,
    /// The editable buffer still mirrors the picks' composition (no hand edit since the
    /// last recompose): the per-region line map is exact, so the gutter bands and the
    /// scrollbar conflict marks are drawn from it. A hand edit clears it (a freely
    /// edited buffer no longer maps line-for-line onto the regions).
    composed: bool,
    /// Conflict region the nav (▲/▼) last jumped to — drives the "i/n" readout.
    active: usize,
    /// A nav jump requested this frame: scroll the matching region into view.
    scroll_to: Option<usize>,
}

impl FileResolution {
    fn new(file: &ConflictFile) -> Self {
        let n = conflict_count(file);
        FileResolution {
            choices: vec![RegionChoice::Unresolved; n],
            manual: vec![String::new(); n],
            disk_divergence: file.disk_divergence.clone(),
            ..Default::default()
        }
    }
}

/// Session state of the conflict editor (conflicts.md §8). Owned by `HelmApp`
/// like `rebase_page`; reset on repo switch / Close.
#[derive(Debug, Clone, Default)]
pub struct ConflictEditorState {
    pub files: Vec<ConflictFile>,
    pub file_index: usize,
    resolutions: Vec<FileResolution>,
    /// A Close was requested with unsaved work — the discard warning is showing.
    confirm_close: bool,
    /// Opened, awaiting the worker's `ReadConflicts` reply (empty `files`): only a
    /// loading editor adopts a reply, so a stale one after Close is ignored.
    loading: bool,
    /// File the rail should select once loaded (a conflicted-row click); `None`
    /// from the banner's Resolve, which lands on the first file.
    focus: Option<String>,
    /// Syntax highlighting of the *current* file (conflicts.md §5), rebuilt on file
    /// switch / theme change (`is_current`); `None` when the path has no syntax.
    syntax: Option<ConflictHighlight>,
}

impl ConflictEditorState {
    pub fn new(files: Vec<ConflictFile>) -> Self {
        let resolutions = files.iter().map(FileResolution::new).collect();
        ConflictEditorState {
            files,
            file_index: 0,
            resolutions,
            confirm_close: false,
            loading: false,
            focus: None,
            syntax: None,
        }
    }

    /// Editor opened from a conflicted row before the rail is read:
    /// empty and loading, the worker's `ReadConflicts` reply fills it.
    pub fn opening(focus: Option<String>) -> Self {
        ConflictEditorState {
            loading: true,
            focus,
            ..Default::default()
        }
    }

    /// Adopts the worker's `ReadConflicts` reply: builds the rail, selecting the
    /// focused file when one was requested (else the first).
    pub fn adopt(&mut self, files: Vec<ConflictFile>) {
        self.resolutions = files.iter().map(FileResolution::new).collect();
        self.file_index = self
            .focus
            .as_ref()
            .and_then(|focus| files.iter().position(|file| &file.path == focus))
            .unwrap_or(0);
        self.files = files;
        self.loading = false;
        self.syntax = None;
    }

    /// Rebuilds the current file's syntax highlighting when the file or theme
    /// changed (`is_current`); a no-op while it is still valid.
    fn ensure_syntax(&mut self, syntax_theme: &'static str) {
        if self.files.is_empty() {
            return;
        }
        let idx = self.file_index.min(self.files.len() - 1);
        let current = self
            .syntax
            .as_ref()
            .is_some_and(|cache| cache.is_current(&self.files[idx], syntax_theme));
        if !current {
            self.syntax = ConflictHighlight::for_file(&self.files[idx], syntax_theme);
        }
    }

    /// `true` while the editor still holds no files and expects a reply.
    pub fn loading(&self) -> bool {
        self.loading
    }

    /// A resolve was sent: re-enter loading so the next `ReadConflicts` reply
    /// rebuilds the rail (or closes the editor once the last conflict is gone).
    /// The current files keep rendering until then; the rail lands on the first
    /// remaining conflict.
    pub fn reload(&mut self) {
        self.loading = true;
        self.focus = None;
    }

    fn has_unsaved(&self) -> bool {
        self.resolutions.iter().any(|res| res.dirty)
    }
}

fn conflict_count(file: &ConflictFile) -> usize {
    file.regions
        .iter()
        .filter(|region| matches!(region, Region::Conflict { .. }))
        .count()
}

fn region_lines<'a>(
    ours: &'a [String],
    theirs: &'a [String],
    choice: RegionChoice,
    manual: &'a str,
) -> Vec<ComposedRow<'a>> {
    let mut out = Vec::new();
    let mut push = |lines: &'a [String], provenance: Provenance, ours_side: bool| {
        for (i, line) in lines.iter().enumerate() {
            out.push(ComposedRow {
                text: line.as_str(),
                provenance,
                source: if ours_side {
                    RowSource::Ours(i)
                } else {
                    RowSource::Theirs(i)
                },
            });
        }
    };
    match choice {
        RegionChoice::Unresolved => out.push(ComposedRow {
            text: UNRESOLVED_LINE,
            provenance: Provenance::Unresolved,
            source: RowSource::None,
        }),
        RegionChoice::Ours => push(ours, Provenance::Ours, true),
        RegionChoice::Theirs => push(theirs, Provenance::Theirs, false),
        RegionChoice::Both { ours_first } => {
            if ours_first {
                push(ours, Provenance::Both, true);
                push(theirs, Provenance::Both, false);
            } else {
                push(theirs, Provenance::Both, false);
                push(ours, Provenance::Both, true);
            }
        }
        RegionChoice::Manual => {
            for line in manual.split('\n') {
                out.push(ComposedRow {
                    text: line,
                    provenance: Provenance::Edited,
                    source: RowSource::None,
                });
            }
        }
    }
    out
}

/// The full composed Output, region by region (painting + Save).
fn compose_rows<'a>(file: &'a ConflictFile, res: &'a FileResolution) -> Vec<ComposedRow<'a>> {
    let mut rows = Vec::new();
    let mut ci = 0;
    for region in &file.regions {
        match region {
            Region::Stable(lines) => {
                for line in lines {
                    rows.push(ComposedRow {
                        text: line.as_str(),
                        provenance: Provenance::Context,
                        source: RowSource::None,
                    });
                }
            }
            Region::Conflict { ours, theirs, .. } => {
                rows.extend(region_lines(ours, theirs, res.choices[ci], &res.manual[ci]));
                ci += 1;
            }
        }
    }
    rows
}

fn file_resolved(res: &FileResolution) -> bool {
    res.whole_override || res.choices.iter().all(|c| *c != RegionChoice::Unresolved)
}

/// The Output text composed from the current picks, region by region. Always LF —
/// the editable buffer stays newline-normalised, `compose_content` re-applies the
/// file's terminator on Save. The final newline follows the original file's.
fn compose_string(file: &ConflictFile, res: &FileResolution) -> String {
    let mut content: String = compose_rows(file, res)
        .into_iter()
        .map(|row| row.text)
        .collect::<Vec<_>>()
        .join("\n");
    if file.eol.final_newline {
        content.push('\n');
    }
    content
}

/// Recompose the editable Output from the current picks (a pick changed): reseeds the
/// buffer, drops any whole-file override, marks it composed (the line map is exact
/// again) and marks the file dirty.
fn recompose_output(file: &ConflictFile, res: &mut FileResolution) {
    res.whole_manual = Some(compose_string(file, res));
    res.whole_override = false;
    res.composed = true;
    res.dirty = true;
}

/// The content Save writes: `None` while a region is still unresolved, else the
/// editable buffer (falling back to a fresh compose before it has been seeded)
/// with the file's line terminator re-applied.
fn compose_content(file: &ConflictFile, res: &FileResolution) -> Option<String> {
    if !file_resolved(res) {
        return None;
    }
    let buffer = res
        .whole_manual
        .clone()
        .unwrap_or_else(|| compose_string(file, res));
    Some(file.eol.apply(&buffer))
}

/// The two sides of a conflict. `A` = ours (left, teal), `B` = theirs (right, gold).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Ab {
    A,
    B,
}

/// `(a = ours taken, b = theirs taken)` for a region choice.
fn sides_from_choice(choice: RegionChoice) -> (bool, bool) {
    match choice {
        RegionChoice::Ours => (true, false),
        RegionChoice::Theirs => (false, true),
        RegionChoice::Both { .. } => (true, true),
        RegionChoice::Unresolved | RegionChoice::Manual => (false, false),
    }
}

/// New choice from the two take-checkboxes; `prev` decides the Both order — the
/// side already taken stays first, so the order follows the tick sequence.
fn choice_from_sides(a: bool, b: bool, prev: RegionChoice) -> RegionChoice {
    match (a, b) {
        (false, false) => RegionChoice::Unresolved,
        (true, false) => RegionChoice::Ours,
        (false, true) => RegionChoice::Theirs,
        (true, true) => RegionChoice::Both {
            ours_first: matches!(prev, RegionChoice::Ours),
        },
    }
}

pub fn conflict_view(
    ui: &mut egui::Ui,
    palette: &Palette,
    state: &mut ConflictEditorState,
    busy: bool,
) -> ConflictEditorAction {
    let mut action = ConflictEditorAction::default();
    if state.files.is_empty() {
        return action;
    }
    state.ensure_syntax(palette.syntax);

    ui.add_space(PAD);
    toolbar(ui, palette, state, &mut action);
    ui.add_space(PAD);
    ui.separator();
    ui.add_space(PAD);

    if state.confirm_close {
        close_confirm(ui, palette, state, &mut action);
        ui.add_space(PAD);
    }

    let idx = state.file_index.min(state.files.len() - 1);
    let file = &state.files[idx];
    let res = &mut state.resolutions[idx];
    let syntax = state.syntax.as_ref();

    if let Some(disk) = res.disk_divergence.clone() {
        divergence_notice(ui, palette, res, disk);
        ui.add_space(PAD);
    }

    match file.kind {
        ConflictKind::BothModified | ConflictKind::AddedByBoth => {
            let panes_h = panes_height(ui);
            ab_panes(ui, palette, file, res, syntax, panes_h);
            ui.add_space(4.0);
            panes_splitter(ui, palette);
            ui.add_space(4.0);
            output_zone(ui, palette, file, res, busy, &mut action);
        }
        ConflictKind::DeletedByThem => {
            deleted_card(
                ui,
                palette,
                file,
                res,
                "Keep the modified version",
                &mut action,
            );
        }
        ConflictKind::DeletedByUs => {
            deleted_card(
                ui,
                palette,
                file,
                res,
                "Keep the incoming version",
                &mut action,
            );
        }
        ConflictKind::Binary => side_choice_card(
            ui,
            palette,
            file,
            res,
            "binary file — pick which whole side to keep.",
            None,
            &mut action,
        ),
        ConflictKind::Oversize => side_choice_card(
            ui,
            palette,
            file,
            res,
            "too large for the inline editor — pick a whole side.",
            Some("Or resolve it in the terminal."),
            &mut action,
        ),
    }
    action
}

/// Height of the A|B panes: a resizable split (conflicts.md §3), persisted in egui
/// temp data, defaulting to ~45 % of the editor and clamped so the Output keeps room.
fn panes_height(ui: &egui::Ui) -> f32 {
    let avail = ui.available_height();
    let stored = ui.ctx().data_mut(|d| {
        let default_h = (avail * 0.45).clamp(140.0, 420.0);
        *d.get_temp_mut_or(panes_height_id(), default_h)
    });
    stored.clamp(120.0, (avail - 160.0).max(120.0))
}

fn panes_height_id() -> egui::Id {
    egui::Id::new("conflict_panes_height")
}

/// The draggable boundary between the panes and the Output (graph_view's resize
/// grammar): a 1px rule that brightens on hover, dragging stores a new panes height.
fn panes_splitter(ui: &mut egui::Ui, palette: &Palette) {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 9.0),
        egui::Sense::click_and_drag(),
    );
    let response = response.on_hover_cursor(egui::CursorIcon::ResizeVertical);
    let active = response.hovered() || response.dragged();
    let y = rect.center().y;
    ui.painter().line_segment(
        [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
        egui::Stroke::new(
            1.0,
            if active {
                palette.accent
            } else {
                palette.border_subtle
            },
        ),
    );
    if response.dragged() {
        let delta = response.drag_delta().y;
        ui.ctx().data_mut(|d| {
            let id = panes_height_id();
            let current = d.get_temp::<f32>(id).unwrap_or(260.0);
            d.insert_temp(id, current + delta);
        });
    }
}

/// File name + conflict count on the left, Close on the right (conflicts.md §3).
fn toolbar(
    ui: &mut egui::Ui,
    palette: &Palette,
    state: &mut ConflictEditorState,
    action: &mut ConflictEditorAction,
) {
    let idx = state.file_index.min(state.files.len() - 1);
    let path = state.files[idx].path.clone();
    let total = conflict_count(&state.files[idx]);
    let unsaved = state.has_unsaved();
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("⚠").color(palette.git_conflict));
        ui.label(
            egui::RichText::new(path)
                .strong()
                .color(palette.text_primary),
        );
        let suffix = if total == 1 { "" } else { "s" };
        ui.label(
            egui::RichText::new(format!("· {total} conflict{suffix}")).color(palette.text_muted),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("✕ Close").clicked() {
                if unsaved {
                    state.confirm_close = true;
                } else {
                    action.close = true;
                }
            }
        });
    });
}

fn close_confirm(
    ui: &mut egui::Ui,
    palette: &Palette,
    state: &mut ConflictEditorState,
    action: &mut ConflictEditorAction,
) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("Unsaved resolution — discard it?").color(palette.git_modified),
        );
        if ui
            .add(crate::ui::danger_button(palette, "Discard"))
            .clicked()
        {
            action.close = true;
        }
        if ui.button("Keep editing").clicked() {
            state.confirm_close = false;
        }
    });
}

fn divergence_notice(ui: &mut egui::Ui, palette: &Palette, res: &mut FileResolution, disk: String) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("This file was edited outside helm.").color(palette.git_modified),
        );
        if ui.button("Load my version").clicked() {
            res.whole_manual = Some(disk);
            res.whole_override = true;
            res.composed = false;
            res.disk_divergence = None;
            res.dirty = true;
        }
        if ui.button("Start from the merge").clicked() {
            res.whole_manual = None;
            res.whole_override = false;
            res.composed = true;
            res.disk_divergence = None;
        }
    });
}

/// A take-checkbox / body click setting one region's choice.
struct PaneToggle {
    ci: usize,
    choice: RegionChoice,
}

fn apply_pane_toggle(res: &mut FileResolution, toggle: PaneToggle) {
    res.choices[toggle.ci] = toggle.choice;
    res.active = toggle.ci;
}

/// The two sides side by side in one scroll area, each with line-number gutters,
/// per-region take checkboxes and a highlight band (conflicts.md §3–§4).
fn ab_panes(
    ui: &mut egui::Ui,
    palette: &Palette,
    file: &ConflictFile,
    res: &mut FileResolution,
    syntax: Option<&ConflictHighlight>,
    max_height: f32,
) {
    pane_headers(ui, palette, file);
    let mut toggle: Option<PaneToggle> = None;
    let out = egui::ScrollArea::vertical()
        .id_salt("conflict_panes")
        .max_height(max_height)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.columns(2, |cols| {
                let (ta, marks) = pane_rows(&mut cols[0], palette, file, res, syntax, Ab::A);
                let (tb, _) = pane_rows(&mut cols[1], palette, file, res, syntax, Ab::B);
                toggle = ta.or(tb);
                marks
            })
        });
    paint_markers(
        ui,
        out.inner_rect,
        out.state.offset.y,
        out.content_size.y,
        &out.inner,
    );
    if let Some(toggle) = toggle {
        apply_pane_toggle(res, toggle);
        recompose_output(file, res);
    }
}

/// The two pane headers (A · ours | B · theirs), rendered above the scrolled rows so
/// they stay frozen while the panes scroll.
fn pane_headers(ui: &mut egui::Ui, palette: &Palette, file: &ConflictFile) {
    ui.columns(2, |cols| {
        pane_header(
            &mut cols[0],
            palette,
            &file.ours_label,
            Ab::A,
            palette.git_renamed,
        );
        pane_header(
            &mut cols[1],
            palette,
            &file.theirs_label,
            Ab::B,
            palette.git_modified,
        );
    });
}

fn pane_header(ui: &mut egui::Ui, palette: &Palette, label: &str, side: Ab, color: egui::Color32) {
    ui.horizontal(|ui| {
        ab_badge(ui, side, color);
        ui.label(crate::ui::section_label(palette, label));
    });
}

/// One pane's scrolled rows: `(toggle, markers)` — the take/body click, plus a
/// `(screen y, color)` per conflict region for its scrollbar tick.
fn pane_rows(
    ui: &mut egui::Ui,
    palette: &Palette,
    file: &ConflictFile,
    res: &FileResolution,
    syntax: Option<&ConflictHighlight>,
    side: Ab,
) -> (Option<PaneToggle>, Vec<(f32, egui::Color32)>) {
    let color = match side {
        Ab::A => palette.git_renamed,
        Ab::B => palette.git_modified,
    };
    let mut out = None;
    let mut marks = Vec::new();

    ui.spacing_mut().item_spacing.y = 0.0;
    let mut lineno = 1usize;
    let mut ci = 0usize;
    let mut salt = 0usize;
    for (region_idx, region) in file.regions.iter().enumerate() {
        let region_spans = syntax.and_then(|cache| cache.region(region_idx));
        match region {
            Region::Stable(lines) => {
                for (i, line) in lines.iter().enumerate() {
                    code_row(
                        ui,
                        palette,
                        salt,
                        Row {
                            band: None,
                            active: false,
                            check: None,
                            clickable: false,
                            padding: false,
                            gutter: Some(lineno),
                            text: line,
                            spans: region_spans
                                .and_then(|rs| rs.stable.get(i))
                                .map(Vec::as_slice),
                            color: palette.text_secondary,
                        },
                    );
                    salt += 1;
                    lineno += 1;
                }
            }
            Region::Conflict { ours, theirs, .. } => {
                let lines = match side {
                    Ab::A => ours,
                    Ab::B => theirs,
                };
                let (a, b) = sides_from_choice(res.choices[ci]);
                let checked = match side {
                    Ab::A => a,
                    Ab::B => b,
                };
                let active = ci == res.active;
                let height = ours.len().max(theirs.len());
                let start = lineno;
                let mut first_rect = None;
                for row in 0..height {
                    let check = if row == 0 { Some(checked) } else { None };
                    let spans = match side {
                        Ab::A => region_spans.and_then(|rs| rs.ours.get(row)),
                        Ab::B => region_spans.and_then(|rs| rs.theirs.get(row)),
                    }
                    .map(Vec::as_slice);
                    let (text, gutter, line_color, padding) = match lines.get(row) {
                        Some(line) => (line.as_str(), Some(start + row), color, false),
                        None => ("", None, palette.text_muted, true),
                    };
                    let out_row = code_row(
                        ui,
                        palette,
                        salt,
                        Row {
                            band: Some(color),
                            active,
                            check,
                            clickable: true,
                            padding,
                            gutter,
                            text,
                            spans: if padding { None } else { spans },
                            color: line_color,
                        },
                    );
                    salt += 1;
                    if row == 0 {
                        first_rect = Some(out_row.rect);
                    }
                    let new_side = out_row.check.or(out_row.clicked.then_some(!checked));
                    if let Some(v) = new_side {
                        let (na, nb) = match side {
                            Ab::A => (v, b),
                            Ab::B => (a, v),
                        };
                        out = Some(PaneToggle {
                            ci,
                            choice: choice_from_sides(na, nb, res.choices[ci]),
                        });
                    }
                }
                if let Some(rect) = first_rect {
                    marks.push((rect.top(), region_marker_color(palette, res.choices[ci])));
                    if res.scroll_to == Some(ci) {
                        ui.scroll_to_rect(rect, Some(egui::Align::Center));
                    }
                }
                lineno += lines.len();
                ci += 1;
            }
        }
    }
    (out, marks)
}

/// The small A/B chip in a pane header.
fn ab_badge(ui: &mut egui::Ui, side: Ab, color: egui::Color32) {
    let letter = match side {
        Ab::A => "A",
        Ab::B => "B",
    };
    egui::Frame::new()
        .fill(color)
        .corner_radius(egui::CornerRadius::same(3))
        .inner_margin(egui::Margin::symmetric(6, 1))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(letter)
                    .monospace()
                    .strong()
                    .color(egui::Color32::WHITE),
            );
        });
}

/// Paint a conflict region's band slice on one row: a soft tinted fill plus the
/// `color` accent bar on the left edge. Painted per row (not as one frame) so each
/// row stays independently cullable while the slices read as one continuous band.
fn paint_band(ui: &egui::Ui, rect: egui::Rect, color: egui::Color32, active: bool) {
    let painter = ui.painter();
    let fill_alpha = if active {
        BAND_ALPHA_ACTIVE
    } else {
        BAND_ALPHA_DIM
    };
    painter.rect_filled(
        rect,
        egui::CornerRadius::ZERO,
        crate::ui::with_alpha(color, fill_alpha),
    );
    let (bar_color, bar_w) = if active {
        (color, BAR_W)
    } else {
        (crate::ui::with_alpha(color, 150), BAR_W - 1.0)
    };
    painter.rect_filled(
        egui::Rect::from_min_size(rect.min, egui::vec2(bar_w, rect.height())),
        egui::CornerRadius::ZERO,
        bar_color,
    );
}

/// The content of one [`code_row`].
struct Row<'a> {
    /// Conflict band tint + accent colour; `None` for a context row.
    band: Option<egui::Color32>,
    /// Emphasize the band — the focused (nav-active) conflict region.
    active: bool,
    /// Take-checkbox initial value; `None` on a non-region-start row.
    check: Option<bool>,
    /// The body (band minus the checkbox cell) takes the side on click.
    clickable: bool,
    /// A shorter-side filler row — drawn as a faint rule, not content.
    padding: bool,
    /// Line number; `None` for padding / placeholder / base rows.
    gutter: Option<usize>,
    text: &'a str,
    /// Syntax-highlighted spans of the line; `None` falls back to a flat `color`.
    spans: Option<&'a [HighlightedSpan]>,
    color: egui::Color32,
}

/// What a [`code_row`] reported this frame.
struct RowOut {
    rect: egui::Rect,
    /// The take checkbox's new value when it was ticked.
    check: Option<bool>,
    /// The clickable body was clicked (toggles the side).
    clicked: bool,
}

/// A line's [`egui::WidgetText`]: a multi-colour `LayoutJob` when highlighted (its
/// text stays the accessibility label), else a flat-colour monospace `RichText`.
fn line_widget_text(
    text: &str,
    spans: Option<&[HighlightedSpan]>,
    fallback: egui::Color32,
) -> egui::WidgetText {
    match spans {
        Some(spans) if !spans.is_empty() => {
            let mut job = egui::text::LayoutJob::default();
            for span in spans {
                job.append(
                    &span.text,
                    0.0,
                    egui::text::TextFormat::simple(egui::FontId::monospace(CODE_SIZE), span.color),
                );
            }
            job.into()
        }
        _ => egui::RichText::new(text)
            .monospace()
            .size(CODE_SIZE)
            .color(fallback)
            .into(),
    }
}

/// One monospace code line — a fixed take-checkbox cell, the line-number gutter,
/// then the (highlighted) text — at a fixed [`ROW_H`]. The row's rect is always
/// reserved (so the scroll geometry is exact and an off-screen region can still be
/// scrolled to), but the widgets are built only when the row is on screen.
fn code_row(ui: &mut egui::Ui, palette: &Palette, salt: usize, row: Row) -> RowOut {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), ROW_H),
        egui::Sense::hover(),
    );
    let content_left = match row.band {
        Some(c) => {
            paint_band(ui, rect, c, row.active);
            rect.left() + BAR_W + 4.0
        }
        None => rect.left(),
    };
    if !ui.is_rect_visible(rect) {
        return RowOut {
            rect,
            check: None,
            clicked: false,
        };
    }
    if row.padding {
        let y = rect.center().y;
        ui.painter().line_segment(
            [
                egui::pos2(content_left + CHECK_W + 6.0, y),
                egui::pos2(rect.right() - 8.0, y),
            ],
            egui::Stroke::new(1.0, crate::ui::with_alpha(palette.text_muted, 70)),
        );
        return RowOut {
            rect,
            check: None,
            clicked: false,
        };
    }
    let mut checkbox = None;
    let mut clicked = false;
    let content = egui::Rect::from_min_max(egui::pos2(content_left, rect.top()), rect.max);
    ui.scope_builder(
        egui::UiBuilder::new()
            .id_salt(salt)
            .max_rect(content)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
        |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            match row.check {
                Some(v) => {
                    let mut b = v;
                    if ui
                        .add_sized(egui::vec2(CHECK_W, ROW_H), egui::Checkbox::new(&mut b, ""))
                        .clicked()
                    {
                        checkbox = Some(b);
                    }
                }
                None => {
                    let _ =
                        ui.allocate_exact_size(egui::vec2(CHECK_W, ROW_H), egui::Sense::hover());
                }
            }
            let _ = ui.allocate_ui_with_layout(
                egui::vec2(GUTTER_W, ROW_H),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    if let Some(n) = row.gutter {
                        ui.label(
                            egui::RichText::new(n.to_string())
                                .monospace()
                                .color(palette.text_muted),
                        );
                    }
                },
            );
            // A clickable line takes its side; sensed on the Label so it stays a
            // queryable accessibility node (a bare `interact` rect is not).
            let mut label =
                egui::Label::new(line_widget_text(row.text, row.spans, row.color)).truncate();
            if row.clickable {
                label = label.sense(egui::Sense::click());
            }
            let response = ui.add(label);
            if row.clickable {
                clicked = response
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked();
            }
        },
    );
    RowOut {
        rect,
        check: checkbox,
        clicked,
    }
}

/// The colour of a region's scrollbar tick: orange while unresolved, purple once a
/// side is taken — the same read as the in-band colours.
fn region_marker_color(palette: &Palette, choice: RegionChoice) -> egui::Color32 {
    match choice {
        RegionChoice::Unresolved => palette.git_conflict,
        _ => palette.accent,
    }
}

/// Paint a tick per conflict region on a scroll area's scrollbar track, so the
/// regions stay locatable while scrolling a long file (conflicts.md §3). `marks`
/// holds each region's first-row screen-space top and colour.
fn paint_markers(
    ui: &egui::Ui,
    view: egui::Rect,
    offset_y: f32,
    content_h: f32,
    marks: &[(f32, egui::Color32)],
) {
    if content_h <= view.height() + 0.5 || view.height() <= 0.0 {
        return;
    }
    let painter = ui.painter();
    let x1 = view.right();
    let x0 = x1 - MARK_W;
    for (screen_top, color) in marks {
        let content_y = *screen_top - view.top() + offset_y;
        let frac = (content_y / content_h).clamp(0.0, 1.0);
        let y = view.top() + frac * view.height();
        let rect = egui::Rect::from_min_max(
            egui::pos2(x0, y - MARK_H * 0.5),
            egui::pos2(x1, y + MARK_H * 0.5),
        );
        painter.rect_filled(rect, egui::CornerRadius::same(1), *color);
    }
}

/// The editable Output (conflicts.md §5), always a live `TextEdit` à la a real code
/// editor: a line-number gutter, a soft band per conflict region (purple resolved /
/// orange unresolved) and scrollbar ticks — all derived from the galley so they ride
/// the buffer's own layout. The buffer is seeded from the picks and recomposed when a
/// pick changes; a hand edit just keeps the text (the per-region decorations drop, the
/// gutter stays). Save writes the buffer verbatim.
fn output_zone(
    ui: &mut egui::Ui,
    palette: &Palette,
    file: &ConflictFile,
    res: &mut FileResolution,
    busy: bool,
    action: &mut ConflictEditorAction,
) {
    let total = conflict_count(file);
    output_header(ui, palette, file, res, busy, total, action);

    if res.whole_manual.is_none() {
        res.whole_manual = Some(compose_string(file, res));
        res.composed = true;
    }

    // The bands + scrollbar marks map onto buffer lines, exact only while the buffer
    // still mirrors the picks (composed); a hand-edited buffer keeps just the gutter.
    let regions = if res.composed {
        output_region_spans(file, res, palette)
    } else {
        Vec::new()
    };
    let unresolved_lines: Vec<usize> = regions
        .iter()
        .filter(|r| r.unresolved)
        .map(|r| r.start)
        .collect();

    let path = file.path.clone();
    let syntax_theme = palette.syntax;
    let fallback = palette.text_secondary;
    let unresolved_color = palette.git_conflict;
    let scroll_to = res.scroll_to;

    let out = egui::ScrollArea::vertical()
        .id_salt("conflict_result")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let buffer = res.whole_manual.as_mut().expect("seeded above");
            let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| {
                output_galley(
                    ui,
                    &path,
                    syntax_theme,
                    egui::TextBuffer::as_str(buf),
                    wrap_width,
                    fallback,
                    &unresolved_lines,
                    unresolved_color,
                )
            };
            let output = egui::TextEdit::multiline(buffer)
                .code_editor()
                .desired_width(f32::INFINITY)
                .margin(egui::Margin {
                    left: GUTTER_W as i8,
                    right: 6,
                    top: 2,
                    bottom: 2,
                })
                .layouter(&mut layouter)
                .show(ui);
            if output.response.changed() {
                res.dirty = true;
                res.composed = false;
            }
            paint_output_decorations(ui, palette, &output, &regions, scroll_to)
        });
    paint_markers(
        ui,
        out.inner_rect,
        out.state.offset.y,
        out.content_size.y,
        &out.inner,
    );
    res.scroll_to = None;
}

/// One conflict region's footprint in the Output buffer (composed state): the logical
/// line it starts at, its line count, its band colour and whether it is still unresolved.
struct OutputRegionSpan {
    ci: usize,
    start: usize,
    count: usize,
    color: egui::Color32,
    active: bool,
    unresolved: bool,
}

/// Walks the regions like [`compose_rows`], yielding each conflict region's line range
/// in the composed buffer so the bands / marks / unresolved colour line up exactly.
fn output_region_spans(
    file: &ConflictFile,
    res: &FileResolution,
    palette: &Palette,
) -> Vec<OutputRegionSpan> {
    let mut spans = Vec::new();
    let mut line = 0usize;
    let mut ci = 0usize;
    for region in &file.regions {
        match region {
            Region::Stable(lines) => line += lines.len(),
            Region::Conflict { ours, theirs, .. } => {
                let choice = res.choices[ci];
                let count = region_lines(ours, theirs, choice, &res.manual[ci]).len();
                spans.push(OutputRegionSpan {
                    ci,
                    start: line,
                    count,
                    color: region_marker_color(palette, choice),
                    active: ci == res.active,
                    unresolved: choice == RegionChoice::Unresolved,
                });
                line += count;
                ci += 1;
            }
        }
    }
    spans
}

/// Paints the Output's line-number gutter and conflict bands from the laid-out galley,
/// returning the per-region scrollbar marks. Numbers go once per logical line (a
/// wrapped line numbers only on its first row); bands tint a region's full row range
/// translucently (the editable text shows through) with the accent bar in the gutter.
fn paint_output_decorations(
    ui: &mut egui::Ui,
    palette: &Palette,
    output: &egui::text_edit::TextEditOutput,
    regions: &[OutputRegionSpan],
    scroll_to: Option<usize>,
) -> Vec<(f32, egui::Color32)> {
    let gpos = output.galley_pos;
    let rows = &output.galley.rows;
    if rows.is_empty() {
        return Vec::new();
    }
    // The galley row each logical line starts on: row 0, then any row after a
    // newline-terminated one (a long line wraps to several rows, numbered once).
    let mut starts = Vec::with_capacity(rows.len());
    starts.push(0usize);
    for (i, row) in rows.iter().enumerate() {
        if row.ends_with_newline && i + 1 < rows.len() {
            starts.push(i + 1);
        }
    }

    let left = output.response.rect.left();
    let right = output.response.rect.right();
    let last_row = rows.len() - 1;

    let mut marks = Vec::new();
    for span in regions {
        let Some(&first_row) = starts.get(span.start) else {
            continue;
        };
        let end_row = starts
            .get(span.start + span.count)
            .map(|next| next - 1)
            .unwrap_or(last_row)
            .min(last_row);
        let top = gpos.y + rows[first_row].min_y();
        let bottom = gpos.y + rows[end_row].max_y();
        let rect = egui::Rect::from_min_max(egui::pos2(left, top), egui::pos2(right, bottom));
        paint_band(ui, rect, span.color, span.active);
        marks.push((top, span.color));
        if scroll_to == Some(span.ci) {
            ui.scroll_to_rect(rect, Some(egui::Align::Center));
        }
    }

    let clip = ui.clip_rect();
    let painter = ui.painter();
    for (line, &row_idx) in starts.iter().enumerate() {
        let top = gpos.y + rows[row_idx].min_y();
        let bottom = gpos.y + rows[row_idx].max_y();
        if top > clip.bottom() || bottom < clip.top() {
            continue;
        }
        painter.text(
            egui::pos2(left + GUTTER_W - 6.0, top),
            egui::Align2::RIGHT_TOP,
            line + 1,
            egui::FontId::monospace(CODE_SIZE),
            palette.text_muted,
        );
    }
    marks
}

/// Memo of the Output's incremental highlighter and last laid-out galley. syntect's onig
/// `ParseState` is `!Send`, so it can't live in egui temp data; this sits in a thread-local
/// instead — sound because egui renders on a single thread and one Output is open at a time.
/// The highlighter resets itself when the path changes (file switch), so a single slot fits.
#[derive(Default)]
struct OutputHighlight {
    hl: IncrementalHighlighter,
    galley_key: u64,
    galley: Option<Arc<egui::Galley>>,
}

thread_local! {
    static OUTPUT_HL: RefCell<OutputHighlight> = RefCell::new(OutputHighlight::default());
}

/// The editable Output's galley. Colours come from an [`IncrementalHighlighter`] that
/// re-parses only the lines a keystroke touched, so highlighting runs every frame with no
/// flicker; the laid-out galley is memoised too (idle frames reuse it). Above
/// `MAX_LIVE_HIGHLIGHT_BYTES` highlighting is skipped and the buffer renders plain.
/// Reconstructs the buffer exactly so the `TextEdit` cursor still maps.
#[allow(clippy::too_many_arguments)]
fn output_galley(
    ui: &egui::Ui,
    path: &str,
    syntax_theme: &'static str,
    text: &str,
    wrap_width: f32,
    fallback: egui::Color32,
    unresolved: &[usize],
    unresolved_color: egui::Color32,
) -> Arc<egui::Galley> {
    let can_highlight = text.len() <= MAX_LIVE_HIGHLIGHT_BYTES;
    OUTPUT_HL.with_borrow_mut(|cache| {
        let spans = if can_highlight {
            cache.hl.highlight(path, syntax_theme, text)
        } else {
            None
        };
        let galley_key = {
            let mut hasher = DefaultHasher::new();
            path.hash(&mut hasher);
            syntax_theme.hash(&mut hasher);
            text.hash(&mut hasher);
            spans.is_some().hash(&mut hasher);
            wrap_width.to_bits().hash(&mut hasher);
            unresolved.hash(&mut hasher);
            unresolved_color.to_array().hash(&mut hasher);
            hasher.finish()
        };
        if cache.galley_key == galley_key {
            if let Some(galley) = &cache.galley {
                return galley.clone();
            }
        }
        let job = build_output_job(
            spans,
            text,
            wrap_width,
            fallback,
            unresolved,
            unresolved_color,
        );
        let galley = ui.painter().layout_job(job);
        cache.galley_key = galley_key;
        cache.galley = Some(galley.clone());
        galley
    })
}

fn build_output_job(
    spans: Option<&[Vec<HighlightedSpan>]>,
    text: &str,
    wrap_width: f32,
    fallback: egui::Color32,
    unresolved: &[usize],
    unresolved_color: egui::Color32,
) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = wrap_width;
    let font = egui::FontId::monospace(CODE_SIZE);
    match spans {
        Some(lines) => {
            for (i, line) in lines.iter().enumerate() {
                if i > 0 {
                    job.append(
                        "\n",
                        0.0,
                        egui::text::TextFormat::simple(font.clone(), fallback),
                    );
                }
                if unresolved.contains(&i) {
                    let placeholder: String = line.iter().map(|span| span.text.as_str()).collect();
                    job.append(
                        &placeholder,
                        0.0,
                        egui::text::TextFormat::simple(font.clone(), unresolved_color),
                    );
                } else {
                    for span in line {
                        job.append(
                            &span.text,
                            0.0,
                            egui::text::TextFormat::simple(font.clone(), span.color),
                        );
                    }
                }
            }
        }
        None => {
            for (i, line) in text.split('\n').enumerate() {
                if i > 0 {
                    job.append(
                        "\n",
                        0.0,
                        egui::text::TextFormat::simple(font.clone(), fallback),
                    );
                }
                let color = if unresolved.contains(&i) {
                    unresolved_color
                } else {
                    fallback
                };
                job.append(
                    line,
                    0.0,
                    egui::text::TextFormat::simple(font.clone(), color),
                );
            }
        }
    }
    job
}

/// "Output" header: Save and the per-conflict prev/next nav (`▲ i/N ▼`) with the
/// both-order swap (`⇅`).
fn output_header(
    ui: &mut egui::Ui,
    palette: &Palette,
    file: &ConflictFile,
    res: &mut FileResolution,
    busy: bool,
    total: usize,
    action: &mut ConflictEditorAction,
) {
    let active = res.active.min(total.saturating_sub(1));
    res.active = active;
    let resolved = resolved_count(res, total);
    ui.horizontal(|ui| {
        ui.label(crate::ui::section_label(palette, "Output"));
        let (count_color, done) = if resolved == total {
            (palette.accent, true)
        } else {
            (palette.text_muted, false)
        };
        ui.label(egui::RichText::new(format!("{resolved}/{total} resolved")).color(count_color));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let save = egui::Button::new(egui::RichText::new("Save").color(egui::Color32::WHITE))
                .fill(palette.primary_button_fill());
            let reason = if busy {
                "A git command is running — wait for it to finish."
            } else if res.saved {
                "Already saved — nothing changed since."
            } else {
                "Resolve every conflict first (tick a side in each region)."
            };
            if ui
                .add_enabled(!busy && !res.saved && done, save)
                .on_disabled_hover_text(reason)
                .clicked()
            {
                if let Some(content) = compose_content(file, res) {
                    action.resolve = Some(ResolveRequest::Compose {
                        path: file.path.clone(),
                        content,
                    });
                    res.saved = true;
                    res.dirty = false;
                }
            }
            ui.separator();
            // right-to-left: ▼ (next) is added first, so it sits on the right.
            if ui
                .add_enabled(active + 1 < total, egui::Button::new("▼"))
                .clicked()
            {
                res.active = active + 1;
                res.scroll_to = Some(res.active);
            }
            ui.label(
                egui::RichText::new(format!("{}/{}", active + 1, total)).color(palette.text_muted),
            );
            if ui.add_enabled(active > 0, egui::Button::new("▲")).clicked() {
                res.active = active - 1;
                res.scroll_to = Some(res.active);
            }
            let both = !res.whole_override
                && matches!(res.choices.get(active), Some(RegionChoice::Both { .. }));
            if ui
                .add_enabled(both, egui::Button::new("⇅"))
                .on_hover_text("Swap which side leads in this region when both are kept.")
                .clicked()
            {
                if let Some(RegionChoice::Both { ours_first }) = res.choices.get(active).copied() {
                    res.choices[active] = RegionChoice::Both {
                        ours_first: !ours_first,
                    };
                    recompose_output(file, res);
                }
            }
        });
    });
}

/// Count of regions with a side taken; a whole-file override counts as all resolved.
fn resolved_count(res: &FileResolution, total: usize) -> usize {
    if res.whole_override {
        return total;
    }
    res.choices
        .iter()
        .filter(|c| **c != RegionChoice::Unresolved)
        .count()
}

fn deleted_card(
    ui: &mut egui::Ui,
    palette: &Palette,
    file: &ConflictFile,
    res: &mut FileResolution,
    keep_label: &str,
    action: &mut ConflictEditorAction,
) {
    ui.label(
        egui::RichText::new(format!(
            "{} — modified on one side, deleted on the other.",
            file.path
        ))
        .color(palette.text_secondary),
    );
    ui.add_space(PAD);
    ui.horizontal(|ui| {
        if ui.button(keep_label).clicked() {
            action.resolve = Some(ResolveRequest::Keep {
                path: file.path.clone(),
            });
            res.saved = true;
            res.dirty = false;
        }
        if ui
            .add(crate::ui::danger_button(palette, "Delete the file"))
            .clicked()
        {
            action.resolve = Some(ResolveRequest::Delete {
                path: file.path.clone(),
            });
            res.saved = true;
            res.dirty = false;
        }
    });
}

/// A file the inline editor can't compose (binary / oversize): pick one whole side
/// to keep (conflicts.md §5). The worker reads that side's blob from the index.
fn side_choice_card(
    ui: &mut egui::Ui,
    palette: &Palette,
    file: &ConflictFile,
    res: &mut FileResolution,
    message: &str,
    hint: Option<&str>,
    action: &mut ConflictEditorAction,
) {
    ui.label(
        egui::RichText::new(format!("{} — {message}", file.path)).color(palette.text_secondary),
    );
    if let Some(hint) = hint {
        ui.add_space(2.0);
        ui.label(egui::RichText::new(hint).color(palette.text_muted));
    }
    ui.add_space(PAD);
    ui.horizontal(|ui| {
        let mut use_side = |ui: &mut egui::Ui, color, label: &str, ours| {
            ab_badge(ui, if ours { Ab::A } else { Ab::B }, color);
            if ui.button(label).clicked() {
                action.resolve = Some(ResolveRequest::UseSide {
                    path: file.path.clone(),
                    ours,
                });
                res.saved = true;
                res.dirty = false;
            }
        };
        use_side(ui, palette.git_renamed, &file.ours_label, true);
        ui.add_space(PAD);
        use_side(ui, palette.git_modified, &file.theirs_label, false);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::conflict::LineEnding;

    fn both_modified() -> ConflictFile {
        ConflictFile {
            path: "x.rs".to_owned(),
            kind: ConflictKind::BothModified,
            ours_label: "Current · ours".to_owned(),
            theirs_label: "Incoming · theirs".to_owned(),
            regions: vec![
                Region::Stable(vec!["fn run() {".to_owned()]),
                Region::Conflict {
                    ours: vec!["    ours_a".to_owned(), "    ours_b".to_owned()],
                    theirs: vec!["    theirs_a".to_owned()],
                    base: vec!["    base".to_owned()],
                },
                Region::Stable(vec!["}".to_owned()]),
            ],
            has_base: true,
            eol: LineEnding::default(),
            disk_divergence: None,
        }
    }

    fn resolution(file: &ConflictFile, choice: RegionChoice) -> FileResolution {
        let mut res = FileResolution::new(file);
        res.choices[0] = choice;
        res
    }

    #[test]
    fn unresolved_blocks_save() {
        let file = both_modified();
        let res = FileResolution::new(&file);
        assert!(!file_resolved(&res));
        assert_eq!(compose_content(&file, &res), None);
    }

    #[test]
    fn taking_ours_composes_the_ours_side() {
        let file = both_modified();
        let res = resolution(&file, RegionChoice::Ours);
        assert_eq!(
            compose_content(&file, &res),
            Some("fn run() {\n    ours_a\n    ours_b\n}\n".to_owned())
        );
    }

    #[test]
    fn taking_theirs_composes_the_theirs_side() {
        let file = both_modified();
        let res = resolution(&file, RegionChoice::Theirs);
        assert_eq!(
            compose_content(&file, &res),
            Some("fn run() {\n    theirs_a\n}\n".to_owned())
        );
    }

    #[test]
    fn both_concatenates_in_the_tick_order() {
        let file = both_modified();
        // ours_first = false → theirs then ours (B ticked first).
        let theirs_first = resolution(&file, RegionChoice::Both { ours_first: false });
        assert_eq!(
            compose_content(&file, &theirs_first),
            Some("fn run() {\n    theirs_a\n    ours_a\n    ours_b\n}\n".to_owned())
        );
        let ours_first = resolution(&file, RegionChoice::Both { ours_first: true });
        assert_eq!(
            compose_content(&file, &ours_first),
            Some("fn run() {\n    ours_a\n    ours_b\n    theirs_a\n}\n".to_owned())
        );
    }

    #[test]
    fn manual_uses_the_hand_edited_buffer() {
        let file = both_modified();
        let mut res = resolution(&file, RegionChoice::Manual);
        res.manual[0] = "    hand_edited".to_owned();
        assert_eq!(
            compose_content(&file, &res),
            Some("fn run() {\n    hand_edited\n}\n".to_owned())
        );
    }

    #[test]
    fn save_re_applies_the_files_line_terminator() {
        let mut file = both_modified();
        file.eol = LineEnding {
            crlf: true,
            final_newline: true,
        };
        let res = resolution(&file, RegionChoice::Ours);
        assert_eq!(
            compose_content(&file, &res),
            Some("fn run() {\r\n    ours_a\r\n    ours_b\r\n}\r\n".to_owned())
        );
        // The editable buffer itself stays LF — no `\r` glyph in the TextEdit.
        assert_eq!(
            compose_string(&file, &res),
            "fn run() {\n    ours_a\n    ours_b\n}\n"
        );
    }

    #[test]
    fn a_file_without_a_final_newline_does_not_gain_one() {
        let mut file = both_modified();
        file.eol = LineEnding {
            crlf: false,
            final_newline: false,
        };
        let res = resolution(&file, RegionChoice::Ours);
        assert_eq!(
            compose_content(&file, &res),
            Some("fn run() {\n    ours_a\n    ours_b\n}".to_owned())
        );
    }

    #[test]
    fn checkbox_sides_round_trip() {
        use RegionChoice::*;
        assert_eq!(sides_from_choice(Unresolved), (false, false));
        assert_eq!(sides_from_choice(Ours), (true, false));
        assert_eq!(sides_from_choice(Theirs), (false, true));
        assert_eq!(sides_from_choice(Both { ours_first: true }), (true, true));

        // Ticking the second side keeps the first as the leading one.
        assert_eq!(
            choice_from_sides(true, true, Ours),
            Both { ours_first: true }
        );
        assert_eq!(
            choice_from_sides(true, true, Theirs),
            Both { ours_first: false }
        );
        // Un-ticking falls back to the side left standing.
        assert_eq!(
            choice_from_sides(false, true, Both { ours_first: true }),
            Theirs
        );
        assert_eq!(
            choice_from_sides(true, false, Both { ours_first: true }),
            Ours
        );
        assert_eq!(choice_from_sides(false, false, Ours), Unresolved);
    }
}
