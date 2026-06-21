use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::ai::AiProvider;
use crate::git::sync::PullDefault;
use crate::keybindings::{Action, Keymap};
use crate::terminal::links::Editor;
use crate::theme::ThemeMode;
use crate::ui::agents_view::AgentsViewMode;
use crate::ui::file_list::FileViewMode;
use crate::workspace_launcher::WorkspaceOpener;

const PREFS_FILE: &str = "prefs.toml";
const DEFAULT_LEFT_SIDEBAR_WIDTH: f32 = 280.0;
const DEFAULT_RIGHT_SIDEBAR_WIDTH: f32 = 480.0;
/// Default shared width of a project column in the agents dashboard's columns
/// view; the user resizes it by dragging a column gap (specs/agents.md §5).
const DEFAULT_AGENTS_COLUMN_WIDTH: f32 = 874.0;
/// Default shared height of an agent's live-terminal card in the columns view;
/// the user resizes it by dragging a card's bottom edge (specs/agents.md §5).
const DEFAULT_AGENTS_TERMINAL_HEIGHT: f32 = 360.0;
/// Default height of the Run terminal strip at the bottom of the git sidebar
/// (git.md §3); the user resizes it by dragging its top edge.
const DEFAULT_RUN_PANEL_HEIGHT: f32 = 280.0;

/// A sidebar project: a root repo (or plain folder) and its linked worktrees
/// (worktrees.md §5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub root: PathBuf,
    #[serde(default)]
    pub worktrees: Vec<PathBuf>,
    /// Group folded in the sidebar (worktrees.md §3); omitted from the TOML when
    /// expanded — the common case — so plain projects stay one line.
    #[serde(default, skip_serializing_if = "is_false")]
    pub collapsed: bool,
    /// Hidden from the sidebar by the user (eye dropdown / header menu); omitted
    /// from the TOML unless set, so visible projects round-trip unchanged.
    #[serde(default, skip_serializing_if = "is_false")]
    pub hidden: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// Per-project settings keyed by the **project root** (shared across its
/// worktrees), kept apart from `Project` because the latter is rebuilt from the
/// workspace on every mutation (worktrees.md §6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ProjectSettings {
    pub root: PathBuf,
    /// Base directory new worktrees are created under; `None` ⇒ `<root>.worktrees`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree_base: Option<PathBuf>,
    /// Bash run in the new worktree's first terminal after creation (verbatim).
    pub post_create: String,
    /// Command run by the Run terminal (git.md §3); empty ⇒ auto-detect from the
    /// project's manifest (`crate::run::detect_run_command`).
    pub run_command: String,
    /// Base port for the group's `$PORT` substitution (git.md §3); `None` ⇒
    /// `crate::run::DEFAULT_BASE_PORT`. Each worktree gets base + its group offset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_port: Option<u16>,
    /// Per-worktree `$PORT` overrides keyed by worktree path (git.md §3); a worktree
    /// absent here falls back to the auto base+offset.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub port_overrides: BTreeMap<PathBuf, u16>,
}

impl ProjectSettings {
    /// Nothing configured: such an entry is not persisted (kept out of the TOML).
    pub fn is_empty(&self) -> bool {
        self.worktree_base.is_none()
            && self.post_create.is_empty()
            && self.run_command.is_empty()
            && self.base_port.is_none()
            && self.port_overrides.is_empty()
    }
}

// `projects` last: TOML requires tables after scalar values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub active: Option<PathBuf>,
    pub theme: ThemeMode,
    /// Theme families per mode (`theme::PRESETS`) — an unknown id falls back to
    /// Helm at resolution time, without rewriting the TOML.
    pub light_theme: String,
    pub dark_theme: String,
    pub left_sidebar_width: f32,
    pub right_sidebar_width: f32,
    pub show_workspace: bool,
    pub show_git: bool,
    pub pull_default: PullDefault,
    /// AI CLI behind the commit card's "Generate commit message" button.
    pub ai_provider: AiProvider,
    /// Instructions appended to the commit message prompt.
    pub ai_instructions: String,
    /// AI CLI that performs the AI rebase (agentic — runs git itself, git.md §9);
    /// configured separately from the commit-message provider.
    pub ai_rebase_provider: AiProvider,
    /// IDE opening a file from a terminal Cmd+click link (terminal.md §12): its
    /// CLI template is spawned with the file path and line (`Editor::template`).
    pub editor: Editor,
    /// Post a native banner when a watched agent finishes a turn (specs/agents.md);
    /// on by default.
    pub notify_on_agent_completion: bool,
    /// Cross-repo agents dashboard layout (specs/agents.md §5): the master-detail
    /// list or the multi-terminal column grid. Restored on launch.
    pub agents_view: AgentsViewMode,
    /// Shared width of a project column in the dashboard's columns view, set by
    /// dragging a column gap (specs/agents.md §5). Restored on launch; clamped by
    /// the view.
    pub agents_column_width: f32,
    /// Shared height of an agent's live-terminal card in the columns view, set by
    /// dragging a card's bottom edge (specs/agents.md §5). Restored on launch;
    /// clamped by the view.
    pub agents_terminal_height: f32,
    /// Flat vs IDE-style tree layout shared by the WIP and commit-detail file
    /// lists (M40). Restored on launch; absent in older prefs falls back to Flat.
    pub git_file_view: FileViewMode,
    /// Height of the Run terminal strip at the bottom of the git sidebar (git.md
    /// §3), set by dragging its top edge. Restored on launch.
    pub run_panel_height: f32,
    /// Run terminal strip folded to its header (git.md §3). Restored on launch.
    #[serde(default, skip_serializing_if = "is_false")]
    pub run_panel_collapsed: bool,
    /// Last app picked in the workspace launcher; the main button reopens it.
    pub workspace_opener: WorkspaceOpener,
    /// Highest app version whose release notes the user has already seen
    /// (update.md §9.3): the boot trigger shows the What's new modal once when
    /// `current_version()` exceeds it. Empty on a first install ⇒ silent baseline.
    pub last_seen_version: String,
    /// Rebindable-action deviations (`action-id = "combo"`, keybindings.md §6):
    /// only deviations from the defaults, `""` = unbound; unknown ids are kept
    /// verbatim. Regular table — after the scalars, before the arrays-of-tables.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub keybindings: BTreeMap<String, String>,
    pub projects: Vec<Project>,
    /// Per-project settings (worktrees.md §6); array-of-tables like `projects`,
    /// so it stays after every scalar field.
    pub project_settings: Vec<ProjectSettings>,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            active: None,
            theme: ThemeMode::default(),
            light_theme: "helm".to_owned(),
            dark_theme: "helm".to_owned(),
            left_sidebar_width: DEFAULT_LEFT_SIDEBAR_WIDTH,
            right_sidebar_width: DEFAULT_RIGHT_SIDEBAR_WIDTH,
            show_workspace: true,
            show_git: false,
            pull_default: PullDefault::default(),
            ai_provider: AiProvider::default(),
            ai_instructions: String::new(),
            ai_rebase_provider: AiProvider::default(),
            editor: Editor::default(),
            notify_on_agent_completion: true,
            agents_view: AgentsViewMode::default(),
            agents_column_width: DEFAULT_AGENTS_COLUMN_WIDTH,
            agents_terminal_height: DEFAULT_AGENTS_TERMINAL_HEIGHT,
            git_file_view: FileViewMode::default(),
            run_panel_height: DEFAULT_RUN_PANEL_HEIGHT,
            run_panel_collapsed: false,
            workspace_opener: WorkspaceOpener::default(),
            last_seen_version: String::new(),
            keybindings: BTreeMap::new(),
            projects: Vec::new(),
            project_settings: Vec::new(),
        }
    }
}

