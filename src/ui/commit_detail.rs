use crate::git::commit_detail::{CommitDetail, CommitFile};
use crate::git::file_tree::{self, TreeRow};
use crate::theme::{Palette, BODY_SIZE, RADIUS_PILL, SECTION_TITLE_SIZE, TITLE_SIZE};
use std::collections::HashSet;
use std::path::Path;

use crate::ui::detail::{author_avatar, count_chip};
use crate::ui::file_list::{self, row_separator, FileMenuCtx, FileMenuOutput, FileViewMode};
use crate::ui::git_panel::ratio_bar;
use crate::ui::spinner::Spinner;
use crate::ui::{arrow_nav_pressed, format_date_time, paint_icon, ArrowNav, SECTION_TOP_MARGIN};

const TITLE_ICON_GLYPH: f32 = 15.0;
const HASH_SIZE: f32 = 11.0;
const META_SIZE: f32 = 12.0;
const AVATAR_GAP: f32 = 9.0;
const AUTHOR_NAME_SIZE: f32 = 13.0;
const SUBJECT_SIZE: f32 = 15.0;
const TOTALS_SIZE: f32 = 13.0;

/// Detail loading spinner (a11y label, no visible text — same pattern as the
/// graph loader).
const SPINNER_SIZE: f32 = 22.0;
const LOADING_LABEL: &str = "Loading commit";

/// Detail of the commit selected in the right sidebar in Graph mode (git.md §9, M9-6),
/// following the "commit page" mockup: a **Commit** header, an author block (initials
/// avatar + name, "authored `DD/MM/YYYY @ HH:MM`" line at the author's wall-clock
/// time, email on hover, hash chip on the right), the subject emphasized + a dimmed
/// body, then a **Files changed** band (count chip, `+A`/`−D` totals, ratio bar —
/// same language as the M13 card) and the file list with per-file stats. Clicking a
/// file sets `open_file` to the displayed commit's oid and the path (intent to open
/// the full-screen diff, arbitrated by the caller; rendering M9-7). Only rendered
/// with a commit selected: `detail` still `None` means the worker's reply is in
/// flight ⇒ centered spinner (never a stale list, never a placeholder). Read-only:
/// no staging controls.
///
/// `open` = file whose full-screen diff is open: its row is emphasized, and ↑/↓
/// without modifier open the commit's previous / next file with wraparound
/// (keybindings §3, same path as the status sidebar) — via the same `open_file`
/// intent as a click.
#[allow(clippy::too_many_arguments)]
pub fn commit_detail_panel(
    ui: &mut egui::Ui,
    palette: &Palette,
    detail: Option<&CommitDetail>,
    open: Option<&(git2::Oid, String)>,
    open_file: &mut Option<(git2::Oid, String)>,
    repo_root: Option<&Path>,
    file_menu: &mut FileMenuOutput,
    view: FileViewMode,
    set_view: &mut Option<FileViewMode>,
) {
    panel_header(ui, palette);
    let Some(detail) = detail else {
        loading_placeholder(ui, palette);
        return;
    };

    // Only a file of THIS commit arms the navigation (the open diff may target a
    // commit whose list is no longer displayed).
    let open_path = open.and_then(|(oid, path)| (*oid == detail.meta.oid).then_some(path.as_str()));
    meta_block(ui, palette, detail);
    let mut menu = FileMenuCtx {
        root: repo_root,
        out: file_menu,
    };
    // Tree-view directory folding lives in egui temp keyed by the commit oid:
    // session-only (no persistence), and a fresh selection starts expanded.
    let collapse_id = egui::Id::new(("commit_detail_dirs", detail.meta.oid));
    let mut collapsed: HashSet<String> = ui.data(|d| d.get_temp(collapse_id).unwrap_or_default());
    if let Some(target) = files_section(
        ui,
        palette,
        detail,
        open_path,
        open_file,
        &mut menu,
        view,
        &mut collapsed,
    ) {
        *set_view = Some(target);
    }
    ui.data_mut(|d| d.insert_temp(collapse_id, collapsed.clone()));
    if let (Some(path), Some(nav)) = (open_path, arrow_nav_pressed(ui)) {
        navigate_open_file(detail, path, nav, open_file, view, &collapsed);
        // Only keyboard navigation follows the row into the viewport — never the
        // click (a clicked row is already visible).
        if let Some(target) = open_file.clone() {
            file_list::request_row_scroll(ui, open_file_scroll_id(), target);
        }
    }
}

fn open_file_scroll_id() -> egui::Id {
    egui::Id::new("commit_detail_open_file_scroll")
}

