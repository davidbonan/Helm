use crate::agents::{Agent, CommitMessageSettings, Preset, ReviewSettings, PROMPT_ENV};
use crate::git::sync::PullDefault;
use crate::keybindings::{Action, Group, Keymap, Shortcut};
use crate::pull_requests::runner::SourceStatus;
use crate::terminal::links::Editor;
use crate::theme::{self, Palette, ThemeMode, RADIUS_PILL};
use crate::ui::spinner::Spinner;
use crate::ui::{clickable, paint_icon, MACOS_TITLEBAR_INSET};
use crate::update::UpdateState;

const SEGMENT_SIZE: egui::Vec2 = egui::vec2(88.0, 36.0);
const SEGMENT_LABEL_SIZE: f32 = 13.0;
const SEGMENT_ICON_SIZE: f32 = 15.0;
const SEGMENT_ICON_GAP: f32 = 6.0;

const DROPDOWN_PAD_X: f32 = 12.0;
const DROPDOWN_GAP: f32 = 6.0;
const DROPDOWN_ICON_SIZE: f32 = 13.0;
const DROPDOWN_MIN_WIDTH: f32 = 150.0;
const DROPDOWN_POPUP_GAP: f32 = 4.0;

const CARD_RADIUS: u8 = 12;
const CARD_PAD_X: f32 = 16.0;
const CARD_GAP: f32 = 16.0;
const ROW_MIN_HEIGHT: f32 = 76.0;
const LABEL_SIZE: f32 = 13.0;
const DESCRIPTION_SIZE: f32 = 12.0;
const LABEL_GAP: f32 = 4.0;

const NAV_WIDTH: f32 = 240.0;
const NAV_PAD: f32 = 12.0;
const NAV_ROW_HEIGHT: f32 = 32.0;
const NAV_ROW_RADIUS: u8 = 7;
const NAV_ROW_GAP: f32 = 2.0;
const NAV_ICON_SIZE: f32 = 15.0;
const NAV_LABEL_SIZE: f32 = 13.0;
const NAV_SECTIONS_GAP: f32 = 24.0;
const TITLE_SIZE: f32 = 24.0;
const TITLE_GAP: f32 = 24.0;
const CONTENT_PAD_X: f32 = 32.0;
const CONTENT_PAD_Y: f32 = 40.0;
const CONTENT_MAX_WIDTH: f32 = 640.0;

const MODES: [(ThemeMode, &str, lucide_icons::Icon); 3] = [
    (ThemeMode::Auto, "Auto", lucide_icons::Icon::Monitor),
    (ThemeMode::Light, "Light", lucide_icons::Icon::Sun),
    (ThemeMode::Dark, "Dark", lucide_icons::Icon::Moon),
];

/// Sections of the Preferences page (preferences.md §4). The active section is
/// **session** memory (§5): not persisted, default Appearance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PreferencesSection {
    #[default]
    Appearance,
    Git,
    Keyboard,
    Terminal,
    Agents,
    Phone,
    PullRequests,
    Project,
    Updates,
}

const SECTIONS: [PreferencesSection; 9] = [
    PreferencesSection::Appearance,
    PreferencesSection::Git,
    PreferencesSection::Keyboard,
    PreferencesSection::Terminal,
    PreferencesSection::Agents,
    PreferencesSection::Phone,
    PreferencesSection::PullRequests,
    PreferencesSection::Project,
    PreferencesSection::Updates,
];

impl PreferencesSection {
    fn title(self) -> &'static str {
        match self {
            PreferencesSection::Appearance => "Appearance",
            PreferencesSection::Git => "Git",
            PreferencesSection::Keyboard => "Keyboard",
            PreferencesSection::Terminal => "Terminal",
            PreferencesSection::Agents => "Agents",
            PreferencesSection::Phone => "Phone",
            PreferencesSection::PullRequests => "Pull Requests",
            PreferencesSection::Project => "Project",
            PreferencesSection::Updates => "Updates",
        }
    }

    fn icon(self) -> lucide_icons::Icon {
        match self {
            PreferencesSection::Appearance => lucide_icons::Icon::SunMoon,
            PreferencesSection::Git => lucide_icons::Icon::GitBranch,
            PreferencesSection::Keyboard => lucide_icons::Icon::Keyboard,
            PreferencesSection::Terminal => lucide_icons::Icon::SquareTerminal,
            PreferencesSection::Agents => lucide_icons::Icon::Bot,
            PreferencesSection::Phone => lucide_icons::Icon::Smartphone,
            PreferencesSection::PullRequests => lucide_icons::Icon::GitPullRequest,
            PreferencesSection::Project => lucide_icons::Icon::FolderGit2,
            PreferencesSection::Updates => lucide_icons::Icon::Download,
        }
    }
}

/// Recorder of the Keyboard section (preferences.md §4): the armed row and its
/// inline refusal. Session memory held by the app — it also gates the page's
/// `Esc`-closes and `Cmd+,`-toggles while a capture is in flight.
#[derive(Debug, Default)]
pub struct KeyboardState {
    pub recording: Option<Action>,
    pub error: Option<(Action, String)>,
}

/// Read-only snapshot of the updater (update.md §6) rendered by the Updates
/// section — built by the app each frame; the page only raises intents.
pub struct UpdatesView {
    /// Current version, without the `v` prefix.
    pub version: String,
    pub state: UpdateState,
    /// `false` outside an `.app` bundle (`cargo run`): updater disabled.
    pub bundled: bool,
}

/// Read-only snapshot of the PR sources rendered by the Pull Requests section
/// (pull-requests.md §3) — each source's usability from the last fetch. Built by
/// the app from `pr_cache`; the page only reads it and raises creds intents.
pub struct PrSourcesView {
    pub github: SourceStatus,
    pub bitbucket: SourceStatus,
    /// `false` until the first fetch replies — the status lines read "Checking…".
    pub loaded: bool,
}

/// The Phone section (preferences.md §4): what outlives phone access. The devices
/// are a snapshot of the app's book; revoking is an intent.
pub struct PhoneView<'a> {
    pub start_at_launch: &'a mut bool,
    pub devices: &'a [PhoneDeviceRow],
}

pub struct PhoneDeviceRow {
    pub name: String,
    /// *Paired <date> · Last seen <age>*.
    pub detail: String,
}

/// Signals raised by the page: the app closes (`back`) or applies + persists the
/// mutated setting (`theme_changed` / `pull_changed` / `agents_changed`) — no prefs
/// writes in the UI (§5), and a Pull default never triggers an operation.
/// `theme_changed` covers the mode **and** the light/dark theme families. The updater
/// intents (`check_updates` / `install_update`) are routed to the runner by the
/// app — the page executes nothing (update.md §6).
#[derive(Debug, Default, Clone, Copy)]
pub struct PreferencesAction {
    pub back: bool,
    pub theme_changed: bool,
    pub pull_changed: bool,
    /// The terminal's editor (IDE) was changed — the app persists it
    /// (terminal.md §12); it never opens anything on its own.
    pub editor_changed: bool,
    pub check_updates: bool,
    pub install_update: bool,
    /// The Terminal section asked for the `helm` shell command to be symlinked
    /// into the PATH (specs/cli.md §7) — the page writes no file itself.
    pub install_shell_command: bool,
    /// A per-project field (worktree base / post-create script) was edited — the
    /// app writes the edit buffers back to `prefs` (worktrees.md §6).
    pub project_changed: bool,
    /// The Project section's picker selected another project (index into
    /// `ProjectView::projects`) — the app rescopes the section to it.
    pub project_selected: Option<usize>,
    /// The "Choose…" button asked for the native folder picker (the page never
    /// opens `rfd` itself — intent pattern, architecture.md §1).
    pub pick_worktree_base: bool,
    /// A binding changed (capture/unbind/reset/restore): the mutated `Keymap` is
    /// live for routing at once — the app persists it (keybindings.md §6).
    pub keymap_changed: bool,
    /// The agent completion-notification toggle flipped — the app persists it
    /// (specs/agents.md).
    pub agent_notify_changed: bool,
    /// The agents table, the commit message or the review agent was edited
    /// (preferences.md §4 *Agents*).
    pub agents_changed: bool,
    /// The *Start at launch* toggle flipped — the app persists it (remote.md §3.4).
    pub phone_at_launch_changed: bool,
    /// Revoke the paired device at this index of `PhoneView::devices`.
    pub revoke_device: Option<usize>,
    pub revoke_all_devices: bool,
    /// The Bitbucket email field was edited — the app persists it
    /// (pull-requests.md §3); the token stays out of `prefs`.
    pub bitbucket_email_changed: bool,
    /// The "Save" button asked to store the typed Bitbucket token in the Keychain
    /// (pull-requests.md §3) — the page never touches the Keychain itself.
    pub save_bitbucket_token: bool,
}

/// Settings of the project the Project section configures (worktrees.md §6): the
/// app passes the edit buffers `&mut` and persists on change. `None` when no
/// repository is open.
pub struct ProjectView<'a> {
    /// Folder names of every workspace project, in sidebar order — the choices of
    /// the section's picker (replaces the title); `selected` indexes into it.
    pub projects: &'a [String],
    /// Index of the project the edit buffers below belong to.
    pub selected: usize,
    pub worktree_base: &'a mut String,
    pub post_create: &'a mut String,
    /// Per-project run command (git.md §3); empty falls back to auto-detection.
    pub run_command: &'a mut String,
    /// Project base port for `$PORT` (git.md §3); empty falls back to 3000.
    pub base_port: &'a mut String,
    /// Default base shown as the field placeholder (`<root>.worktrees`).
    pub base_hint: &'a str,
}