/// Old flat format (M4-3): list of paths + active by index.
#[derive(Deserialize)]
#[serde(default)]
struct LegacyPrefs {
    repos: Vec<PathBuf>,
    active: Option<usize>,
    theme: ThemeMode,
    left_sidebar_width: f32,
    right_sidebar_width: f32,
    pull_default: PullDefault,
}

impl Default for LegacyPrefs {
    fn default() -> Self {
        Self {
            repos: Vec::new(),
            active: None,
            theme: ThemeMode::default(),
            left_sidebar_width: DEFAULT_LEFT_SIDEBAR_WIDTH,
            right_sidebar_width: DEFAULT_RIGHT_SIDEBAR_WIDTH,
            pull_default: PullDefault::default(),
        }
    }
}

impl From<LegacyPrefs> for Prefs {
    fn from(legacy: LegacyPrefs) -> Self {
        Self {
            active: legacy.active.and_then(|i| legacy.repos.get(i).cloned()),
            theme: legacy.theme,
            left_sidebar_width: legacy.left_sidebar_width,
            right_sidebar_width: legacy.right_sidebar_width,
            pull_default: legacy.pull_default,
            projects: legacy
                .repos
                .into_iter()
                .map(|root| Project {
                    root,
                    worktrees: Vec::new(),
                    collapsed: false,
                    hidden: false,
                })
                .collect(),
            ..Self::default()
        }
    }
}

impl Prefs {
    pub fn to_toml(&self) -> Result<String, toml::ser::Error> {
        toml::to_string_pretty(self)
    }

    pub fn from_toml(text: &str) -> Result<Self, toml::de::Error> {
        Ok(Self::parse(text)?.0)
    }

    /// Resolves the persisted `keybindings` table into a `Keymap`
    /// (preferences.md §5): unknown ids and bad combos are ignored — the
    /// defaults apply — without rewriting the TOML.
    pub fn keymap(&self) -> Keymap {
        Keymap::resolve(&self.keybindings)
    }

    /// Rewrites the known-action entries from `keymap` — only deviations, `""` =
    /// unbound — while keeping unknown ids verbatim (preferences.md §5).
    pub fn set_keybindings(&mut self, keymap: &Keymap) {
        self.keybindings
            .retain(|id, _| Action::from_id(id).is_none());
        for (action, shortcut) in keymap.deviations() {
            self.keybindings.insert(
                action.id().to_owned(),
                shortcut.map(|s| s.canonical()).unwrap_or_default(),
            );
        }
    }

    /// Settings of the project rooted at `root`, if any (worktrees.md §6).
    pub fn project_settings(&self, root: &Path) -> Option<&ProjectSettings> {
        self.project_settings.iter().find(|s| s.root == root)
    }

    /// Upserts the settings of `root` via `edit`, dropping the entry if it ends up
    /// empty so the TOML never accumulates blank tables. Fields `edit` doesn't touch
    /// are preserved across the update.
    fn upsert_project_settings(&mut self, root: &Path, edit: impl FnOnce(&mut ProjectSettings)) {
        let mut settings = self
            .project_settings
            .iter()
            .position(|s| s.root == root)
            .map(|i| self.project_settings.remove(i))
            .unwrap_or_else(|| ProjectSettings {
                root: root.to_path_buf(),
                ..Default::default()
            });
        edit(&mut settings);
        if !settings.is_empty() {
            self.project_settings.push(settings);
        }
    }

