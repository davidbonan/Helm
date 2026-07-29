use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

pub const DEFAULT_WORKSPACE_OPENER: WorkspaceOpener = WorkspaceOpener::Zed;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkspaceOpener {
    Cursor,
    Zed,
    Finder,
    Terminal,
    Ghostty,
    GitKraken,
}

impl WorkspaceOpener {
    pub const ALL: [Self; 6] = [
        Self::Cursor,
        Self::Zed,
        Self::Finder,
        Self::Terminal,
        Self::Ghostty,
        Self::GitKraken,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Cursor => "Cursor",
            Self::Zed => "Zed",
            Self::Finder => "Finder",
            Self::Terminal => "Terminal",
            Self::Ghostty => "Ghostty",
            Self::GitKraken => "GitKraken",
        }
    }

    const fn application_name(self) -> Option<&'static str> {
        match self {
            Self::Cursor => Some("Cursor"),
            Self::Zed => Some("Zed"),
            Self::Finder => None,
            Self::Terminal => Some("Terminal"),
            Self::Ghostty => Some("Ghostty"),
            Self::GitKraken => None,
        }
    }

    /// The `.app` bundle looked up under the Applications folders to decide
    /// whether the app is installed. `None` for the macOS system apps (Finder,
    /// Terminal) that always ship with the OS.
    const fn bundle_name(self) -> Option<&'static str> {
        match self {
            Self::Cursor => Some("Cursor.app"),
            Self::Zed => Some("Zed.app"),
            Self::Finder => None,
            Self::Terminal => None,
            Self::Ghostty => Some("Ghostty.app"),
            Self::GitKraken => Some("GitKraken.app"),
        }
    }

    /// Candidate `.app` bundle locations, most-likely first, used to read the
    /// real app icon. System apps live at fixed paths (and `bundle_name` is
    /// `None` for them); the rest are looked up under the Applications folders.
    fn bundle_candidates(self) -> Vec<PathBuf> {
        match self {
            Self::Finder => vec![PathBuf::from("/System/Library/CoreServices/Finder.app")],
            Self::Terminal => vec![
                PathBuf::from("/System/Applications/Utilities/Terminal.app"),
                PathBuf::from("/Applications/Utilities/Terminal.app"),
            ],
            Self::Cursor | Self::Zed | Self::Ghostty | Self::GitKraken => self
                .bundle_name()
                .map(|bundle| {
                    application_dirs()
                        .into_iter()
                        .map(|dir| dir.join(bundle))
                        .collect()
                })
                .unwrap_or_default(),
        }
    }

    fn bundle_path(self) -> Option<PathBuf> {
        self.bundle_candidates()
            .into_iter()
            .find(|path| path.exists())
    }

    /// CLI shipped inside the `.app` bundle, for the IDE openers able to open the
    /// workspace **and** a file as a tab of the same window. `open -a` with two
    /// paths can't do this: the app routes the file to its last active window.
    const fn ide_cli(self) -> Option<&'static str> {
        match self {
            Self::Cursor => Some("Contents/Resources/app/bin/cursor"),
            Self::Zed => Some("Contents/MacOS/cli"),
            Self::Finder | Self::Terminal | Self::Ghostty | Self::GitKraken => None,
        }
    }

    fn ide_cli_path(self) -> Option<PathBuf> {
        let cli = self.bundle_path()?.join(self.ide_cli()?);
        cli.exists().then_some(cli)
    }
}

/// Openers whose app is present on the Mac, in menu order. System apps are
/// always kept; the rest must have their bundle on disk. Probes the filesystem,
/// so the result is cached at startup rather than recomputed each frame.
pub fn installed_openers() -> Vec<WorkspaceOpener> {
    let dirs = application_dirs();
    installed_openers_where(|bundle| dirs.iter().any(|dir| dir.join(bundle).exists()))
}

fn application_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![PathBuf::from("/Applications")];
    if let Some(home) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(home).join("Applications"));
    }
    dirs
}

fn installed_openers_where(is_present: impl Fn(&str) -> bool) -> Vec<WorkspaceOpener> {
    WorkspaceOpener::ALL
        .into_iter()
        .filter(|opener| match opener.bundle_name() {
            None => true,
            Some(bundle) => is_present(bundle),
        })
        .collect()
}

/// The opener shown on the launcher's main button: the last-used one when it is
/// still installed, otherwise the first installed opener (Finder/Terminal always
/// qualify, so the list is never empty on macOS).
pub fn resolve_default(
    preferred: WorkspaceOpener,
    installed: &[WorkspaceOpener],
) -> WorkspaceOpener {
    if installed.contains(&preferred) {
        preferred
    } else {
        installed.first().copied().unwrap_or(preferred)
    }
}