/// Full-window Preferences page (preferences.md §3): fixed left nav
/// (`bg.sidebar`, "← Back to app" row then section items) + scrollable content
/// (`bg.canvas`, title then cards bounded to ~640pt).
#[allow(clippy::too_many_arguments)]
pub fn preferences_page(
    ui: &mut egui::Ui,
    palette: &Palette,
    section: &mut PreferencesSection,
    mode: &mut ThemeMode,
    light_theme: &mut String,
    dark_theme: &mut String,
    pull_default: &mut PullDefault,
    editor: &mut Editor,
    bitbucket_email: &mut String,
    bitbucket_token: &mut String,
    pr_sources: &PrSourcesView,
    notify_on_agent_completion: &mut bool,
    mut agents: AgentsView<'_>,
    phone: PhoneView<'_>,
    keymap: &mut Keymap,
    keyboard: &mut KeyboardState,
    updates: &UpdatesView,
    shell_command: &crate::cli::ShellCommand,
    release_notes_cache: &mut egui_commonmark::CommonMarkCache,
    mut project: Option<ProjectView<'_>>,
) -> PreferencesAction {
    let mut action = PreferencesAction::default();
    let rect = ui.available_rect_before_wrap();
    let split_x = rect.left() + NAV_WIDTH;
    let nav_rect = egui::Rect::from_min_max(rect.min, egui::pos2(split_x, rect.bottom()));
    let content_rect = egui::Rect::from_min_max(egui::pos2(split_x, rect.top()), rect.max);
    ui.painter().rect_filled(nav_rect, 0, palette.bg_sidebar);
    ui.painter().rect_filled(content_rect, 0, palette.bg_canvas);
    ui.painter().vline(
        split_x,
        rect.y_range(),
        egui::Stroke::new(1.0_f32, palette.border_subtle),
    );

    // The nav occupies the top-left corner: its content sits under the macOS traffic lights.
    let nav_inner = egui::Rect::from_min_max(
        egui::pos2(
            nav_rect.left() + NAV_PAD,
            nav_rect.top() + f32::from(MACOS_TITLEBAR_INSET),
        ),
        egui::pos2(nav_rect.right() - NAV_PAD, nav_rect.bottom() - NAV_PAD),
    );
    let mut nav = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(nav_inner)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    nav.spacing_mut().item_spacing.y = NAV_ROW_GAP;
    if nav_row(
        &mut nav,
        palette,
        lucide_icons::Icon::ArrowLeft,
        "Back to app",
        false,
    )
    .clicked()
    {
        action.back = true;
    }
    nav.add_space(NAV_SECTIONS_GAP);
    for candidate in SECTIONS {
        if nav_row(
            &mut nav,
            palette,
            candidate.icon(),
            candidate.title(),
            *section == candidate,
        )
        .clicked()
        {
            *section = candidate;
        }
    }

    // Column bounded (~640pt) and **centered** in the content zone — the section
    // floats at the center full-screen instead of staying anchored left; the
    // `min` bounds by the real space at widths near the minimum.
    let column_width = CONTENT_MAX_WIDTH.min(content_rect.width() - 2.0 * CONTENT_PAD_X);
    let content_inner = egui::Rect::from_x_y_ranges(
        egui::Rangef::new(
            content_rect.center().x - column_width / 2.0,
            content_rect.center().x + column_width / 2.0,
        ),
        content_rect.y_range(),
    );
    // The scroll area spans the whole content zone, so its bar sits on the
    // window's edge rather than against the cards.
    let mut content = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(content_rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(&mut content, |ui| {
            let column = egui::Rect::from_x_y_ranges(
                content_inner.x_range(),
                ui.available_rect_before_wrap().y_range(),
            );
            ui.scope_builder(egui::UiBuilder::new().max_rect(column), |ui| {
                ui.set_width(ui.available_width());
                ui.add_space(CONTENT_PAD_Y);
                // The Project section swaps its title for a picker over the workspace's
                // projects (preferences.md §4); every other section keeps a plain title.
                match (*section, project.as_ref()) {
                    (PreferencesSection::Project, Some(p)) => {
                        action.project_selected =
                            project_title_dropdown(ui, palette, p.projects, p.selected);
                    }
                    _ => section_title(ui, palette, section.title()),
                }
                ui.add_space(TITLE_GAP);
                match *section {
                    PreferencesSection::Appearance => {
                        settings_card(ui, palette, |ui| {
                            setting_row(
                                ui,
                                palette,
                                "Theme",
                                Some("Use light, dark, or match your system"),
                                |ui| {
                                    if theme_segments(ui, palette, mode) {
                                        action.theme_changed = true;
                                    }
                                },
                            );
                            setting_divider(ui, palette);
                            setting_row(
                                ui,
                                palette,
                                "Light theme",
                                Some("Colors used when the appearance is light"),
                                |ui| {
                                    if preset_dropdown(ui, palette, false, light_theme) {
                                        action.theme_changed = true;
                                    }
                                },
                            );
                            setting_divider(ui, palette);
                            setting_row(
                                ui,
                                palette,
                                "Dark theme",
                                Some("Colors used when the appearance is dark"),
                                |ui| {
                                    if preset_dropdown(ui, palette, true, dark_theme) {
                                        action.theme_changed = true;
                                    }
                                },
                            );
                        });
                    }
                    PreferencesSection::Git => {
                        settings_card(ui, palette, |ui| {
                            setting_row(
                                ui,
                                palette,
                                "Default pull behavior",
                                Some("Operation run by the Pull button in the graph toolbar"),
                                |ui| {
                                    if pull_dropdown(ui, palette, pull_default) {
                                        action.pull_changed = true;
                                    }
                                },
                            );
                        });
                    }
                    PreferencesSection::Keyboard => {
                        keyboard_section(ui, palette, keymap, keyboard, &mut action);
                    }
                    PreferencesSection::Terminal => {
                        settings_card(ui, palette, |ui| {
                            setting_row(
                                ui,
                                palette,
                                "Editor",
                                Some("IDE opened by a Cmd+click on a file link in the terminal"),
                                |ui| {
                                    if editor_dropdown(ui, palette, editor) {
                                        action.editor_changed = true;
                                    }
                                },
                            );
                            setting_divider(ui, palette);
                            shell_command_row(ui, palette, shell_command, &mut action);
                        });
                    }
                    PreferencesSection::Agents => {
                        action.agents_changed = agents_card(ui, palette, &mut agents);
                        ui.add_space(GROUP_GAP);
                        group_title(ui, palette, "Tasks");
                        action.agents_changed |=
                            commit_message_card(ui, palette, agents.commit_message, agents.agents);
                        ui.add_space(CARD_GAP);
                        action.agents_changed |=
                            review_card(ui, palette, agents.review, agents.agents);
                        ui.add_space(GROUP_GAP);
                        group_title(ui, palette, "Notifications");
                        settings_card(ui, palette, |ui| {
                            setting_row(
                                ui,
                                palette,
                                "Completion notifications",
                                Some("Show a macOS banner when an agent finishes a turn"),
                                |ui| {
                                    if toggle_switch(ui, palette, notify_on_agent_completion) {
                                        action.agent_notify_changed = true;
                                    }
                                },
                            );
                        });
                    }
                    PreferencesSection::Phone => phone_section(ui, palette, phone, &mut action),
                    PreferencesSection::PullRequests => {
                        pull_requests_section(
                            ui,
                            palette,
                            bitbucket_email,
                            bitbucket_token,
                            pr_sources,
                            &mut action,
                        );
                    }
                    PreferencesSection::Project => match project.as_mut() {
                        Some(p) => {
                            settings_card(ui, palette, |ui| {
                                if worktree_base_row(
                                    ui,
                                    palette,
                                    p.worktree_base,
                                    p.base_hint,
                                    &mut action,
                                ) {
                                    action.project_changed = true;
                                }
                                setting_divider(ui, palette);
                                if run_command_row(
                                    ui,
                                    palette,
                                    "Run command",
                                    "Launched by the sidebar Run strip; empty auto-detects",
                                    "npm run dev",
                                    p.run_command,
                                ) {
                                    action.project_changed = true;
                                }
                                setting_divider(ui, palette);
                                if base_port_row(ui, palette, p.base_port) {
                                    action.project_changed = true;
                                }
                                setting_divider(ui, palette);
                                if post_create_row(ui, palette, p.post_create) {
                                    action.project_changed = true;
                                }
                            });
                        }
                        None => {
                            ui.label(
                                egui::RichText::new("Open a repository to configure it.")
                                    .size(LABEL_SIZE)
                                    .color(palette.text_muted),
                            );
                        }
                    },
                    PreferencesSection::Updates => {
                        updates_card(ui, palette, updates, &mut action);
                        ui.add_space(CARD_GAP);
                        settings_card(ui, palette, |ui| {
                            egui::Frame::new().inner_margin(CARD_PAD_X).show(ui, |ui| {
                                crate::ui::release_notes::body(ui, release_notes_cache);
                            });
                        });
                    }
                }
                ui.add_space(CONTENT_PAD_Y);
            });
        });
    action
}

/// Section title (preferences.md §3): ~24pt weight-500 `text.primary`, leading
/// the content column.
fn section_title(ui: &mut egui::Ui, palette: &Palette, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(TITLE_SIZE)
            .family(theme::medium_family(ui.ctx()))
            .color(palette.text_primary),
    );
}

const TITLE_DROPDOWN_GAP: f32 = 10.0;
const TITLE_DROPDOWN_ICON: f32 = 18.0;

/// Project picker rendered in place of the Project section's title
/// (preferences.md §4): the selected project's name at title size + a chevron,
/// opening a menu of every workspace project. Returns the newly picked index
/// when the selection changes — the app rescopes the section to it.
fn project_title_dropdown(
    ui: &mut egui::Ui,
    palette: &Palette,
    projects: &[String],
    selected: usize,
) -> Option<usize> {
    let label = projects
        .get(selected)
        .map(String::as_str)
        .unwrap_or("Project");
    let font = egui::FontId::new(TITLE_SIZE, theme::medium_family(ui.ctx()));
    let row = ui.ctx().fonts_mut(|f| f.row_height(&font));
    let text_width = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font.clone(), palette.text_primary)
        .size()
        .x;
    let size = egui::vec2(text_width + TITLE_DROPDOWN_GAP + TITLE_DROPDOWN_ICON, row);
    let (rect, response, hovered) = clickable(ui, size, true);
    let ink = if hovered {
        palette.accent
    } else {
        palette.text_primary
    };
    ui.painter().text(
        egui::pos2(rect.left(), rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        font,
        ink,
    );
    paint_icon(
        ui.painter(),
        egui::pos2(
            rect.left() + text_width + TITLE_DROPDOWN_GAP + TITLE_DROPDOWN_ICON / 2.0,
            rect.center().y,
        ),
        TITLE_DROPDOWN_ICON,
        lucide_icons::Icon::ChevronDown,
        if hovered {
            palette.accent
        } else {
            palette.text_secondary
        },
    );
    let mut picked = None;
    egui::Popup::menu(&response)
        .gap(DROPDOWN_POPUP_GAP)
        .style(theme::menu_style)
        .show(|ui| {
            for (index, name) in projects.iter().enumerate() {
                if ui.radio(index == selected, name).clicked() && index != selected {
                    picked = Some(index);
                }
            }
        });
    let info = label.to_owned();
    response.widget_info(move || egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &info));
    picked
}