    /// Upserts the worktree base / post-create / run command of `root`, leaving the
    /// port settings (git.md §3) untouched.
    pub fn set_project_settings(
        &mut self,
        root: PathBuf,
        worktree_base: Option<PathBuf>,
        post_create: String,
        run_command: String,
    ) {
        self.upsert_project_settings(&root, |s| {
            s.worktree_base = worktree_base;
            s.post_create = post_create;
            s.run_command = run_command;
        });
    }

    /// Sets the group's base port for `$PORT` substitution (git.md §3); `None`
    /// restores the auto default.
    pub fn set_base_port(&mut self, root: &Path, base_port: Option<u16>) {
        self.upsert_project_settings(root, |s| s.base_port = base_port);
    }

    /// Sets (or with `None` clears) a worktree's manual `$PORT` override (git.md §3).
    pub fn set_worktree_port(&mut self, root: &Path, worktree: &Path, port: Option<u16>) {
        self.upsert_project_settings(root, |s| match port {
            Some(p) => {
                s.port_overrides.insert(worktree.to_path_buf(), p);
            }
            None => {
                s.port_overrides.remove(worktree);
            }
        });
    }

    /// Drops settings whose project is no longer in `roots` — orphaned by a
    /// Remove-from-sidebar or a startup purge (worktrees.md §6).
    pub fn retain_project_settings(&mut self, roots: &[PathBuf]) {
        self.project_settings.retain(|s| roots.contains(&s.root));
    }

    /// Migration: the `repos` key marks the old flat format (`active` is an index
    /// there, untranslatable to a path by the new struct). Git grouping of the
    /// migrated paths is done by the startup sync (M11-6).
    fn parse(text: &str) -> Result<(Self, bool), toml::de::Error> {
        let value: toml::Value = toml::from_str(text)?;
        if value.get("repos").is_some() {
            let legacy: LegacyPrefs = value.try_into()?;
            return Ok((legacy.into(), true));
        }
        Ok((value.try_into()?, false))
    }

    pub fn load() -> Self {
        prefs_path()
            .map(|p| Self::load_from(&p))
            .unwrap_or_default()
    }

    /// Loads the prefs; a file in the old format is migrated then **rewritten** in
    /// the new format (worktrees.md §5). Failures fall back to defaults but are
    /// logged — silent fallback made corrupt prefs undiagnosable (M17-3).
    pub fn load_from(path: &Path) -> Self {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            // First launch: no file yet, nothing to report.
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Self::default(),
            Err(err) => {
                eprintln!("helm: cannot read prefs {}: {err}", path.display());
                return Self::default();
            }
        };
        match Self::parse(&text) {
            Ok((prefs, migrated)) => {
                if migrated {
                    if let Err(err) = prefs.save_to(path) {
                        eprintln!(
                            "helm: cannot rewrite migrated prefs {}: {err}",
                            path.display()
                        );
                    }
                }
                prefs
            }
            Err(err) => {
                eprintln!(
                    "helm: invalid prefs {} ({err}); using defaults",
                    path.display()
                );
                Self::default()
            }
        }
    }

    /// Purges projects whose root is no longer a git repository — gone from disk or
    /// turned into a plain folder (Open Folder is git-only, overview.md §3.1): missing
    /// root ⇒ **whole group**, missing worktree ⇒ its entry. `active` follows: a
    /// surviving path is kept; if the active worktree is purged ⇒ falls back to its
    /// root, otherwise `None`. Returns `true` if something was removed — the TOML must
    /// then be rewritten.
    pub fn purge_missing_repos(&mut self) -> bool {
        let active_root = self.active.as_ref().and_then(|a| {
            self.projects
                .iter()
                .find(|p| &p.root == a || p.worktrees.contains(a))
                .map(|p| p.root.clone())
        });

        let mut changed = false;
        self.projects.retain(|p| {
            let keep = crate::git::is_repo(&p.root);
            changed |= !keep;
            keep
        });
        for p in &mut self.projects {
            let before = p.worktrees.len();
            p.worktrees.retain(|w| w.exists());
            changed |= p.worktrees.len() != before;
        }
        if !changed {
            return false;
        }

        if let Some(a) = self.active.clone() {
            let survives = self
                .projects
                .iter()
                .any(|p| p.root == a || p.worktrees.contains(&a));
            if !survives {
                self.active = active_root.filter(|r| self.projects.iter().any(|p| &p.root == r));
            }
        }
        true
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = prefs_path().ok_or_else(|| anyhow::anyhow!("no preferences directory"))?;
        self.save_to(&path)
    }

    pub fn save_to(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(path, self.to_toml()?)?;
        Ok(())
    }
}