/// Centered spinner while the selected commit's detail is in flight (M9-2 reply
/// pending): the panel never shows a placeholder between the click and the data.
fn loading_placeholder(ui: &mut egui::Ui, palette: &Palette) {
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() / 2.0 - SPINNER_SIZE);
        ui.add(Spinner::new().size(SPINNER_SIZE).color(palette.text_muted))
            .widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::ProgressIndicator, true, LOADING_LABEL)
            });
    });
}

/// ↑/↓ when a commit file's full-screen diff is open: opens the previous / next
/// file, with wraparound.
fn navigate_open_file(
    detail: &CommitDetail,
    open_path: &str,
    nav: ArrowNav,
    open_file: &mut Option<(git2::Oid, String)>,
    view: FileViewMode,
    collapsed: &HashSet<String>,
) {
    let order = open_file_order(detail, view, collapsed);
    if order.is_empty() {
        return;
    }
    let current = order.iter().position(|path| *path == open_path);
    let target = match (current, nav) {
        (Some(0) | None, ArrowNav::Up) => order.len() - 1,
        (Some(index), ArrowNav::Up) => index - 1,
        (Some(index), ArrowNav::Down) => (index + 1) % order.len(),
        (None, ArrowNav::Down) => 0,
    };
    *open_file = Some((detail.meta.oid, order[target].to_owned()));
}

/// ↑/↓ navigation order: the files in **display order**. Tree mode follows the
/// visible tree (a collapsed directory hides its files) so the arrows match what
/// the eye sees.
fn open_file_order<'a>(
    detail: &'a CommitDetail,
    view: FileViewMode,
    collapsed: &HashSet<String>,
) -> Vec<&'a str> {
    match view {
        FileViewMode::Flat => detail.files.iter().map(|f| f.path.as_str()).collect(),
        FileViewMode::Tree => {
            let paths: Vec<&str> = detail.files.iter().map(|f| f.path.as_str()).collect();
            file_tree::tree_rows(&paths, collapsed)
                .into_iter()
                .filter_map(|row| match row {
                    TreeRow::File { index, .. } => Some(detail.files[index].path.as_str()),
                    TreeRow::Dir { .. } => None,
                })
                .collect()
        }
    }
}

fn leaf_name(path: &str) -> &str {
    path.rsplit_once('/').map_or(path, |(_, name)| name)
}

fn panel_header(ui: &mut egui::Ui, palette: &Palette) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let (icon_rect, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
        paint_icon(
            ui.painter(),
            icon_rect.center(),
            TITLE_ICON_GLYPH,
            lucide_icons::Icon::GitGraph,
            palette.text_primary,
        );
        ui.add_space(2.0);
        ui.label(
            egui::RichText::new("Commit")
                .size(TITLE_SIZE)
                .strong()
                .color(palette.text_primary),
        );
    });
}

fn meta_block(ui: &mut egui::Ui, palette: &Palette, detail: &CommitDetail) {
    let meta = &detail.meta;
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        author_avatar(ui, palette, &meta.author);
        ui.add_space(AVATAR_GAP);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            let name = ui.label(
                egui::RichText::new(&meta.author)
                    .size(AUTHOR_NAME_SIZE)
                    .strong()
                    .color(palette.text_primary),
            );
            if !meta.email.is_empty() {
                name.on_hover_text(&meta.email);
            }
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.label(
                    egui::RichText::new("authored")
                        .size(META_SIZE)
                        .color(palette.text_muted),
                );
                // Author's wall-clock time, like `git log` (commit offset applied).
                let wall_clock = meta.time + i64::from(meta.offset_minutes) * 60;
                ui.label(
                    egui::RichText::new(format_date_time(wall_clock))
                        .size(META_SIZE)
                        .color(palette.text_secondary),
                );
            });
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            hash_chip(ui, palette, &meta.short_id);
        });
    });
    ui.add_space(10.0);
    ui.label(
        egui::RichText::new(&meta.summary)
            .size(SUBJECT_SIZE)
            .strong()
            .color(palette.text_primary),
    );
    if !meta.body.is_empty() {
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new(&meta.body)
                .size(BODY_SIZE)
                .color(palette.text_secondary),
        );
    }
}

fn hash_chip(ui: &mut egui::Ui, palette: &Palette, hash: &str) {
    let font = egui::FontId::monospace(HASH_SIZE);
    let galley = ui
        .painter()
        .layout_no_wrap(hash.to_owned(), font, egui::Color32::PLACEHOLDER);
    let size = galley.size() + egui::vec2(12.0, 5.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter();
    painter.rect(
        rect,
        egui::CornerRadius::same(RADIUS_PILL),
        palette.bg_surface,
        egui::Stroke::new(1.0, palette.border_subtle),
        egui::StrokeKind::Inside,
    );
    painter.galley(
        rect.center() - galley.size() / 2.0,
        galley,
        palette.text_secondary,
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, hash));
}