/// Page nav row: Lucide icon + label, active `accent.subtle` + `accent`
/// (design-system §4).
fn nav_row(
    ui: &mut egui::Ui,
    palette: &Palette,
    icon: lucide_icons::Icon,
    label: &str,
    selected: bool,
) -> egui::Response {
    let size = egui::vec2(ui.available_width(), NAV_ROW_HEIGHT);
    let (rect, response, hovered) = clickable(ui, size, true);
    let fill = if selected {
        Some(palette.accent_subtle)
    } else if hovered {
        Some(palette.bg_surface_hover)
    } else {
        None
    };
    if let Some(fill) = fill {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(NAV_ROW_RADIUS), fill);
    }
    let color = if selected {
        palette.accent
    } else {
        palette.text_secondary
    };
    paint_icon(
        ui.painter(),
        egui::pos2(rect.left() + NAV_PAD + NAV_ICON_SIZE / 2.0, rect.center().y),
        NAV_ICON_SIZE,
        icon,
        color,
    );
    ui.painter().text(
        egui::pos2(rect.left() + NAV_PAD + NAV_ICON_SIZE + 8.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(NAV_LABEL_SIZE),
        color,
    );
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, label)
    });
    response
}

/// Segmented Auto/Light/Dark control (M7-4, design-system §6). Mutates `mode` on
/// click and returns `true` if the selection changed, so the caller applies and
/// persists the palette immediately (components re-read the tokens). The group
/// carries a single border with thin inner separators; the selected segment
/// detaches on top (`theme_segment`). The group is allocated as one block then
/// laid out left-to-right: `ui.horizontal` would inherit the right-to-left of
/// `setting_row`'s slot and reverse the order.
fn theme_segments(ui: &mut egui::Ui, palette: &Palette, mode: &mut ThemeMode) -> bool {
    let mut changed = false;
    let group = egui::vec2(SEGMENT_SIZE.x * MODES.len() as f32, SEGMENT_SIZE.y);
    let (rect, _) = ui.allocate_exact_size(group, egui::Sense::hover());
    ui.painter().rect(
        rect,
        egui::CornerRadius::same(RADIUS_PILL),
        palette.bg_surface,
        egui::Stroke::new(1.0_f32, palette.border_subtle),
        egui::StrokeKind::Inside,
    );
    for index in 1..MODES.len() {
        ui.painter().vline(
            rect.left() + SEGMENT_SIZE.x * index as f32,
            egui::Rangef::new(rect.top() + 1.0, rect.bottom() - 1.0),
            egui::Stroke::new(1.0_f32, palette.border_subtle),
        );
    }
    let mut segments = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    segments.spacing_mut().item_spacing.x = 0.0;
    for (index, (candidate, label, icon)) in MODES.iter().enumerate() {
        let position = SegmentPosition::at(index, MODES.len());
        if theme_segment(
            &mut segments,
            palette,
            label,
            *icon,
            *mode == *candidate,
            position,
        )
        .clicked()
            && *mode != *candidate
        {
            *mode = *candidate;
            changed = true;
        }
    }
    changed
}

/// Dropdown button of the setting rows: current value label + chevron, sized to
/// the text.
fn dropdown_button(ui: &mut egui::Ui, palette: &Palette, label: &str) -> egui::Response {
    let font = egui::FontId::proportional(SEGMENT_LABEL_SIZE);
    let text_width = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font.clone(), palette.text_primary)
        .size()
        .x;
    let size = egui::vec2(
        (DROPDOWN_PAD_X + text_width + DROPDOWN_GAP + DROPDOWN_ICON_SIZE + DROPDOWN_PAD_X)
            .max(DROPDOWN_MIN_WIDTH),
        SEGMENT_SIZE.y,
    );
    let (rect, response, hovered) = clickable(ui, size, true);
    let fill = if hovered {
        palette.bg_surface_hover
    } else {
        palette.bg_surface
    };
    ui.painter().rect(
        rect,
        egui::CornerRadius::same(RADIUS_PILL),
        fill,
        egui::Stroke::new(1.0_f32, palette.border_subtle),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        egui::pos2(rect.left() + DROPDOWN_PAD_X, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        font,
        palette.text_primary,
    );
    paint_icon(
        ui.painter(),
        egui::pos2(
            rect.right() - DROPDOWN_PAD_X - DROPDOWN_ICON_SIZE / 2.0,
            rect.center().y,
        ),
        DROPDOWN_ICON_SIZE,
        lucide_icons::Icon::ChevronDown,
        palette.text_secondary,
    );
    let label = label.to_owned();
    response.widget_info(move || egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    response
}

/// Pull-default dropdown (preferences.md §4): button labeled with the current
/// default + chevron, radio menu of the 4 operations (same domain labels as the
/// toolbar split-button, M12-6). Mutates `current` on selection and returns
/// `true` if the default changed — **never executes**.
fn pull_dropdown(ui: &mut egui::Ui, palette: &Palette, current: &mut PullDefault) -> bool {
    let response = dropdown_button(ui, palette, current.menu_label());
    let mut changed = false;
    egui::Popup::menu(&response)
        .gap(DROPDOWN_POPUP_GAP)
        .style(theme::menu_style)
        .show(|ui| {
            for option in PullDefault::ALL {
                if ui.radio(*current == option, option.menu_label()).clicked() && *current != option
                {
                    *current = option;
                    changed = true;
                }
            }
        });
    changed
}

/// Editor dropdown (terminal.md §12, preferences.md §4 Terminal): button labeled
/// with the current IDE + chevron, radio menu of the 3 supported IDEs
/// (`links::Editor`). Mutates `current` on selection and returns `true` if the IDE
/// changed — never opens anything.
fn editor_dropdown(ui: &mut egui::Ui, palette: &Palette, current: &mut Editor) -> bool {
    let response = dropdown_button(ui, palette, current.label());
    let mut changed = false;
    egui::Popup::menu(&response)
        .gap(DROPDOWN_POPUP_GAP)
        .style(theme::menu_style)
        .show(|ui| {
            for option in Editor::ALL {
                if ui.radio(*current == option, option.label()).clicked() && *current != option {
                    *current = option;
                    changed = true;
                }
            }
        });
    changed
}

/// Full-width Worktrees-base row (worktrees.md §6): label + description, then a
/// path field and a "Choose…" button that asks the app for the folder picker.
/// The placeholder shows the default base (`<root>.worktrees`). Returns `true`
/// when the field text changed.
fn worktree_base_row(
    ui: &mut egui::Ui,
    palette: &Palette,
    base: &mut String,
    hint: &str,
    action: &mut PreferencesAction,
) -> bool {
    let mut changed = false;
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(CARD_PAD_X as i8, 16))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = LABEL_GAP;
            ui.label(
                egui::RichText::new("Worktrees base")
                    .size(LABEL_SIZE)
                    .family(theme::medium_family(ui.ctx()))
                    .color(palette.text_primary),
            );
            ui.label(
                egui::RichText::new("Base folder new worktrees for this project are created under")
                    .size(DESCRIPTION_SIZE)
                    .color(palette.text_muted),
            );
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let field_w = (ui.available_width() - 96.0).max(120.0);
                let response = ui.add_sized(
                    [field_w, SEGMENT_SIZE.y],
                    tall_field(base).hint_text(egui::RichText::new(hint).color(palette.text_muted)),
                );
                changed = response.changed();
                if pill_button(ui, palette, "Choose…", true, false) {
                    action.pick_worktree_base = true;
                }
            });
        });
    changed
}

const POST_CREATE_ROWS: usize = 5;
const POST_CREATE_HINT: &str = "e.g. npm install && cp \"$HELM_PROJECT_ROOT/.env\" .";

/// Full-width Post-create script row (worktrees.md §6): a monospace bash field
/// run in the new worktree's first terminal. Returns `true` on every edit.
fn post_create_row(ui: &mut egui::Ui, palette: &Palette, text: &mut String) -> bool {
    let mut changed = false;
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(CARD_PAD_X as i8, 16))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = LABEL_GAP;
            ui.label(
                egui::RichText::new("Post-create script")
                    .size(LABEL_SIZE)
                    .family(theme::medium_family(ui.ctx()))
                    .color(palette.text_primary),
            );
            ui.label(
                egui::RichText::new("Bash run in the new worktree's terminal right after creation")
                    .size(DESCRIPTION_SIZE)
                    .color(palette.text_muted),
            );
            ui.add_space(4.0);
            let response = ui.add(
                egui::TextEdit::multiline(text)
                    .desired_rows(POST_CREATE_ROWS)
                    .desired_width(f32::INFINITY)
                    .font(egui::TextStyle::Monospace)
                    .hint_text(egui::RichText::new(POST_CREATE_HINT).color(palette.text_muted)),
            );
            changed = response.changed();
        });
    changed
}

/// Full-width labeled monospace singleline command row, shared by the project Run
/// command (git.md §3) and the agent commands (preferences.md §4). Returns `true`
/// on every edit.
fn run_command_row(
    ui: &mut egui::Ui,
    palette: &Palette,
    label: &str,
    description: &str,
    hint: &str,
    text: &mut String,
) -> bool {
    let mut changed = false;
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(CARD_PAD_X as i8, 16))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = LABEL_GAP;
            ui.label(
                egui::RichText::new(label)
                    .size(LABEL_SIZE)
                    .family(theme::medium_family(ui.ctx()))
                    .color(palette.text_primary),
            );
            ui.label(
                egui::RichText::new(description)
                    .size(DESCRIPTION_SIZE)
                    .color(palette.text_muted),
            );
            ui.add_space(4.0);
            let response = ui.add(
                egui::TextEdit::singleline(text)
                    .desired_width(f32::INFINITY)
                    .font(egui::TextStyle::Monospace)
                    .hint_text(egui::RichText::new(hint).color(palette.text_muted)),
            );
            changed = response.changed();
        });
    changed
}