pub fn prefs_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "helm").map(|dirs| dirs.config_dir().join(PREFS_FILE))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(root: &str, worktrees: &[&str]) -> Project {
        Project {
            root: PathBuf::from(root),
            worktrees: worktrees.iter().map(PathBuf::from).collect(),
            collapsed: false,
            hidden: false,
        }
    }

    #[test]
    fn hidden_project_round_trips_and_omits_when_visible() {
        let prefs = Prefs {
            projects: vec![
                Project {
                    root: PathBuf::from("/Users/dev/alpha"),
                    worktrees: Vec::new(),
                    collapsed: false,
                    hidden: true,
                },
                project("/Users/dev/beta", &[]),
            ],
            ..Default::default()
        };
        let text = prefs.to_toml().unwrap();
        assert!(text.contains("hidden = true"), "unexpected format:\n{text}");
        // The visible project omits the key entirely (default false).
        assert_eq!(
            text.matches("hidden").count(),
            1,
            "unexpected format:\n{text}"
        );
        assert_eq!(Prefs::from_toml(&text).unwrap(), prefs);
    }

    #[test]
    fn default_prefs_match_architecture_contract() {
        let p = Prefs::default();
        assert!(p.projects.is_empty());
        assert_eq!(p.active, None);
        assert_eq!(p.theme, ThemeMode::Auto);
        assert_eq!(p.left_sidebar_width, DEFAULT_LEFT_SIDEBAR_WIDTH);
        assert_eq!(p.right_sidebar_width, DEFAULT_RIGHT_SIDEBAR_WIDTH);
        assert!(p.show_workspace, "workspace sidebar open by default");
        assert!(!p.show_git, "git sidebar closed by default");
        assert_eq!(p.pull_default, PullDefault::Ff);
    }

    #[test]
    fn toml_round_trip_is_stable() {
        let prefs = Prefs {
            active: Some(PathBuf::from("/Users/dev/alpha.worktrees/feat")),
            theme: ThemeMode::Dark,
            light_theme: "one".to_owned(),
            dark_theme: "catppuccin".to_owned(),
            left_sidebar_width: 240.0,
            right_sidebar_width: 300.0,
            show_workspace: false,
            show_git: true,
            pull_default: PullDefault::FfOnly,
            ai_provider: AiProvider::Codex,
            ai_instructions: "Always write in French.".to_owned(),
            ai_rebase_provider: AiProvider::Opencode,
            editor: Editor::Zed,
            notify_on_agent_completion: false,
            agents_view: AgentsViewMode::Columns,
            agents_column_width: 540.0,
            agents_terminal_height: 420.0,
            git_file_view: FileViewMode::Tree,
            run_panel_height: 240.0,
            run_panel_collapsed: true,
            workspace_opener: WorkspaceOpener::GitKraken,
            last_seen_version: "0.8.4".to_owned(),
            keybindings: BTreeMap::from([("split-right".to_owned(), "cmd+shift+x".to_owned())]),
            projects: vec![
                project("/Users/dev/alpha", &["/Users/dev/alpha.worktrees/feat"]),
                project("/Users/dev/beta", &[]),
            ],
            project_settings: vec![ProjectSettings {
                root: PathBuf::from("/Users/dev/alpha"),
                worktree_base: Some(PathBuf::from("/wt/alpha")),
                post_create: "npm install\n".to_owned(),
                run_command: "npm run dev -- --port $PORT".to_owned(),
                base_port: Some(4000),
                port_overrides: BTreeMap::from([(PathBuf::from("/wt/alpha/feat"), 4100)]),
            }],
        };

        let text = prefs.to_toml().unwrap();
        assert!(text.contains("[[projects]]"), "unexpected format:\n{text}");
        assert!(
            text.contains("[[project_settings]]"),
            "unexpected format:\n{text}"
        );
        assert_eq!(Prefs::from_toml(&text).unwrap(), prefs);
    }

    #[test]
    fn project_settings_default_empty_and_round_trip() {
        assert!(Prefs::default().project_settings.is_empty());
        assert!(Prefs::from_toml("").unwrap().project_settings.is_empty());
    }

    #[test]
    fn set_project_settings_upserts_and_drops_empty_entries() {
        let mut prefs = Prefs::default();
        let root = PathBuf::from("/Users/dev/alpha");

        prefs.set_project_settings(root.clone(), None, "make setup".to_owned(), String::new());
        assert_eq!(
            prefs
                .project_settings(&root)
                .map(|s| s.post_create.as_str()),
            Some("make setup")
        );

        prefs.set_project_settings(
            root.clone(),
            Some(PathBuf::from("/wt")),
            String::new(),
            String::new(),
        );
        assert_eq!(
            prefs.project_settings.len(),
            1,
            "same root overwrites, not appends"
        );
        assert_eq!(
            prefs
                .project_settings(&root)
                .and_then(|s| s.worktree_base.as_deref()),
            Some(Path::new("/wt"))
        );

        prefs.set_project_settings(root.clone(), None, String::new(), "cargo run".to_owned());
        assert_eq!(
            prefs
                .project_settings(&root)
                .map(|s| s.run_command.as_str()),
            Some("cargo run"),
            "a lone run command is enough to persist the entry"
        );

        prefs.set_project_settings(root.clone(), None, String::new(), String::new());
        assert!(
            prefs.project_settings(&root).is_none(),
            "an empty entry is dropped, not persisted blank"
        );
    }

    #[test]
    fn retain_project_settings_drops_orphans() {
        let mut prefs = Prefs::default();
        prefs.set_project_settings(PathBuf::from("/a"), None, "x".to_owned(), String::new());
        prefs.set_project_settings(PathBuf::from("/b"), None, "y".to_owned(), String::new());

        prefs.retain_project_settings(&[PathBuf::from("/b")]);

        assert_eq!(prefs.project_settings.len(), 1);
        assert_eq!(prefs.project_settings[0].root, PathBuf::from("/b"));
    }

    #[test]
    fn port_settings_survive_a_run_command_edit() {
        let root = PathBuf::from("/a");
        let wt = PathBuf::from("/a.wt/feat");
        let mut prefs = Prefs::default();
        prefs.set_base_port(&root, Some(8080));
        prefs.set_worktree_port(&root, &wt, Some(8090));

        // Editing the run command must not wipe the port settings.
        prefs.set_project_settings(root.clone(), None, String::new(), "cargo run".to_owned());
        let s = prefs.project_settings(&root).unwrap();
        assert_eq!(s.base_port, Some(8080));
        assert_eq!(s.port_overrides.get(&wt), Some(&8090));
        assert_eq!(s.run_command, "cargo run");

        // Clearing an override drops just that key; a now-empty entry is removed.
        prefs.set_worktree_port(&root, &wt, None);
        assert!(prefs
            .project_settings(&root)
            .unwrap()
            .port_overrides
            .is_empty());
        prefs.set_base_port(&root, None);
        prefs.set_project_settings(root.clone(), None, String::new(), String::new());
        assert!(
            prefs.project_settings(&root).is_none(),
            "an entry with no command and no ports is dropped"
        );
    }

    #[test]
    fn collapsed_is_omitted_when_expanded_and_round_trips_when_folded() {
        let expanded = Prefs {
            projects: vec![project("/a", &["/a.wt/x"])],
            ..Prefs::default()
        };
        let text = expanded.to_toml().unwrap();
        assert!(
            !text.contains("collapsed"),
            "an expanded project must not write the flag:\n{text}"
        );

        let folded = Prefs {
            projects: vec![Project {
                root: PathBuf::from("/a"),
                worktrees: vec![PathBuf::from("/a.wt/x")],
                collapsed: true,
                hidden: false,
            }],
            ..Prefs::default()
        };
        let text = folded.to_toml().unwrap();
        assert!(
            text.contains("collapsed = true"),
            "unexpected format:\n{text}"
        );
        assert_eq!(Prefs::from_toml(&text).unwrap(), folded);
    }

    #[test]
    fn project_order_is_preserved_through_round_trip() {
        let prefs = Prefs {
            projects: vec![project("/c", &[]), project("/a", &[]), project("/b", &[])],
            ..Prefs::default()
        };

        let restored = Prefs::from_toml(&prefs.to_toml().unwrap()).unwrap();
        let roots: Vec<&str> = restored
            .projects
            .iter()
            .map(|p| p.root.to_str().unwrap())
            .collect();
        assert_eq!(roots, vec!["/c", "/a", "/b"]);
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let prefs = Prefs::from_toml("theme = \"Light\"\n").unwrap();
        assert_eq!(prefs.theme, ThemeMode::Light);
        assert!(prefs.projects.is_empty());
        assert_eq!(prefs.active, None);
        assert_eq!(prefs.left_sidebar_width, DEFAULT_LEFT_SIDEBAR_WIDTH);
        assert_eq!(prefs.right_sidebar_width, DEFAULT_RIGHT_SIDEBAR_WIDTH);
        assert!(prefs.show_workspace);
        assert!(!prefs.show_git);
        assert_eq!(prefs.pull_default, PullDefault::Ff);
        // Absent from an older file ⇒ notifications on by default (specs/agents.md).
        assert!(prefs.notify_on_agent_completion);
    }

    #[test]
    fn notify_on_agent_completion_round_trips() {
        let prefs = Prefs {
            notify_on_agent_completion: false,
            ..Prefs::default()
        };
        let text = prefs.to_toml().unwrap();
        assert!(!Prefs::from_toml(&text).unwrap().notify_on_agent_completion);
    }

    #[test]
    fn last_seen_version_defaults_empty_and_round_trips() {
        assert_eq!(Prefs::default().last_seen_version, "");
        // Absent from an older file ⇒ empty ⇒ silent baseline (update.md §9.3).
        let old = Prefs::from_toml("theme = \"Light\"\n").unwrap();
        assert_eq!(old.last_seen_version, "");

        let prefs = Prefs {
            last_seen_version: "0.9.0".to_owned(),
            ..Prefs::default()
        };
        let text = prefs.to_toml().unwrap();
        assert!(
            text.contains("last_seen_version = \"0.9.0\""),
            "unexpected format:\n{text}"
        );
        assert_eq!(Prefs::from_toml(&text).unwrap().last_seen_version, "0.9.0");
    }

    #[test]
    fn agents_view_defaults_to_list() {
        assert_eq!(Prefs::default().agents_view, AgentsViewMode::List);
        // Absent from an older file ⇒ the master-detail cockpit (specs/agents.md §5).
        let prefs = Prefs::from_toml("theme = \"Light\"\n").unwrap();
        assert_eq!(prefs.agents_view, AgentsViewMode::List);
    }

    #[test]
    fn agents_column_metrics_default_and_round_trip() {
        assert_eq!(
            Prefs::default().agents_column_width,
            DEFAULT_AGENTS_COLUMN_WIDTH
        );
        assert_eq!(
            Prefs::default().agents_terminal_height,
            DEFAULT_AGENTS_TERMINAL_HEIGHT
        );
        // Absent from an older file ⇒ the defaults.
        let old = Prefs::from_toml("theme = \"Light\"\n").unwrap();
        assert_eq!(old.agents_column_width, DEFAULT_AGENTS_COLUMN_WIDTH);
        assert_eq!(old.agents_terminal_height, DEFAULT_AGENTS_TERMINAL_HEIGHT);
        let prefs = Prefs {
            agents_column_width: 540.0,
            agents_terminal_height: 420.0,
            ..Prefs::default()
        };
        let text = prefs.to_toml().unwrap();
        let back = Prefs::from_toml(&text).unwrap();
        assert_eq!(back.agents_column_width, 540.0);
        assert_eq!(back.agents_terminal_height, 420.0);
    }

    #[test]
    fn run_panel_metrics_default_and_round_trip() {
        assert_eq!(Prefs::default().run_panel_height, DEFAULT_RUN_PANEL_HEIGHT);
        assert!(!Prefs::default().run_panel_collapsed);
        // Absent from an older file ⇒ the defaults.
        let old = Prefs::from_toml("theme = \"Light\"\n").unwrap();
        assert_eq!(old.run_panel_height, DEFAULT_RUN_PANEL_HEIGHT);
        assert!(!old.run_panel_collapsed);
        let prefs = Prefs {
            run_panel_height: 260.0,
            run_panel_collapsed: true,
            ..Prefs::default()
        };
        let back = Prefs::from_toml(&prefs.to_toml().unwrap()).unwrap();
        assert_eq!(back.run_panel_height, 260.0);
        assert!(back.run_panel_collapsed);
    }

    #[test]
    fn agents_view_round_trips_in_snake_case() {
        let prefs = Prefs {
            agents_view: AgentsViewMode::Columns,
            ..Prefs::default()
        };
        let text = prefs.to_toml().unwrap();
        assert!(
            text.contains("agents_view = \"columns\""),
            "unexpected format:\n{text}"
        );
        assert_eq!(
            Prefs::from_toml(&text).unwrap().agents_view,
            AgentsViewMode::Columns
        );
    }

    #[test]
    fn git_file_view_defaults_to_flat() {
        assert_eq!(Prefs::default().git_file_view, FileViewMode::Flat);
        // Absent from an older file ⇒ the historical flat list (M40).
        let prefs = Prefs::from_toml("theme = \"Light\"\n").unwrap();
        assert_eq!(prefs.git_file_view, FileViewMode::Flat);
    }

    #[test]
    fn git_file_view_round_trips_in_snake_case() {
        let prefs = Prefs {
            git_file_view: FileViewMode::Tree,
            ..Prefs::default()
        };
        let text = prefs.to_toml().unwrap();
        assert!(
            text.contains("git_file_view = \"tree\""),
            "unexpected format:\n{text}"
        );
        assert_eq!(
            Prefs::from_toml(&text).unwrap().git_file_view,
            FileViewMode::Tree
        );
    }

    #[test]
    fn pull_default_round_trips_in_kebab_case() {
        for (value, expected) in [
            (PullDefault::FetchAll, "pull_default = \"fetch-all\""),
            (PullDefault::Ff, "pull_default = \"ff\""),
            (PullDefault::FfOnly, "pull_default = \"ff-only\""),
            (PullDefault::Rebase, "pull_default = \"rebase\""),
        ] {
            let prefs = Prefs {
                pull_default: value,
                ..Prefs::default()
            };
            let text = prefs.to_toml().unwrap();
            assert!(text.contains(expected), "unexpected format:\n{text}");
            assert_eq!(Prefs::from_toml(&text).unwrap().pull_default, value);
        }
    }

    #[test]
    fn legacy_flat_toml_is_migrated_with_active_remapped_to_a_path() {
        let legacy = "repos = [\"/Users/dev/alpha\", \"/Users/dev/beta\"]\n\
                      active = 1\n\
                      theme = \"Dark\"\n\
                      left_sidebar_width = 240.0\n";

        let prefs = Prefs::from_toml(legacy).unwrap();

        assert_eq!(
            prefs.projects,
            vec![
                project("/Users/dev/alpha", &[]),
                project("/Users/dev/beta", &[])
            ]
        );
        assert_eq!(prefs.active, Some(PathBuf::from("/Users/dev/beta")));
        assert_eq!(prefs.theme, ThemeMode::Dark);
        assert_eq!(prefs.left_sidebar_width, 240.0);
        assert_eq!(prefs.right_sidebar_width, DEFAULT_RIGHT_SIDEBAR_WIDTH);
    }

    #[test]
    fn legacy_active_out_of_bounds_migrates_to_none() {
        let prefs = Prefs::from_toml("repos = [\"/a\"]\nactive = 7\n").unwrap();
        assert_eq!(prefs.projects, vec![project("/a", &[])]);
        assert_eq!(prefs.active, None);
    }

    #[test]
    fn load_from_rewrites_a_legacy_file_in_the_new_format() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("prefs.toml");
        fs::write(&path, "repos = [\"/a\"]\nactive = 0\n").unwrap();

        let prefs = Prefs::load_from(&path);

        assert_eq!(prefs.active, Some(PathBuf::from("/a")));
        let rewritten = fs::read_to_string(&path).unwrap();
        assert!(
            rewritten.contains("[[projects]]") && !rewritten.contains("repos ="),
            "the file should be rewritten in the new format:\n{rewritten}"
        );
        assert_eq!(Prefs::load_from(&path), prefs, "stable once migrated");
    }

    #[test]
    fn empty_input_loads_default_prefs() {
        assert_eq!(Prefs::from_toml("").unwrap(), Prefs::default());
    }

    #[test]
    fn corrupt_file_falls_back_to_defaults() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("prefs.toml");
        fs::write(&path, "not [ valid { toml").unwrap();

        assert_eq!(Prefs::load_from(&path), Prefs::default());
    }

    #[test]
    fn missing_file_falls_back_to_defaults() {
        let tmp = tempfile::tempdir().unwrap();

        assert_eq!(
            Prefs::load_from(&tmp.path().join("absent.toml")),
            Prefs::default()
        );
    }

    #[test]
    fn all_three_theme_modes_round_trip() {
        for mode in [ThemeMode::Auto, ThemeMode::Light, ThemeMode::Dark] {
            let prefs = Prefs {
                theme: mode,
                ..Prefs::default()
            };
            assert_eq!(
                Prefs::from_toml(&prefs.to_toml().unwrap()).unwrap().theme,
                mode
            );
        }
    }

    #[test]
    fn theme_families_default_to_helm_and_round_trip() {
        let defaults = Prefs::default();
        assert_eq!(defaults.light_theme, "helm");
        assert_eq!(defaults.dark_theme, "helm");
        assert_eq!(Prefs::from_toml("").unwrap().light_theme, "helm");

        let prefs = Prefs {
            light_theme: "github".to_owned(),
            dark_theme: "tokyo".to_owned(),
            ..Prefs::default()
        };
        let restored = Prefs::from_toml(&prefs.to_toml().unwrap()).unwrap();
        assert_eq!(restored.light_theme, "github");
        assert_eq!(restored.dark_theme, "tokyo");
    }

    #[test]
    fn ai_prefs_default_to_claude_and_round_trip_in_kebab_case() {
        let defaults = Prefs::default();
        assert_eq!(defaults.ai_provider, AiProvider::Claude);
        assert_eq!(defaults.ai_instructions, "");
        assert_eq!(defaults.ai_rebase_provider, AiProvider::Claude);
        assert_eq!(
            Prefs::from_toml("").unwrap().ai_provider,
            AiProvider::Claude
        );
        assert_eq!(
            Prefs::from_toml("").unwrap().ai_rebase_provider,
            AiProvider::Claude
        );

        let prefs = Prefs {
            ai_provider: AiProvider::Opencode,
            ai_instructions: "Use conventional commits.".to_owned(),
            ai_rebase_provider: AiProvider::Codex,
            ..Prefs::default()
        };
        let text = prefs.to_toml().unwrap();
        assert!(
            text.contains("ai_provider = \"opencode\""),
            "unexpected format:\n{text}"
        );
        assert!(
            text.contains("ai_rebase_provider = \"codex\""),
            "unexpected format:\n{text}"
        );
        let restored = Prefs::from_toml(&text).unwrap();
        assert_eq!(restored.ai_provider, AiProvider::Opencode);
        assert_eq!(restored.ai_instructions, "Use conventional commits.");
        assert_eq!(restored.ai_rebase_provider, AiProvider::Codex);
    }

    #[test]
    fn editor_defaults_to_vscode_and_round_trips() {
        assert_eq!(Prefs::default().editor, Editor::VsCode);
        assert_eq!(
            Prefs::from_toml("").unwrap().editor,
            Editor::VsCode,
            "an absent key falls back to the default"
        );

        let prefs = Prefs {
            editor: Editor::Zed,
            ..Prefs::default()
        };
        let text = prefs.to_toml().unwrap();
        assert!(
            text.contains("editor = \"zed\""),
            "unexpected format:\n{text}"
        );
        assert_eq!(Prefs::from_toml(&text).unwrap().editor, Editor::Zed);
    }

    #[test]
    fn keybindings_default_empty_omitted_and_round_trip() {
        assert!(Prefs::default().keybindings.is_empty());
        assert!(Prefs::from_toml("").unwrap().keybindings.is_empty());
        let text = Prefs::default().to_toml().unwrap();
        assert!(
            !text.contains("[keybindings]"),
            "empty table must be omitted:\n{text}"
        );

        let mut prefs = Prefs {
            projects: vec![project("/a", &[])],
            ..Prefs::default()
        };
        prefs
            .keybindings
            .insert("split-right".to_owned(), "cmd+shift+x".to_owned());
        let text = prefs.to_toml().unwrap();
        assert!(
            text.contains("[keybindings]") && text.contains("split-right = \"cmd+shift+x\""),
            "unexpected format:\n{text}"
        );
        assert!(
            text.find("[keybindings]").unwrap() < text.find("[[projects]]").unwrap(),
            "table must come before the arrays-of-tables:\n{text}"
        );
        assert_eq!(Prefs::from_toml(&text).unwrap(), prefs);
    }

    #[test]
    fn set_keybindings_writes_only_deviations_with_empty_for_unbound() {
        use crate::keybindings::Shortcut;

        let mut prefs = Prefs::default();
        let mut keymap = Keymap::default();
        keymap.set(Action::SplitRight, Some(Shortcut::cmd_shift(egui::Key::X)));
        keymap.set(Action::Commit, None);

        prefs.set_keybindings(&keymap);
        assert_eq!(
            prefs.keybindings.get("split-right").map(String::as_str),
            Some("cmd+shift+x")
        );
        assert_eq!(
            prefs.keybindings.get("commit").map(String::as_str),
            Some(""),
            "unbound is persisted as the empty string"
        );
        assert_eq!(prefs.keybindings.len(), 2, "defaults are omitted");

        keymap.reset(Action::SplitRight);
        prefs.set_keybindings(&keymap);
        assert!(
            !prefs.keybindings.contains_key("split-right"),
            "an entry back at its default is dropped"
        );
    }

    #[test]
    fn unknown_keybinding_preserved_on_save_and_ignored_at_resolution() {
        use crate::keybindings::Shortcut;

        let text = "[keybindings]\n\
                    future-action = \"cmd+y\"\n\
                    split-right = \"cmd+shift+x\"\n";
        let mut prefs = Prefs::from_toml(text).unwrap();

        let keymap = prefs.keymap();
        assert_eq!(
            keymap.shortcut_for(Action::SplitRight),
            Some(Shortcut::cmd_shift(egui::Key::X))
        );
        assert_eq!(
            keymap.deviations().count(),
            1,
            "the unknown id contributes nothing to the keymap"
        );

        prefs.set_keybindings(&keymap);
        let saved = prefs.to_toml().unwrap();
        assert!(
            saved.contains("future-action = \"cmd+y\""),
            "unknown entry must survive a save:\n{saved}"
        );
        assert!(saved.contains("split-right = \"cmd+shift+x\""));
    }

    #[test]
    fn workspace_opener_defaults_to_zed_and_round_trips_in_kebab_case() {
        assert_eq!(Prefs::default().workspace_opener, WorkspaceOpener::Zed);
        assert_eq!(
            Prefs::from_toml("").unwrap().workspace_opener,
            WorkspaceOpener::Zed
        );

        let prefs = Prefs {
            workspace_opener: WorkspaceOpener::GitKraken,
            ..Prefs::default()
        };
        let text = prefs.to_toml().unwrap();
        assert!(
            text.contains("workspace_opener = \"git-kraken\""),
            "unexpected format:\n{text}"
        );
        assert_eq!(
            Prefs::from_toml(&text).unwrap().workspace_opener,
            WorkspaceOpener::GitKraken
        );
    }

    #[test]
    fn purge_drops_the_whole_group_when_its_root_is_gone() {
        let tmp = tempfile::tempdir().unwrap();
        let kept = tmp.path().join("kept");
        let kept_wt = tmp.path().join("kept-wt");
        git2::Repository::init(&kept).unwrap();
        fs::create_dir(&kept_wt).unwrap();
        let mut prefs = Prefs {
            active: Some(kept.clone()),
            projects: vec![
                Project {
                    root: tmp.path().join("gone"),
                    worktrees: vec![kept_wt.clone()],
                    collapsed: false,
                    hidden: false,
                },
                Project {
                    root: kept.clone(),
                    worktrees: Vec::new(),
                    collapsed: false,
                    hidden: false,
                },
            ],
            ..Prefs::default()
        };

        assert!(prefs.purge_missing_repos());
        assert_eq!(
            prefs.projects,
            vec![Project {
                root: kept.clone(),
                worktrees: Vec::new(),
                collapsed: false,
                hidden: false,
            }],
            "a gone root drops its whole group, even with surviving worktree dirs"
        );
        assert_eq!(prefs.active, Some(kept));
    }

    #[test]
    fn purge_drops_a_gone_worktree_and_active_falls_back_to_its_root() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("proj");
        git2::Repository::init(&root).unwrap();
        let gone_wt = tmp.path().join("feat");
        let mut prefs = Prefs {
            active: Some(gone_wt.clone()),
            projects: vec![Project {
                root: root.clone(),
                worktrees: vec![gone_wt],
                collapsed: false,
                hidden: false,
            }],
            ..Prefs::default()
        };

        assert!(prefs.purge_missing_repos());
        assert_eq!(
            prefs.projects,
            vec![Project {
                root: root.clone(),
                worktrees: Vec::new(),
                collapsed: false,
                hidden: false,
            }]
        );
        assert_eq!(prefs.active, Some(root), "active falls back to the root");
    }

    #[test]
    fn purge_clears_active_when_its_whole_group_is_gone() {
        let tmp = tempfile::tempdir().unwrap();
        let kept = tmp.path().join("kept");
        git2::Repository::init(&kept).unwrap();
        let gone_root = tmp.path().join("gone");
        let gone_wt = tmp.path().join("gone-wt");
        let mut prefs = Prefs {
            active: Some(gone_wt.clone()),
            projects: vec![
                Project {
                    root: kept.clone(),
                    worktrees: Vec::new(),
                    collapsed: false,
                    hidden: false,
                },
                Project {
                    root: gone_root,
                    worktrees: vec![gone_wt],
                    collapsed: false,
                    hidden: false,
                },
            ],
            ..Prefs::default()
        };

        assert!(prefs.purge_missing_repos());
        assert_eq!(prefs.projects.len(), 1);
        assert_eq!(prefs.active, None);
    }

    #[test]
    fn purge_drops_a_root_that_exists_but_is_not_a_git_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let kept = tmp.path().join("kept");
        git2::Repository::init(&kept).unwrap();
        let plain = tmp.path().join("notes");
        fs::create_dir(&plain).unwrap();
        let mut prefs = Prefs {
            active: Some(plain.clone()),
            projects: vec![
                Project {
                    root: kept.clone(),
                    worktrees: Vec::new(),
                    collapsed: false,
                    hidden: false,
                },
                Project {
                    root: plain,
                    worktrees: Vec::new(),
                    collapsed: false,
                    hidden: false,
                },
            ],
            ..Prefs::default()
        };

        assert!(prefs.purge_missing_repos());
        assert_eq!(
            prefs.projects,
            vec![Project {
                root: kept.clone(),
                worktrees: Vec::new(),
                collapsed: false,
                hidden: false,
            }],
            "an existing folder that is not a git repo is dropped like a vanished root"
        );
        assert_eq!(prefs.active, None);
    }

    #[test]
    fn purge_is_a_no_op_when_all_folders_exist() {
        let tmp = tempfile::tempdir().unwrap();
        git2::Repository::init(tmp.path()).unwrap();
        let mut prefs = Prefs {
            active: Some(tmp.path().to_path_buf()),
            projects: vec![Project {
                root: tmp.path().to_path_buf(),
                worktrees: Vec::new(),
                collapsed: false,
                hidden: false,
            }],
            ..Prefs::default()
        };

        assert!(!prefs.purge_missing_repos());
        assert_eq!(prefs.projects.len(), 1);
        assert_eq!(prefs.active, Some(tmp.path().to_path_buf()));
    }

    #[test]
    fn prefs_path_lands_in_application_support() {
        let path = prefs_path().expect("project dirs resolved");
        assert!(path.ends_with("prefs.toml"));
        let parent = path.parent().unwrap().to_string_lossy().into_owned();
        assert!(parent.contains("helm"), "unexpected prefs dir: {parent}");
        if cfg!(target_os = "macos") {
            assert!(
                parent.contains("Application Support"),
                "macOS prefs dir must live under Application Support: {parent}"
            );
        }
    }
}