impl Default for WorkspaceOpener {
    fn default() -> Self {
        DEFAULT_WORKSPACE_OPENER
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenWorkspaceCommand {
    pub program: OsString,
    pub args: Vec<OsString>,
}

pub fn command_for(opener: WorkspaceOpener, path: &Path) -> OpenWorkspaceCommand {
    let mut args = Vec::new();
    match opener {
        // `open -a GitKraken <path>` launches the app but ignores the folder; the
        // `gitkraken://repo<path>` scheme (documented by `gitkraken --help`) opens
        // the repo, whether the app is already running or not.
        WorkspaceOpener::GitKraken => args.push(OsString::from(gitkraken_repo_uri(path))),
        _ => {
            if let Some(app) = opener.application_name() {
                args.push(OsString::from("-a"));
                args.push(OsString::from(app));
            }
            args.push(path.as_os_str().to_owned());
        }
    }
    OpenWorkspaceCommand {
        program: OsString::from("open"),
        args,
    }
}

/// Command opening `workspace` in an IDE window through the bundled CLI, with
/// `file` (when present) as a tab of that window. The window flag keeps each
/// project in its own window instead of taking over whatever the IDE last
/// focused — but only when the project isn't open yet: Zed's `--classic` matches
/// the workspace against the open windows and re-focuses the one that already
/// holds it, where `--new` skips that lookup and forces a second window with a
/// full project reload. Cursor (VS Code CLI) wants the file behind `--goto`; Zed
/// takes plain paths and merges them into the workspace.
fn ide_command(
    opener: WorkspaceOpener,
    cli: PathBuf,
    workspace: &Path,
    file: Option<&Path>,
) -> OpenWorkspaceCommand {
    let window_flag = match opener {
        WorkspaceOpener::Cursor => "--new-window",
        _ => "--classic",
    };
    let mut args = vec![
        OsString::from(window_flag),
        workspace.as_os_str().to_owned(),
    ];
    if let Some(file) = file {
        if opener == WorkspaceOpener::Cursor {
            args.push(OsString::from("--goto"));
        }
        args.push(file.as_os_str().to_owned());
    }
    OpenWorkspaceCommand {
        program: cli.into_os_string(),
        args,
    }
}

fn gitkraken_repo_uri(path: &Path) -> String {
    let mut uri = String::from("gitkraken://repo");
    for byte in path.to_string_lossy().bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                uri.push(byte as char)
            }
            _ => uri.push_str(&format!("%{byte:02X}")),
        }
    }
    uri
}

/// Opens the workspace in `opener`. For IDEs the bundled CLI opens the project in
/// a new window — and, when `file` is given, with that file as a tab of the same
/// new window. Falls back to the plain `open` command when the opener has no CLI
/// (system apps) or its bundled CLI is missing; the file is dropped when gone from
/// disk (deleted entry in the git status).
pub fn open_workspace(
    opener: WorkspaceOpener,
    path: &Path,
    file: Option<&Path>,
) -> std::io::Result<()> {
    let file = file.filter(|file| file.exists());
    let command = opener
        .ide_cli_path()
        .map(|cli| ide_command(opener, cli, path, file))
        .unwrap_or_else(|| command_for(opener, path));
    Command::new(&command.program).args(command.args).spawn()?;
    Ok(())
}