const LAUNCH_ROW_GAP: f32 = 8.0;
const WARNING_ICON_SIZE: f32 = 12.0;

/// The agent settings the Agents section edits (preferences.md §4).
pub struct AgentsView<'a> {
    pub agents: &'a mut Vec<Agent>,
    pub commit_message: &'a mut CommitMessageSettings,
    pub review: &'a mut ReviewSettings,
}

fn open_agent_id() -> egui::Id {
    egui::Id::new("preferences-open-agent")
}

/// The agents table (preferences.md §4): one folded row per agent, a single one
/// open on its fields, then *Add agent*. A renamed agent keeps the commit
/// message and the review that chose it. Returns `true` on every edit, addition
/// or removal.
fn agents_card(ui: &mut egui::Ui, palette: &Palette, view: &mut AgentsView<'_>) -> bool {
    let mut changed = false;
    let mut removed = None;
    let mut open = ui
        .data(|data| data.get_temp::<Option<usize>>(open_agent_id()))
        .flatten();
    settings_card(ui, palette, |ui| {
        for (index, agent) in view.agents.iter_mut().enumerate() {
            if index > 0 {
                setting_divider(ui, palette);
            }
            let is_open = open == Some(index);
            let header = agent_header(ui, palette, agent, is_open);
            if header.removed {
                removed = Some(index);
            }
            if header.toggled {
                open = (!is_open).then_some(index);
            }
            if !is_open {
                continue;
            }
            let name_before = agent.name.clone();
            changed |= agent_fields(ui, palette, index, agent);
            if agent.name != name_before && !name_before.trim().is_empty() {
                for choice in [&mut view.commit_message.agent, &mut view.review.agent] {
                    if *choice == name_before {
                        choice.clone_from(&agent.name);
                    }
                }
            }
        }
        if !view.agents.is_empty() {
            setting_divider(ui, palette);
        }
        egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(CARD_PAD_X as i8, 12))
            .show(ui, |ui| {
                if let Some(agent) = add_agent_menu(ui, palette) {
                    view.agents.push(agent);
                    let added = view.agents.len() - 1;
                    open = Some(added);
                    // The new fields are drawn next frame: focusing an id absent from
                    // this one's accessibility tree panics accesskit.
                    ui.data_mut(|data| data.insert_temp(launch_focus_id(), added));
                    changed = true;
                }
            });
    });
    if let Some(index) = removed {
        view.agents.remove(index);
        open = None;
        changed = true;
    }
    ui.data_mut(|data| data.insert_temp(open_agent_id(), open));
    changed
}

const ADD_AGENT_LABEL: &str = "Add agent";
const EMPTY_AGENT_LABEL: &str = "Empty agent";

/// "Add agent" dropdown: a known agent, its three commands pre-filled, or an
/// empty block.
fn add_agent_menu(ui: &mut egui::Ui, palette: &Palette) -> Option<Agent> {
    let response = dropdown_button(ui, palette, ADD_AGENT_LABEL);
    let mut added = None;
    egui::Popup::menu(&response)
        .gap(DROPDOWN_POPUP_GAP)
        .style(theme::menu_style)
        .show(|ui| {
            for preset in Preset::ALL {
                if ui.button(preset.name()).clicked() {
                    added = Some(preset.agent());
                }
            }
            ui.separator();
            if ui.button(EMPTY_AGENT_LABEL).clicked() {
                added = Some(Agent::new("", ""));
            }
        });
    added
}

const TALL_FIELD_PAD_X: i8 = 10;

/// Single-line field given an explicit height: its text centered and inset,
/// where egui anchors it to the top-left corner.
fn tall_field(text: &mut String) -> egui::TextEdit<'_> {
    egui::TextEdit::singleline(text)
        .vertical_align(egui::Align::Center)
        .margin(egui::Margin::symmetric(TALL_FIELD_PAD_X, 0))
}

const PROMPT_ROWS: usize = 5;
const NO_AGENT_LABEL: &str = "Choose an agent";
const NO_ABLE_AGENT: &str = "No agent has the command this task needs";

/// Agent + prompt of the commit card's "Generate commit message" (git.md §5).
/// Returns `true` on every edit.
fn commit_message_card(
    ui: &mut egui::Ui,
    palette: &Palette,
    settings: &mut CommitMessageSettings,
    agents: &[Agent],
) -> bool {
    let mut changed = false;
    settings_card(ui, palette, |ui| {
        setting_row(
            ui,
            palette,
            "Commit message",
            Some("Written by the agent's Headless command"),
            |ui| {
                let able = agents.iter().filter(|a| a.can_write_commit_messages());
                changed |= agent_dropdown(ui, palette, &mut settings.agent, able);
            },
        );
        setting_divider(ui, palette);
        changed |= prompt_row(
            ui,
            palette,
            &PromptField {
                label: "Commit message prompt",
                description:
                    "{changes} is the staged files and diff; helm appends the reply format",
                default: &CommitMessageSettings::default().prompt,
            },
            &mut settings.prompt,
        );
    });
    changed
}

/// The agent the review notes and the PR review surface hand work to
/// (pull-requests.md §11). Returns `true` on every edit.
fn review_card(
    ui: &mut egui::Ui,
    palette: &Palette,
    settings: &mut ReviewSettings,
    agents: &[Agent],
) -> bool {
    let mut changed = false;
    let defaults = ReviewSettings::default();
    settings_card(ui, palette, |ui| {
        setting_row(
            ui,
            palette,
            "Review",
            Some("Opens the agent on your review comments or a pull request"),
            |ui| {
                let able = agents.iter().filter(|a| a.can_review());
                changed |= agent_dropdown(ui, palette, &mut settings.agent, able);
            },
        );
        setting_divider(ui, palette);
        changed |= prompt_row(
            ui,
            palette,
            &PromptField {
                label: "Review comments prompt",
                description:
                    "Send to, Ask on a thread: {comments} is the comments, grouped by file",
                default: &defaults.comments_prompt,
            },
            &mut settings.comments_prompt,
        );
        setting_divider(ui, palette);
        changed |= prompt_row(
            ui,
            palette,
            &PromptField {
                label: "Pull request prompt",
                description: "Ask on a pull request: {source} {dest} {title} {number}",
                default: &defaults.pr_prompt,
            },
            &mut settings.pr_prompt,
        );
    });
    changed
}

/// Dropdown choosing one agent of the table among those `able` to do the job;
/// a choice that left the table, or lost its command, reads as unset.
fn agent_dropdown<'a>(
    ui: &mut egui::Ui,
    palette: &Palette,
    chosen: &mut String,
    able: impl Iterator<Item = &'a Agent>,
) -> bool {
    let able: Vec<&str> = able.map(|agent| agent.name.as_str()).collect();
    let label = if able.contains(&chosen.as_str()) {
        chosen.as_str()
    } else {
        NO_AGENT_LABEL
    };
    let response = dropdown_button(ui, palette, label);
    let mut changed = false;
    egui::Popup::menu(&response)
        .gap(DROPDOWN_POPUP_GAP)
        .style(theme::menu_style)
        .show(|ui| {
            // A menu first sized empty keeps that width: its choices must not wrap into it.
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
            if able.is_empty() {
                ui.label(egui::RichText::new(NO_ABLE_AGENT).color(palette.text_muted));
            }
            for name in able {
                if ui.radio(chosen.as_str() == name, name).clicked() && chosen.as_str() != name {
                    *chosen = name.to_owned();
                    changed = true;
                }
            }
        });
    changed
}

struct PromptField<'a> {
    label: &'a str,
    description: &'a str,
    default: &'a str,
}

/// Folded prompt row saying whether the text deviates; open, its description, a
/// multiline field, and *Restore default* once it deviates. Returns `true` on
/// every edit.
fn prompt_row(
    ui: &mut egui::Ui,
    palette: &Palette,
    field: &PromptField<'_>,
    text: &mut String,
) -> bool {
    let open_id = egui::Id::new(("preferences-prompt", field.label));
    let mut open = ui.data(|data| data.get_temp::<bool>(open_id)) == Some(true);
    let deviates = text.as_str() != field.default;
    let header = Disclosure {
        title: field.label,
        detail: if deviates { "Edited" } else { "Default" },
        detail_font: egui::FontId::proportional(DESCRIPTION_SIZE),
        open,
    };
    if disclosure_row(ui, palette, &header).clicked() {
        open = !open;
        ui.data_mut(|data| data.insert_temp(open_id, open));
    }
    if !open {
        return false;
    }
    let mut changed = false;
    disclosure_body(ui, |ui| {
        ui.spacing_mut().item_spacing.y = LABEL_GAP;
        ui.label(
            egui::RichText::new(field.description)
                .size(DESCRIPTION_SIZE)
                .color(palette.text_muted),
        );
        ui.add_space(4.0);
        let response = ui.add(
            egui::TextEdit::multiline(text)
                .desired_rows(PROMPT_ROWS)
                .desired_width(f32::INFINITY),
        );
        changed = response.changed();
        if deviates {
            ui.add_space(4.0);
            if pill_button(ui, palette, "Restore default", true, false) {
                *text = field.default.to_owned();
                changed = true;
            }
        }
    });
    changed
}

const DISCLOSURE_ROW_HEIGHT: f32 = 44.0;
const DISCLOSURE_ICON_SIZE: f32 = 14.0;
const DISCLOSURE_GAP: f32 = 8.0;
const DISCLOSURE_INDENT: f32 = CARD_PAD_X + DISCLOSURE_ICON_SIZE + DISCLOSURE_GAP;

struct Disclosure<'a> {
    title: &'a str,
    detail: &'a str,
    detail_font: egui::FontId,
    open: bool,
}