fn files_header(
    ui: &mut egui::Ui,
    palette: &Palette,
    detail: &CommitDetail,
    view: FileViewMode,
) -> Option<FileViewMode> {
    let (additions, deletions) = detail.total_line_stats();
    let mut set_view = None;
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("Files changed")
                .size(SECTION_TITLE_SIZE)
                .strong()
                .color(palette.text_primary),
        );
        ui.add_space(2.0);
        count_chip(ui, palette, detail.files.len());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            set_view = file_list::view_toggle(ui, palette, view);
            ui.add_space(8.0);
            ratio_bar(ui, palette, additions, deletions);
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(format!("−{deletions}"))
                    .size(TOTALS_SIZE)
                    .color(palette.git_deleted),
            );
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(format!("+{additions}"))
                    .size(TOTALS_SIZE)
                    .color(palette.git_added),
            );
        });
    });
    set_view
}

#[allow(clippy::too_many_arguments)]
fn files_section(
    ui: &mut egui::Ui,
    palette: &Palette,
    detail: &CommitDetail,
    open_path: Option<&str>,
    open_file: &mut Option<(git2::Oid, String)>,
    menu: &mut FileMenuCtx,
    view: FileViewMode,
    collapsed: &mut HashSet<String>,
) -> Option<FileViewMode> {
    ui.add_space(SECTION_TOP_MARGIN);
    let set_view = files_header(ui, palette, detail, view);
    ui.add_space(6.0);
    egui::ScrollArea::vertical()
        .id_salt("commit_detail_files")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // Rows abut (breathing room lives in ROW_HEIGHT): the hover highlight
            // fills the whole line, with no dead band at the separators.
            ui.spacing_mut().item_spacing.y = 0.0;
            match view {
                FileViewMode::Flat => {
                    for (index, file) in detail.files.iter().enumerate() {
                        if index > 0 {
                            row_separator(ui, palette);
                        }
                        let selected = open_path == Some(file.path.as_str());
                        file_row(
                            ui,
                            palette,
                            detail.meta.oid,
                            file,
                            selected,
                            0.0,
                            &file.path,
                            open_file,
                            menu,
                        );
                    }
                }
                FileViewMode::Tree => {
                    let paths: Vec<&str> = detail.files.iter().map(|f| f.path.as_str()).collect();
                    let rows = file_tree::tree_rows(&paths, collapsed);
                    let mut toggle: Option<String> = None;
                    for row in rows {
                        match row {
                            TreeRow::Dir {
                                name,
                                full_path,
                                depth,
                                collapsed: is_collapsed,
                            } => {
                                let indent = depth as f32 * file_list::TREE_INDENT_STEP;
                                if file_list::dir_row(ui, palette, &name, indent, is_collapsed)
                                    .clicked()
                                {
                                    toggle = Some(full_path);
                                }
                            }
                            TreeRow::File { index, depth } => {
                                let file = &detail.files[index];
                                let indent = depth as f32 * file_list::TREE_INDENT_STEP;
                                let selected = open_path == Some(file.path.as_str());
                                file_row(
                                    ui,
                                    palette,
                                    detail.meta.oid,
                                    file,
                                    selected,
                                    indent,
                                    leaf_name(&file.path),
                                    open_file,
                                    menu,
                                );
                            }
                        }
                    }
                    if let Some(dir) = toggle {
                        if !collapsed.remove(&dir) {
                            collapsed.insert(dir);
                        }
                    }
                }
            }
        });
    set_view
}

#[allow(clippy::too_many_arguments)]
fn file_row(
    ui: &mut egui::Ui,
    palette: &Palette,
    oid: git2::Oid,
    file: &CommitFile,
    selected: bool,
    indent: f32,
    display: &str,
    open_file: &mut Option<(git2::Oid, String)>,
    menu: &mut FileMenuCtx,
) {
    // Shared file-list style (binary ⇒ 0/0 ⇒ no stats).
    let row = file_list::file_row(
        ui,
        palette,
        egui::Sense::click(),
        &file_list::FileRow {
            path: display,
            kind: file.kind,
            additions: file.additions,
            deletions: file.deletions,
            selected,
            stats_hidden_on_hover: false,
            indent,
            trailing_reserved: 0.0,
        },
    );
    if selected {
        let target = (oid, file.path.clone());
        file_list::consume_row_scroll(ui, &row.response, open_file_scroll_id(), &target);
    }
    if row.response.clicked() {
        *open_file = Some((oid, file.path.clone()));
    }
    file_list::file_context_menu(&row.response, &file.path, menu);
    row.response.on_hover_text(&file.path).widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, &file.path)
    });
}