/// A decoded application icon as straight-alpha RGBA pixels, row-major. Kept free
/// of any UI type so the rendering layer owns the texture upload.
pub struct OpenerIcon {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

/// The real macOS icon of the app behind `opener`, read from its `.app` bundle:
/// `CFBundleIconFile` in `Contents/Info.plist` names the `.icns`, which is decoded
/// to RGBA. `None` when the app is absent or its icon can't be decoded.
pub fn load_opener_icon(opener: WorkspaceOpener) -> Option<OpenerIcon> {
    let icns = bundle_icns_path(&opener.bundle_path()?)?;
    decode_icns(&icns)
}

/// Resolves the bundle's icon file: `Contents/Resources/<CFBundleIconFile>`, with
/// the `.icns` extension appended when the plist value omits it (e.g. `Ghostty`).
fn bundle_icns_path(bundle: &Path) -> Option<PathBuf> {
    let info = plist::Value::from_file(bundle.join("Contents/Info.plist")).ok()?;
    let name = info.as_dictionary()?.get("CFBundleIconFile")?.as_string()?;
    let file = if Path::new(name).extension().and_then(|ext| ext.to_str()) == Some("icns") {
        name.to_owned()
    } else {
        format!("{name}.icns")
    };
    Some(bundle.join("Contents/Resources").join(file))
}

fn decode_icns(path: &Path) -> Option<OpenerIcon> {
    let file = std::fs::File::open(path).ok()?;
    let family = icns::IconFamily::read(std::io::BufReader::new(file)).ok()?;
    let icon_type = pick_icon_type(&family.available_icons())?;
    let image = family
        .get_icon_with_type(icon_type)
        .ok()?
        .convert_to(icns::PixelFormat::RGBA);
    Some(OpenerIcon {
        width: image.width() as usize,
        height: image.height() as usize,
        rgba: image.data().to_vec(),
    })
}

/// Display size below which icns entries are too coarse for a crisp glyph. The
/// smallest entry at least this wide is preferred, so a 128px icon is decoded
/// rather than the 512/1024px variants when both are present.
const ICON_TARGET_PX: u32 = 128;

fn pick_icon_type(available: &[icns::IconType]) -> Option<icns::IconType> {
    available
        .iter()
        .filter(|icon_type| icon_type.pixel_width() >= ICON_TARGET_PX)
        .min_by_key(|icon_type| icon_type.pixel_width())
        .or_else(|| {
            available
                .iter()
                .max_by_key(|icon_type| icon_type.pixel_width())
        })
        .copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn default_opener_is_zed_until_preferences_can_override_it() {
        assert_eq!(WorkspaceOpener::default(), WorkspaceOpener::Zed);
    }

    #[test]
    fn finder_opens_the_folder_with_macos_open() {
        let path = PathBuf::from("/tmp/project");
        let command = command_for(WorkspaceOpener::Finder, &path);

        assert_eq!(command.program, "open");
        assert_eq!(command.args, vec![OsString::from("/tmp/project")]);
    }

    #[test]
    fn editors_are_selected_with_open_a() {
        let path = PathBuf::from("/tmp/project");
        let command = command_for(WorkspaceOpener::Cursor, &path);

        assert_eq!(command.program, "open");
        assert_eq!(
            command.args,
            vec![
                OsString::from("-a"),
                OsString::from("Cursor"),
                OsString::from("/tmp/project")
            ]
        );
    }

    #[test]
    fn cursor_opens_the_file_as_a_tab_of_a_new_workspace_window() {
        let command = ide_command(
            WorkspaceOpener::Cursor,
            PathBuf::from("/Applications/Cursor.app/Contents/Resources/app/bin/cursor"),
            Path::new("/tmp/project"),
            Some(Path::new("/tmp/project/src/main.rs")),
        );

        assert_eq!(
            command.program,
            "/Applications/Cursor.app/Contents/Resources/app/bin/cursor"
        );
        assert_eq!(
            command.args,
            vec![
                OsString::from("--new-window"),
                OsString::from("/tmp/project"),
                OsString::from("--goto"),
                OsString::from("/tmp/project/src/main.rs")
            ]
        );
    }

    #[test]
    fn zed_opens_the_workspace_and_file_as_plain_paths_in_a_matched_window() {
        let command = ide_command(
            WorkspaceOpener::Zed,
            PathBuf::from("/Applications/Zed.app/Contents/MacOS/cli"),
            Path::new("/tmp/project"),
            Some(Path::new("/tmp/project/src/main.rs")),
        );

        assert_eq!(command.program, "/Applications/Zed.app/Contents/MacOS/cli");
        assert_eq!(
            command.args,
            vec![
                OsString::from("--classic"),
                OsString::from("/tmp/project"),
                OsString::from("/tmp/project/src/main.rs")
            ]
        );
    }

    // `--new` forces a second window (and a full project reload) for a project Zed
    // already has open: the flag must never come back.
    #[test]
    fn zed_never_forces_an_unconditionally_new_window() {
        for file in [None, Some(Path::new("/tmp/project/src/main.rs"))] {
            let command = ide_command(
                WorkspaceOpener::Zed,
                PathBuf::from("/Applications/Zed.app/Contents/MacOS/cli"),
                Path::new("/tmp/project"),
                file,
            );
            assert!(!command.args.contains(&OsString::from("--new")));
        }
    }

    #[test]
    fn ide_opens_the_workspace_without_a_file() {
        let cursor = ide_command(
            WorkspaceOpener::Cursor,
            PathBuf::from("/Applications/Cursor.app/Contents/Resources/app/bin/cursor"),
            Path::new("/tmp/project"),
            None,
        );
        assert_eq!(
            cursor.args,
            vec![
                OsString::from("--new-window"),
                OsString::from("/tmp/project")
            ]
        );

        let zed = ide_command(
            WorkspaceOpener::Zed,
            PathBuf::from("/Applications/Zed.app/Contents/MacOS/cli"),
            Path::new("/tmp/project"),
            None,
        );
        assert_eq!(
            zed.args,
            vec![OsString::from("--classic"), OsString::from("/tmp/project")]
        );
    }

    #[test]
    fn only_the_ide_openers_have_a_bundled_cli() {
        let ides: Vec<WorkspaceOpener> = WorkspaceOpener::ALL
            .into_iter()
            .filter(|opener| opener.ide_cli().is_some())
            .collect();
        assert_eq!(ides, vec![WorkspaceOpener::Cursor, WorkspaceOpener::Zed]);
    }

    #[test]
    fn gitkraken_opens_the_repo_via_its_url_scheme() {
        let path = PathBuf::from("/tmp/my project");
        let command = command_for(WorkspaceOpener::GitKraken, &path);

        assert_eq!(command.program, "open");
        assert_eq!(
            command.args,
            vec![OsString::from("gitkraken://repo/tmp/my%20project")]
        );
    }

    #[test]
    fn launcher_order_matches_the_menu() {
        let labels: Vec<&str> = WorkspaceOpener::ALL.iter().map(|o| o.label()).collect();
        assert_eq!(
            labels,
            vec![
                "Cursor",
                "Zed",
                "Finder",
                "Terminal",
                "Ghostty",
                "GitKraken"
            ]
        );
    }

    #[test]
    fn installed_filter_keeps_system_apps_and_present_bundles_in_menu_order() {
        let installed = installed_openers_where(|bundle| bundle == "GitKraken.app");
        assert_eq!(
            installed,
            vec![
                WorkspaceOpener::Finder,
                WorkspaceOpener::Terminal,
                WorkspaceOpener::GitKraken
            ]
        );
    }

    #[test]
    fn installed_filter_keeps_only_system_apps_when_nothing_else_is_present() {
        let installed = installed_openers_where(|_| false);
        assert_eq!(
            installed,
            vec![WorkspaceOpener::Finder, WorkspaceOpener::Terminal]
        );
    }

    #[test]
    fn resolve_default_keeps_the_preferred_opener_when_installed() {
        let installed = vec![WorkspaceOpener::Finder, WorkspaceOpener::Zed];
        assert_eq!(
            resolve_default(WorkspaceOpener::Zed, &installed),
            WorkspaceOpener::Zed
        );
    }

    #[test]
    fn resolve_default_falls_back_to_the_first_installed_when_preferred_is_gone() {
        let installed = vec![WorkspaceOpener::Finder, WorkspaceOpener::Terminal];
        assert_eq!(
            resolve_default(WorkspaceOpener::Zed, &installed),
            WorkspaceOpener::Finder
        );
    }

    fn icon_types(sizes: &[u32]) -> Vec<icns::IconType> {
        sizes
            .iter()
            .filter_map(|&size| icns::IconType::from_pixel_size(size, size))
            .collect()
    }

    #[test]
    fn pick_icon_type_prefers_the_smallest_entry_at_least_the_target_size() {
        let available = icon_types(&[16, 32, 128, 256, 512]);
        assert_eq!(
            pick_icon_type(&available).map(|t| t.pixel_width()),
            Some(ICON_TARGET_PX)
        );
    }

    #[test]
    fn pick_icon_type_falls_back_to_the_largest_when_all_below_target() {
        let available = icon_types(&[16, 32]);
        assert_eq!(
            pick_icon_type(&available).map(|t| t.pixel_width()),
            Some(32)
        );
    }

    #[test]
    fn pick_icon_type_is_none_for_an_empty_family() {
        assert!(pick_icon_type(&[]).is_none());
    }

    // Real-resource check: macOS always ships Terminal.app, so on the target its
    // bundle icon must resolve and decode to a non-empty RGBA buffer (mandatory,
    // not skipped — a silent skip would let the test pass without exercising the
    // decode). Inert on non-macOS, where the bundle is legitimately absent.
    #[test]
    fn decodes_a_real_system_app_icon() {
        let bundle = WorkspaceOpener::Terminal.bundle_path();
        if cfg!(target_os = "macos") {
            let bundle = bundle.expect("macOS always ships Terminal.app");
            let icns = bundle_icns_path(&bundle).expect("Terminal exposes CFBundleIconFile");
            let icon = decode_icns(&icns).expect("Terminal icon decodes to RGBA");
            assert!(icon.width >= ICON_TARGET_PX as usize);
            assert_eq!(icon.rgba.len(), icon.width * icon.height * 4);
        }
    }
}