/// Foldable row of a card: chevron, title, then a muted detail. The caller owns
/// the open state and draws the body ([`disclosure_body`]).
fn disclosure_row(ui: &mut egui::Ui, palette: &Palette, row: &Disclosure<'_>) -> egui::Response {
    let size = egui::vec2(ui.available_width(), DISCLOSURE_ROW_HEIGHT);
    let (rect, response, hovered) = clickable(ui, size, true);
    let icon = if row.open {
        lucide_icons::Icon::ChevronDown
    } else {
        lucide_icons::Icon::ChevronRight
    };
    let chevron_ink = if hovered {
        palette.text_primary
    } else {
        palette.text_secondary
    };
    let middle = rect.center().y;
    paint_icon(
        ui.painter(),
        egui::pos2(
            rect.left() + CARD_PAD_X + DISCLOSURE_ICON_SIZE / 2.0,
            middle,
        ),
        DISCLOSURE_ICON_SIZE,
        icon,
        chevron_ink,
    );
    let text_right = rect.right() - CARD_PAD_X - AFFORDANCE_SIZE - DISCLOSURE_GAP;
    let painter = ui.painter().with_clip_rect(rect.with_max_x(text_right));
    let title = painter.text(
        egui::pos2(rect.left() + DISCLOSURE_INDENT, middle),
        egui::Align2::LEFT_CENTER,
        row.title,
        egui::FontId::new(LABEL_SIZE, theme::medium_family(ui.ctx())),
        palette.text_primary,
    );
    painter.text(
        egui::pos2(title.right() + DISCLOSURE_GAP + 2.0, middle),
        egui::Align2::LEFT_CENTER,
        row.detail,
        row.detail_font.clone(),
        palette.text_muted,
    );
    let label = if row.detail.is_empty() {
        row.title.to_owned()
    } else {
        format!("{}, {}", row.title, row.detail)
    };
    response.widget_info(move || egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    response
}

/// Body of an open [`disclosure_row`], aligned under its title.
fn disclosure_body(ui: &mut egui::Ui, contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: DISCLOSURE_INDENT as i8,
            right: CARD_PAD_X as i8,
            top: 0,
            bottom: 16,
        })
        .show(ui, contents);
}

fn phone_section(
    ui: &mut egui::Ui,
    palette: &Palette,
    phone: PhoneView<'_>,
    action: &mut PreferencesAction,
) {
    settings_card(ui, palette, |ui| {
        setting_row(
            ui,
            palette,
            "Start at launch",
            Some("Turn phone access on by itself on a Wi-Fi where a phone was paired"),
            |ui| {
                if toggle_switch(ui, palette, phone.start_at_launch) {
                    action.phone_at_launch_changed = true;
                }
            },
        );
    });
    ui.add_space(CARD_GAP);
    settings_card(ui, palette, |ui| {
        card_header(
            ui,
            palette,
            "Paired devices",
            "Phones that can reach your agents without scanning again",
        );
        if phone.devices.is_empty() {
            setting_divider(ui, palette);
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(CARD_PAD_X as i8, 16))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(NO_PAIRED_DEVICE)
                            .size(DESCRIPTION_SIZE)
                            .color(palette.text_muted),
                    );
                });
        }
        for (index, device) in phone.devices.iter().enumerate() {
            setting_divider(ui, palette);
            setting_row(ui, palette, &device.name, Some(&device.detail), |ui| {
                if pill_button(ui, palette, "Revoke", true, false) {
                    action.revoke_device = Some(index);
                }
            });
        }
    });
    if !phone.devices.is_empty() {
        ui.add_space(LAUNCH_ROW_GAP);
        if pill_button(ui, palette, "Revoke all", true, false) {
            action.revoke_all_devices = true;
        }
    }
}

pub const NO_PAIRED_DEVICE: &str = "No paired phone — use Open on phone from the palette";

/// Title + description block opening a card whose rows follow.
fn card_header(ui: &mut egui::Ui, palette: &Palette, title: &str, description: &str) {
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(CARD_PAD_X as i8, 16))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = LABEL_GAP;
            ui.label(
                egui::RichText::new(title)
                    .size(LABEL_SIZE)
                    .family(theme::medium_family(ui.ctx()))
                    .color(palette.text_primary),
            );
            ui.label(
                egui::RichText::new(description)
                    .size(DESCRIPTION_SIZE)
                    .color(palette.text_muted),
            );
        });
}

struct AgentHeaderOutcome {
    toggled: bool,
    removed: bool,
}

fn launch_focus_id() -> egui::Id {
    egui::Id::new("launch-agent-focus")
}

const UNNAMED_AGENT: &str = "New agent";

fn undetected_program(agent: &Agent) -> Option<&str> {
    agent.program().filter(|_| !agent.is_detected())
}

/// Folded row of an agent: its name and its start command; open, the trash.
fn agent_header(
    ui: &mut egui::Ui,
    palette: &Palette,
    agent: &Agent,
    open: bool,
) -> AgentHeaderOutcome {
    let title = if agent.name.trim().is_empty() {
        UNNAMED_AGENT
    } else {
        agent.name.as_str()
    };
    let header = Disclosure {
        title,
        detail: &agent.command,
        detail_font: egui::FontId::monospace(DESCRIPTION_SIZE),
        open,
    };
    let response = disclosure_row(ui, palette, &header);
    let mut slot = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(response.rect.shrink2(egui::vec2(CARD_PAD_X, 0.0)))
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    let mut removed = false;
    if open {
        let label = format!("Remove agent {}", agent.name);
        removed = affordance(&mut slot, palette, lucide_icons::Icon::Trash2, &label);
    } else if undetected_program(agent).is_some() {
        warning_icon(&mut slot, palette);
    }
    AgentHeaderOutcome {
        toggled: response.clicked(),
        removed,
    }
}

fn warning_icon(ui: &mut egui::Ui, palette: &Palette) {
    let (rect, _) =
        ui.allocate_exact_size(egui::Vec2::splat(WARNING_ICON_SIZE), egui::Sense::hover());
    paint_icon(
        ui.painter(),
        rect.center(),
        WARNING_ICON_SIZE,
        lucide_icons::Icon::AlertTriangle,
        palette.text_muted,
    );
}

/// Fields of the open agent: its name, then one command line per use. Returns
/// `true` on every edit.
fn agent_fields(ui: &mut egui::Ui, palette: &Palette, index: usize, agent: &mut Agent) -> bool {
    let mut edited = false;
    disclosure_body(ui, |ui| {
        let name = agent_field(
            ui,
            palette,
            &AgentField {
                label: "Name",
                usage: "",
                hint: "Name",
                font: egui::TextStyle::Body,
            },
            &mut agent.name,
        );
        if ui.data(|data| data.get_temp::<usize>(launch_focus_id())) == Some(index) {
            ui.data_mut(|data| data.remove::<usize>(launch_focus_id()));
            name.request_focus();
        }
        edited = name.changed();
        let hints = Preset::ClaudeCode.agent();
        for (label, usage, hint, command) in [
            (
                "Start",
                "From the phone",
                &hints.command,
                &mut agent.command,
            ),
            (
                "With a prompt",
                "Review",
                &hints.prompt_command,
                &mut agent.prompt_command,
            ),
            (
                "Headless",
                "Commit message",
                &hints.headless_command,
                &mut agent.headless_command,
            ),
        ] {
            let field = AgentField {
                label,
                usage,
                hint,
                font: egui::TextStyle::Monospace,
            };
            edited |= agent_field(ui, palette, &field, command).changed();
        }
        ui.add_space(LAUNCH_ROW_GAP);
        ui.spacing_mut().item_spacing.y = LABEL_GAP;
        if let Some(program) = undetected_program(agent) {
            ui.horizontal(|ui| {
                warning_icon(ui, palette);
                ui.label(
                    egui::RichText::new(format!(
                        "helm won't detect {program} as an agent — the phone won't list it"
                    ))
                    .size(DESCRIPTION_SIZE)
                    .color(palette.text_muted),
                );
            });
        }
        ui.label(
            egui::RichText::new(format!(
                "Commands run in your login shell; ${PROMPT_ENV} is the prompt. Headless \
                 answers once and exits. A blank command turns that use off"
            ))
            .size(DESCRIPTION_SIZE)
            .color(palette.text_muted),
        );
    });
    edited
}

const AGENT_LABEL_WIDTH: f32 = 132.0;
const AGENT_FIELD_HEIGHT: f32 = 28.0;
const AGENT_FIELD_ROW_HEIGHT: f32 = 42.0;
const AGENT_USAGE_SIZE: f32 = 11.0;

struct AgentField<'a> {
    label: &'a str,
    usage: &'a str,
    hint: &'a str,
    font: egui::TextStyle,
}

/// One field of the open agent: its label over what helm uses it for, the field
/// beside them.
fn agent_field(
    ui: &mut egui::Ui,
    palette: &Palette,
    field: &AgentField<'_>,
    text: &mut String,
) -> egui::Response {
    let size = egui::vec2(ui.available_width(), AGENT_FIELD_ROW_HEIGHT);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let label_font = egui::FontId::proportional(DESCRIPTION_SIZE);
    let label_anchor = if field.usage.is_empty() {
        egui::Align2::LEFT_CENTER
    } else {
        egui::Align2::LEFT_BOTTOM
    };
    ui.painter().text(
        rect.left_center(),
        label_anchor,
        field.label,
        label_font,
        palette.text_primary,
    );
    ui.painter().text(
        rect.left_center() + egui::vec2(0.0, 1.0),
        egui::Align2::LEFT_TOP,
        field.usage,
        egui::FontId::proportional(AGENT_USAGE_SIZE),
        palette.text_muted,
    );
    let field_rect =
        egui::Rect::from_center_size(rect.center(), egui::vec2(rect.width(), AGENT_FIELD_HEIGHT))
            .with_min_x(rect.left() + AGENT_LABEL_WIDTH);
    ui.put(
        field_rect,
        tall_field(text).font(field.font.clone()).hint_text(
            egui::RichText::new(field.hint)
                .text_style(field.font.clone())
                .color(palette.text_muted),
        ),
    )
}

/// Pull Requests section (pull-requests.md §3): GitHub authenticates through the
/// `gh` CLI (no stored secret — a read-only status line), Bitbucket needs the
/// account email plus a token kept in the macOS Keychain.
fn pull_requests_section(
    ui: &mut egui::Ui,
    palette: &Palette,
    email: &mut String,
    token: &mut String,
    sources: &PrSourcesView,
    action: &mut PreferencesAction,
) {
    settings_card(ui, palette, |ui| {
        setting_row(
            ui,
            palette,
            "GitHub",
            Some("Pull requests are read through the gh CLI"),
            |ui| source_status(ui, palette, &sources.github, sources.loaded),
        );
    });
    ui.add_space(CARD_GAP);
    settings_card(ui, palette, |ui| {
        setting_row(
            ui,
            palette,
            "Bitbucket",
            Some("Connection status of the Bitbucket source"),
            |ui| source_status(ui, palette, &sources.bitbucket, sources.loaded),
        );
        setting_divider(ui, palette);
        if bitbucket_email_row(ui, palette, email) {
            action.bitbucket_email_changed = true;
        }
        setting_divider(ui, palette);
        if bitbucket_token_row(ui, palette, token) {
            action.save_bitbucket_token = true;
        }
    });
}

/// Inline usability of one PR source in a setting row's right slot: the last
/// fetch's status, or "Checking…" until the first reply lands.
fn source_status(ui: &mut egui::Ui, palette: &Palette, status: &SourceStatus, loaded: bool) {
    if !loaded {
        inline_status(ui, "Checking…", palette.text_secondary);
        return;
    }
    match status {
        SourceStatus::Ok => {
            inline_status(ui, "Connected", palette.text_secondary);
        }
        SourceStatus::Unavailable(hint) => {
            inline_status(ui, hint, palette.git_deleted).on_hover_text(hint);
        }
        SourceStatus::Absent => {
            inline_status(ui, "No repository in your workspace", palette.text_muted);
        }
    }
}

/// Full-width Bitbucket email row (pull-requests.md §3): the non-secret account
/// email, persisted in `prefs`. Returns `true` on every edit.
fn bitbucket_email_row(ui: &mut egui::Ui, palette: &Palette, email: &mut String) -> bool {
    let mut changed = false;
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(CARD_PAD_X as i8, 16))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = LABEL_GAP;
            ui.label(
                egui::RichText::new("Email")
                    .size(LABEL_SIZE)
                    .family(theme::medium_family(ui.ctx()))
                    .color(palette.text_primary),
            );
            ui.label(
                egui::RichText::new("Bitbucket account email used for Basic auth")
                    .size(DESCRIPTION_SIZE)
                    .color(palette.text_muted),
            );
            ui.add_space(4.0);
            let response = ui.add(
                egui::TextEdit::singleline(email)
                    .desired_width(f32::INFINITY)
                    .hint_text(egui::RichText::new("you@example.com").color(palette.text_muted)),
            );
            changed = response.changed();
        });
    changed
}

/// Full-width Bitbucket token row (pull-requests.md §3): a masked field plus a
/// "Save" button that stores the token in the Keychain (never in `prefs`).
/// Returns `true` when Save is clicked.
fn bitbucket_token_row(ui: &mut egui::Ui, palette: &Palette, token: &mut String) -> bool {
    let mut save = false;
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(CARD_PAD_X as i8, 16))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = LABEL_GAP;
            ui.label(
                egui::RichText::new("API token")
                    .size(LABEL_SIZE)
                    .family(theme::medium_family(ui.ctx()))
                    .color(palette.text_primary),
            );
            ui.label(
                egui::RichText::new("Stored in the macOS Keychain, never written to disk")
                    .size(DESCRIPTION_SIZE)
                    .color(palette.text_muted),
            );
            ui.label(
                egui::RichText::new("Needs read scopes: Account, Repositories, Pull requests")
                    .size(DESCRIPTION_SIZE)
                    .color(palette.text_muted),
            );
            ui.hyperlink_to(
                egui::RichText::new("Create a Bitbucket API token ↗")
                    .size(DESCRIPTION_SIZE)
                    .color(palette.accent),
                "https://id.atlassian.com/manage-profile/security/api-tokens",
            );
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let field_w = (ui.available_width() - 80.0).max(120.0);
                ui.add_sized(
                    [field_w, SEGMENT_SIZE.y],
                    tall_field(token).password(true).hint_text(
                        egui::RichText::new("Bitbucket API token").color(palette.text_muted),
                    ),
                );
                if pill_button(ui, palette, "Save", !token.trim().is_empty(), true) {
                    save = true;
                }
            });
        });
    save
}

/// Base port row (git.md §3): the project's first `$PORT`; each worktree adds its
/// group offset on top, overridable per worktree from the Run strip. Empty falls
/// back to 3000. Returns `true` on every edit.
fn base_port_row(ui: &mut egui::Ui, palette: &Palette, text: &mut String) -> bool {
    let mut changed = false;
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(CARD_PAD_X as i8, 16))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = LABEL_GAP;
            ui.label(
                egui::RichText::new("Base port")
                    .size(LABEL_SIZE)
                    .family(theme::medium_family(ui.ctx()))
                    .color(palette.text_primary),
            );
            ui.label(
                egui::RichText::new("First $PORT; each worktree counts up from here")
                    .size(DESCRIPTION_SIZE)
                    .color(palette.text_muted),
            );
            ui.add_space(4.0);
            let response = ui.add(
                egui::TextEdit::singleline(text)
                    .desired_width(96.0)
                    .font(egui::TextStyle::Monospace)
                    .hint_text(egui::RichText::new("3000").color(palette.text_muted)),
            );
            changed = response.changed();
        });
    changed
}

const GROUPS: [Group; 3] = [Group::Global, Group::Terminal, Group::Git];
const GROUP_GAP: f32 = 24.0;
const GROUP_TITLE_GAP: f32 = 8.0;
const KEYCAP_HEIGHT: f32 = 26.0;
const KEYCAP_PAD_X: f32 = 10.0;
const KEYCAP_RADIUS: u8 = 6;
const RECORDING_LABEL: &str = "Press shortcut…";
const UNBOUND_LABEL: &str = "unbound";
const AFFORDANCE_SIZE: f32 = 24.0;
const AFFORDANCE_ICON_SIZE: f32 = 13.0;

fn action_description(action: Action) -> &'static str {
    match action {
        Action::OpenFolder => "Import a project folder into the workspace",
        Action::NewTab => "Open a new terminal tab",
        Action::TogglePreferences => "Open or close this page",
        Action::ToggleWorkspaceSidebar => "Show or hide the projects sidebar",
        Action::ToggleGitSidebar => "Show or hide the git sidebar",
        Action::ToggleGraph => "Switch the central zone between Terminal and Git",
        Action::NextRepo => "Switch to the next repo or worktree in the sidebar",
        Action::PrevRepo => "Switch to the previous repo or worktree in the sidebar",
        Action::SplitRight => "Split the focused pane to the right",
        Action::SplitDown => "Split the focused pane downward",
        Action::ClosePane => "Close the focused pane, or the tab when alone",
        Action::FocusLeft => "Move focus to the pane on the left",
        Action::FocusRight => "Move focus to the pane on the right",
        Action::FocusUp => "Move focus to the pane above",
        Action::FocusDown => "Move focus to the pane below",
        Action::ResizeLeft => "Move the focused split's seam to the left",
        Action::ResizeRight => "Move the focused split's seam to the right",
        Action::ResizeUp => "Move the focused split's seam up",
        Action::ResizeDown => "Move the focused split's seam down",
        Action::ZoomIn => "Increase the terminal font size",
        Action::ZoomOut => "Decrease the terminal font size",
        Action::ZoomReset => "Restore the default terminal font size",
        Action::ClearTerminal => "Clear the focused terminal's screen",
        Action::Commit => "Commit the staged changes",
        Action::Run => "Run the active project, or relaunch it if already running",
        Action::FocusFinishedAgent => "Jump to the first finished agent listed under Agents",
    }
}

/// Keyboard section (preferences.md §4, keybindings.md §6): Restore defaults,
/// then one card per group with a keycap row per rebindable action.
fn keyboard_section(
    ui: &mut egui::Ui,
    palette: &Palette,
    keymap: &mut Keymap,
    state: &mut KeyboardState,
    action: &mut PreferencesAction,
) {
    let deviates = Action::ALL.iter().any(|a| keymap.deviates(*a));
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
        if pill_button(ui, palette, "Restore defaults", deviates, false) {
            keymap.restore_defaults();
            *state = KeyboardState::default();
            action.keymap_changed = true;
        }
    });
    ui.add_space(GROUP_TITLE_GAP);
    for (index, group) in GROUPS.into_iter().enumerate() {
        if index > 0 {
            ui.add_space(GROUP_GAP);
        }
        group_title(ui, palette, group.label());
        settings_card(ui, palette, |ui| {
            let mut first = true;
            for entry in Action::ALL.into_iter().filter(|a| a.group() == group) {
                if !first {
                    setting_divider(ui, palette);
                }
                first = false;
                binding_row(ui, palette, entry, keymap, state, action);
            }
        });
    }
}

/// Title of a group of cards inside a section.
fn group_title(ui: &mut egui::Ui, palette: &Palette, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(LABEL_SIZE)
            .family(theme::medium_family(ui.ctx()))
            .color(palette.text_secondary),
    );
    ui.add_space(GROUP_TITLE_GAP);
}

/// One rebindable action row: label + description, the binding as a keycap in
/// the slot, hover reset/unbind when deviating, recorder + inline error when
/// armed.
fn binding_row(
    ui: &mut egui::Ui,
    palette: &Palette,
    entry: Action,
    keymap: &mut Keymap,
    state: &mut KeyboardState,
    action: &mut PreferencesAction,
) {
    setting_row(
        ui,
        palette,
        entry.label(),
        Some(action_description(entry)),
        |ui| {
            if state.recording == Some(entry) {
                let badge = keycap(ui, palette, RECORDING_LABEL, false, true);
                if let Some((_, message)) = state.error.as_ref().filter(|(a, _)| *a == entry) {
                    inline_status(ui, message, palette.git_deleted);
                }
                record_capture(ui, entry, badge.rect, keymap, state, action);
                return;
            }
            let current = keymap.shortcut_for(entry);
            let label = current.map(|s| s.display());
            let badge = match &label {
                Some(text) => keycap(ui, palette, text, false, false),
                None => keycap(ui, palette, UNBOUND_LABEL, true, false),
            };
            if badge.clicked() {
                state.recording = Some(entry);
                state.error = None;
            }
            // Hover-revealed deviation affordances (preferences.md §4), like the
            // other per-row secondary controls.
            if keymap.deviates(entry) && ui.rect_contains_pointer(ui.max_rect()) {
                if affordance(
                    ui,
                    palette,
                    lucide_icons::Icon::RotateCcw,
                    &format!("Reset {}", entry.label()),
                ) {
                    keymap.reset(entry);
                    action.keymap_changed = true;
                }
                if current.is_some()
                    && affordance(
                        ui,
                        palette,
                        lucide_icons::Icon::X,
                        &format!("Unbind {}", entry.label()),
                    )
                {
                    keymap.set(entry, None);
                    action.keymap_changed = true;
                }
            }
        },
    );
}

/// Drains this frame's keydowns for the armed row (preferences.md §4): `Esc`
/// cancels, `Backspace`/`Delete` unbinds, anything else is validated
/// (keybindings.md §6 — modifiers required, reserved and conflicting combos
/// refused with the holder named) and applied. A click outside the badge
/// cancels.
fn record_capture(
    ui: &egui::Ui,
    entry: Action,
    badge: egui::Rect,
    keymap: &mut Keymap,
    state: &mut KeyboardState,
    action: &mut PreferencesAction,
) {
    let pressed = ui.input(|i| {
        i.events.iter().find_map(|event| match event {
            egui::Event::Key {
                key,
                pressed: true,
                repeat: false,
                modifiers,
                ..
            } => Some((*key, *modifiers)),
            _ => None,
        })
    });
    if let Some((key, modifiers)) = pressed {
        match key {
            egui::Key::Escape => *state = KeyboardState::default(),
            egui::Key::Backspace | egui::Key::Delete => {
                if keymap.shortcut_for(entry).is_some() {
                    keymap.set(entry, None);
                    action.keymap_changed = true;
                }
                *state = KeyboardState::default();
            }
            _ => {
                let shortcut = Shortcut {
                    cmd: modifiers.command,
                    ctrl: modifiers.ctrl,
                    alt: modifiers.alt,
                    shift: modifiers.shift,
                    key,
                };
                let holder = keymap.holder_of(shortcut).filter(|h| *h != entry);
                if shortcut.is_reserved() {
                    let message = if shortcut.cmd || shortcut.ctrl || shortcut.alt {
                        "Reserved shortcut".to_owned()
                    } else {
                        "Add Cmd, Ctrl or Alt".to_owned()
                    };
                    state.error = Some((entry, message));
                } else if let Some(holder) = holder {
                    state.error = Some((entry, format!("Already used by {}", holder.label())));
                } else {
                    if keymap.shortcut_for(entry) != Some(shortcut) {
                        keymap.set(entry, Some(shortcut));
                        action.keymap_changed = true;
                    }
                    *state = KeyboardState::default();
                }
            }
        }
        return;
    }
    let clicked_away = ui.input(|i| {
        i.pointer.any_pressed()
            && i.pointer
                .interact_pos()
                .is_some_and(|pos| !badge.contains(pos))
    });
    if clicked_away {
        *state = KeyboardState::default();
    }
}

/// Keycap badge of a binding row: clickable, `unbound` muted, armed accented.
fn keycap(
    ui: &mut egui::Ui,
    palette: &Palette,
    text: &str,
    muted: bool,
    armed: bool,
) -> egui::Response {
    let font = egui::FontId::proportional(SEGMENT_LABEL_SIZE);
    let text_width = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font.clone(), palette.text_primary)
        .size()
        .x;
    let size = egui::vec2(KEYCAP_PAD_X * 2.0 + text_width, KEYCAP_HEIGHT);
    let (rect, response, hovered) = clickable(ui, size, true);
    let (border, ink) = if armed {
        (palette.accent, palette.accent)
    } else if muted {
        (palette.border_subtle, palette.text_muted)
    } else {
        (palette.border_subtle, palette.text_primary)
    };
    let fill = if hovered && !armed {
        palette.bg_surface_hover
    } else {
        palette.bg_surface
    };
    ui.painter().rect(
        rect,
        egui::CornerRadius::same(KEYCAP_RADIUS),
        fill,
        egui::Stroke::new(1.0_f32, border),
        egui::StrokeKind::Inside,
    );
    ui.painter()
        .text(rect.center(), egui::Align2::CENTER_CENTER, text, font, ink);
    let text = text.to_owned();
    response.widget_info(move || egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &text));
    response
}

/// Small hover affordance of a deviating row (reset / unbind): icon-only button.
fn affordance(ui: &mut egui::Ui, palette: &Palette, icon: lucide_icons::Icon, label: &str) -> bool {
    let (rect, response, hovered) = clickable(ui, egui::Vec2::splat(AFFORDANCE_SIZE), true);
    if hovered {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(KEYCAP_RADIUS),
            palette.bg_surface_hover,
        );
    }
    paint_icon(
        ui.painter(),
        rect.center(),
        AFFORDANCE_ICON_SIZE,
        icon,
        palette.text_secondary,
    );
    let label = label.to_owned();
    response.widget_info(move || egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    response.clicked()
}

const DEV_MODE_NOTE: &str = "Running outside an app bundle — updates disabled";
const SPINNER_SIZE: f32 = 14.0;

/// Shell-command row of the Terminal section (specs/cli.md §7): installs the
/// `helm` symlink into the PATH. Raises an intent — the app writes the link and
/// reports the outcome by toast.
fn shell_command_row(
    ui: &mut egui::Ui,
    palette: &Palette,
    state: &crate::cli::ShellCommand,
    action: &mut PreferencesAction,
) {
    setting_row(
        ui,
        palette,
        "Shell command",
        Some("Run helm <path> in a terminal to open a repository or worktree"),
        |ui| match state {
            crate::cli::ShellCommand::Unbundled => {
                inline_status(ui, DEV_MODE_NOTE, palette.text_secondary);
            }
            crate::cli::ShellCommand::Installed => {
                inline_status(
                    ui,
                    &format!("Installed in {}", crate::cli::SHELL_COMMAND_DIR),
                    palette.text_secondary,
                );
            }
            crate::cli::ShellCommand::Missing => {
                if pill_button(ui, palette, "Install", true, true) {
                    action.install_shell_command = true;
                }
            }
            crate::cli::ShellCommand::Foreign => {
                if pill_button(ui, palette, "Replace", true, true) {
                    action.install_shell_command = true;
                }
                inline_status(ui, "Another helm is on the PATH", palette.text_secondary);
            }
        },
    );
}

/// Updates card (update.md §6): Version row + Check for updates row whose right
/// slot renders the inline updater state. Busy (check or install in progress)
/// disables the check button; Install & Relaunch only exists in the Available
/// state. Outside a bundle the row carries the dev-mode note, without controls.
fn updates_card(
    ui: &mut egui::Ui,
    palette: &Palette,
    updates: &UpdatesView,
    action: &mut PreferencesAction,
) {
    settings_card(ui, palette, |ui| {
        setting_row(
            ui,
            palette,
            "Version",
            Some("Installed version of Helm"),
            |ui| {
                ui.label(
                    egui::RichText::new(format!("v{}", updates.version))
                        .size(SEGMENT_LABEL_SIZE)
                        .color(palette.text_secondary),
                );
            },
        );
        setting_divider(ui, palette);
        if !updates.bundled {
            setting_row(
                ui,
                palette,
                "Check for updates",
                Some(DEV_MODE_NOTE),
                |_ui| {},
            );
            return;
        }
        setting_row(
            ui,
            palette,
            "Check for updates",
            Some("Latest release from GitHub"),
            |ui| {
                // Right-to-left slot: the check button stays rightmost, the
                // inline state grows to its left.
                let busy = matches!(
                    updates.state,
                    UpdateState::Checking | UpdateState::Downloading | UpdateState::Installing
                );
                if pill_button(ui, palette, "Check now", !busy, false) {
                    action.check_updates = true;
                }
                match &updates.state {
                    UpdateState::Idle => {}
                    UpdateState::Checking => inline_progress(ui, palette, "Checking…"),
                    UpdateState::UpToDate => {
                        inline_status(ui, "Up to date", palette.text_secondary);
                    }
                    UpdateState::Available { version, .. } => {
                        if pill_button(ui, palette, "Install & Relaunch", true, true) {
                            action.install_update = true;
                        }
                        inline_status(ui, &format!("New version v{version}"), palette.text_primary);
                    }
                    UpdateState::Downloading => inline_progress(ui, palette, "Downloading…"),
                    UpdateState::Installing => inline_progress(ui, palette, "Installing…"),
                    UpdateState::Error(message) => {
                        inline_status(ui, message, palette.git_deleted);
                    }
                }
            },
        );
    });
}

/// Spinner + label pair of the slot (right-to-left: label first keeps the
/// spinner on its left).
fn inline_progress(ui: &mut egui::Ui, palette: &Palette, label: &str) {
    inline_status(ui, label, palette.text_secondary);
    ui.add(Spinner::new().size(SPINNER_SIZE).color(palette.accent));
}

/// Inline updater result, truncated to the slot's remaining width.
fn inline_status(ui: &mut egui::Ui, text: &str, color: egui::Color32) -> egui::Response {
    ui.add(
        egui::Label::new(
            egui::RichText::new(text)
                .size(SEGMENT_LABEL_SIZE)
                .color(color),
        )
        .truncate(),
    )
}

/// Pill action button of the setting rows, sized to its label. `primary` paints
/// it with the accent (the row's main action); disabled ⇒ muted label, no click.
fn pill_button(
    ui: &mut egui::Ui,
    palette: &Palette,
    label: &str,
    enabled: bool,
    primary: bool,
) -> bool {
    let font = egui::FontId::proportional(SEGMENT_LABEL_SIZE);
    let text_width = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font.clone(), palette.text_primary)
        .size()
        .x;
    let size = egui::vec2(DROPDOWN_PAD_X * 2.0 + text_width, SEGMENT_SIZE.y);
    let (rect, response, hovered) = clickable(ui, size, enabled);
    let (fill, border, ink) = if primary {
        (palette.accent_subtle, palette.accent, palette.accent)
    } else {
        (
            if hovered {
                palette.bg_surface_hover
            } else {
                palette.bg_surface
            },
            palette.border_subtle,
            if enabled {
                palette.text_primary
            } else {
                palette.text_muted
            },
        )
    };
    ui.painter().rect(
        rect,
        egui::CornerRadius::same(RADIUS_PILL),
        fill,
        egui::Stroke::new(1.0_f32, border),
        egui::StrokeKind::Inside,
    );
    ui.painter()
        .text(rect.center(), egui::Align2::CENTER_CENTER, label, font, ink);
    let label = label.to_owned();
    response
        .widget_info(move || egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, &label));
    response.clicked()
}

/// Theme-family dropdown (preferences.md §4): button with the name of the
/// variant's current preset, radio menu of that variant's presets. Mutates the
/// id on selection and returns `true` if the family changed — an unknown id
/// displays resolved to Helm (same fallback as theme application).
fn preset_dropdown(
    ui: &mut egui::Ui,
    palette: &Palette,
    dark: bool,
    current_id: &mut String,
) -> bool {
    let current = theme::preset(current_id, dark);
    let response = dropdown_button(ui, palette, current.name);
    let mut changed = false;
    egui::Popup::menu(&response)
        .gap(DROPDOWN_POPUP_GAP)
        .style(theme::menu_style)
        .show(|ui| {
            for preset in theme::PRESETS.iter().filter(|p| p.dark == dark) {
                if ui.radio(preset.id == current.id, preset.name).clicked()
                    && preset.id != current.id
                {
                    *current_id = preset.id.to_owned();
                    changed = true;
                }
            }
        });
    changed
}

/// Settings card (design-system §4, preferences.md §3): rounded bordered
/// container on `bg.canvas`. No inner horizontal margin — rows and rules
/// (`setting_divider`) carry their own `CARD_PAD_X` inset.
pub fn settings_card(ui: &mut egui::Ui, palette: &Palette, contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(palette.bg_canvas)
        .stroke(egui::Stroke::new(1.0_f32, palette.border_subtle))
        .corner_radius(egui::CornerRadius::same(CARD_RADIUS))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.set_width(ui.available_width());
            contents(ui);
        });
}

/// Setting row: label + optional description on the left, control in the right
/// slot. The slot is in centered right-to-left layout — a multi-widget control
/// wraps itself in `ui.horizontal` to keep its visual order.
pub fn setting_row(
    ui: &mut egui::Ui,
    palette: &Palette,
    label: &str,
    description: Option<&str>,
    control: impl FnOnce(&mut egui::Ui),
) {
    let size = egui::vec2(ui.available_width(), ROW_MIN_HEIGHT);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let inner = rect.shrink2(egui::vec2(CARD_PAD_X, 0.0));
    // A `vertical` nested in a horizontal layout stretches over the whole rect
    // and anchors its content at the top: to center the label+description block
    // in the row height, we measure the block and place its rect.
    let label_font = egui::FontId::new(LABEL_SIZE, theme::medium_family(ui.ctx()));
    let mut block = ui.ctx().fonts_mut(|f| f.row_height(&label_font));
    if description.is_some() {
        block += LABEL_GAP
            + ui.ctx()
                .fonts_mut(|f| f.row_height(&egui::FontId::proportional(DESCRIPTION_SIZE)));
    }
    let text_rect = egui::Rect::from_x_y_ranges(
        inner.x_range(),
        egui::Rangef::new(
            inner.center().y - block / 2.0,
            inner.center().y + block / 2.0,
        ),
    );
    let mut left = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(text_rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    left.spacing_mut().item_spacing.y = LABEL_GAP;
    left.label(
        egui::RichText::new(label)
            .size(LABEL_SIZE)
            .family(label_font.family.clone())
            .color(palette.text_primary),
    );
    if let Some(text) = description {
        left.label(
            egui::RichText::new(text)
                .size(DESCRIPTION_SIZE)
                .color(palette.text_muted),
        );
    }
    let mut right = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    control(&mut right);
}

/// 1px rule between two rows of a card, inset by the rows' horizontal padding
/// (`CARD_PAD_X`) on each side.
pub fn setting_divider(ui: &mut egui::Ui, palette: &Palette) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().hline(
        egui::Rangef::new(rect.left() + CARD_PAD_X, rect.right() - CARD_PAD_X),
        rect.center().y,
        egui::Stroke::new(1.0_f32, palette.border_subtle),
    );
}

const SWITCH_SIZE: egui::Vec2 = egui::vec2(40.0, 24.0);
const SWITCH_KNOB_INSET: f32 = 3.0;

/// Pill on/off switch (design-system §4): accent track when on, `bg.surface`
/// when off, a circular knob that slides between the two ends. Returns whether
/// the value flipped this frame.
pub fn toggle_switch(ui: &mut egui::Ui, palette: &Palette, on: &mut bool) -> bool {
    let (rect, response, hovered) = clickable(ui, SWITCH_SIZE, true);
    let toggled = response.clicked();
    if toggled {
        *on = !*on;
    }
    let track = if *on {
        palette.accent
    } else if hovered {
        palette.bg_surface_hover
    } else {
        palette.bg_surface
    };
    let painter = ui.painter();
    let radius = rect.height() / 2.0;
    painter.rect(
        rect,
        egui::CornerRadius::same(radius as u8),
        track,
        egui::Stroke::new(1.0_f32, palette.border_subtle),
        egui::StrokeKind::Inside,
    );
    let knob_r = radius - SWITCH_KNOB_INSET;
    let knob_x = if *on {
        rect.right() - radius
    } else {
        rect.left() + radius
    };
    let knob = if *on {
        palette.lane_node_text
    } else {
        palette.text_muted
    };
    painter.circle_filled(egui::pos2(knob_x, rect.center().y), knob_r, knob);
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, *on, ""));
    toggled
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SegmentPosition {
    First,
    Middle,
    Last,
}

impl SegmentPosition {
    fn at(index: usize, count: usize) -> Self {
        if index == 0 {
            SegmentPosition::First
        } else if index + 1 == count {
            SegmentPosition::Last
        } else {
            SegmentPosition::Middle
        }
    }

    fn radius(self) -> egui::CornerRadius {
        match self {
            SegmentPosition::First => egui::CornerRadius {
                nw: RADIUS_PILL,
                ne: 0,
                sw: RADIUS_PILL,
                se: 0,
            },
            SegmentPosition::Middle => egui::CornerRadius::ZERO,
            SegmentPosition::Last => egui::CornerRadius {
                nw: 0,
                ne: RADIUS_PILL,
                sw: 0,
                se: RADIUS_PILL,
            },
        }
    }
}

/// Segment of the Theme control: centered icon + label. The selected segment
/// detaches from the group — fully rounded corners, `accent.subtle` fill,
/// `accent` border — and covers the adjacent separators; hover fills the inside
/// without covering the group's border.
fn theme_segment(
    ui: &mut egui::Ui,
    palette: &Palette,
    label: &str,
    icon: lucide_icons::Icon,
    selected: bool,
    position: SegmentPosition,
) -> egui::Response {
    let (rect, response, hovered) = clickable(ui, SEGMENT_SIZE, true);
    if selected {
        ui.painter().rect(
            rect,
            egui::CornerRadius::same(RADIUS_PILL),
            palette.accent_subtle,
            egui::Stroke::new(1.0_f32, palette.accent),
            egui::StrokeKind::Inside,
        );
    } else if hovered {
        ui.painter().rect_filled(
            rect.shrink(1.0),
            position.radius(),
            palette.bg_surface_hover,
        );
    }
    let color = if selected {
        palette.accent
    } else {
        palette.text_secondary
    };
    let font = egui::FontId::proportional(SEGMENT_LABEL_SIZE);
    let text_width = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font.clone(), color)
        .size()
        .x;
    let left = rect.center().x - (SEGMENT_ICON_SIZE + SEGMENT_ICON_GAP + text_width) / 2.0;
    paint_icon(
        ui.painter(),
        egui::pos2(left + SEGMENT_ICON_SIZE / 2.0, rect.center().y),
        SEGMENT_ICON_SIZE,
        icon,
        color,
    );
    ui.painter().text(
        egui::pos2(left + SEGMENT_ICON_SIZE + SEGMENT_ICON_GAP, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        font,
        color,
    );
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, label)
    });
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_position_rounds_only_the_outer_corners() {
        assert_eq!(SegmentPosition::at(0, 3), SegmentPosition::First);
        assert_eq!(SegmentPosition::at(1, 3), SegmentPosition::Middle);
        assert_eq!(SegmentPosition::at(2, 3), SegmentPosition::Last);
    }

    #[test]
    fn the_nav_opens_on_appearance_and_lists_the_sections_in_order() {
        assert_eq!(
            PreferencesSection::default(),
            PreferencesSection::Appearance
        );
        let titles: Vec<&str> = SECTIONS.iter().map(|s| s.title()).collect();
        assert_eq!(
            titles,
            [
                "Appearance",
                "Git",
                "Keyboard",
                "Terminal",
                "Agents",
                "Phone",
                "Pull Requests",
                "Project",
                "Updates"
            ]
        );
    }

    #[test]
    fn the_three_theme_modes_are_offered_in_order() {
        let labels: Vec<&str> = MODES.iter().map(|(_, l, _)| *l).collect();
        assert_eq!(labels, ["Auto", "Light", "Dark"]);
        let modes: Vec<ThemeMode> = MODES.iter().map(|(m, _, _)| *m).collect();
        assert_eq!(modes, [ThemeMode::Auto, ThemeMode::Light, ThemeMode::Dark]);
    }
}
