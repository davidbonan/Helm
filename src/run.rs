//! Auto-detected run command for a project's "Run" terminal (git.md §3): the
//! build/serve command inferred from the workdir's manifest, used when no explicit
//! per-project override is configured. Detection is filename-driven (only the dev
//! script reads a file), so it stays cheap enough to call on demand.

use std::path::Path;

/// Auto-assigned base port for a project group's `$PORT` substitution when none is
/// configured (git.md §3): worktree #0 (the root) gets this, each later worktree
/// the next integer up.
pub const DEFAULT_BASE_PORT: u16 = 3000;

/// Port a worktree's `$PORT` resolves to (git.md §3): its manual override when set,
/// otherwise the group's base port plus the worktree's offset within the group.
pub fn resolved_port(base: u16, offset: usize, override_port: Option<u16>) -> u16 {
    override_port.unwrap_or_else(|| base.saturating_add(offset.min(u16::MAX as usize) as u16))
}

/// Substitutes `$PORT` and `${PORT}` in `command` with `port`. A `$PORT` glued to
/// another identifier char (`$PORTAL`) is left alone; the command is returned
/// unchanged when no placeholder occurs (git.md §3).
pub fn apply_port(command: &str, port: u16) -> String {
    let replacement = port.to_string();
    let bytes = command.as_bytes();
    let mut out = String::with_capacity(command.len());
    let mut i = 0;
    while i < command.len() {
        let rest = &command[i..];
        if rest.starts_with("${PORT}") {
            out.push_str(&replacement);
            i += "${PORT}".len();
            continue;
        }
        if rest.starts_with("$PORT")
            && !matches!(bytes.get(i + 5), Some(c) if *c == b'_' || c.is_ascii_alphanumeric())
        {
            out.push_str(&replacement);
            i += "$PORT".len();
            continue;
        }
        let ch = rest.chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Whether `command` references the `$PORT` placeholder — the Run strip only
/// surfaces the port chip for commands that consume it (git.md §3).
pub fn uses_port(command: &str) -> bool {
    apply_port(command, 0) != command
}

/// Best-guess command to run the project at `workdir`, or `None` when no known
/// manifest is found — the Run panel then asks for an explicit command.
pub fn detect_run_command(workdir: &Path) -> Option<String> {
    if workdir.join("Cargo.toml").is_file() {
        return Some("cargo run".to_owned());
    }
    if workdir.join("package.json").is_file() {
        return Some(node_run_command(workdir));
    }
    if workdir.join("go.mod").is_file() {
        return Some("go run .".to_owned());
    }
    None
}

/// `<pm> run <script>`: the package manager inferred from the lockfile and the
/// first conventional dev script present in `package.json` (falling back to `dev`,
/// the near-universal convention, when none parses).
fn node_run_command(workdir: &Path) -> String {
    let pm = node_package_manager(workdir);
    let script = node_dev_script(workdir).unwrap_or_else(|| "dev".to_owned());
    format!("{pm} run {script}")
}

fn node_package_manager(workdir: &Path) -> &'static str {
    if workdir.join("bun.lockb").is_file() || workdir.join("bun.lock").is_file() {
        "bun"
    } else if workdir.join("pnpm-lock.yaml").is_file() {
        "pnpm"
    } else if workdir.join("yarn.lock").is_file() {
        "yarn"
    } else {
        "npm"
    }
}

fn node_dev_script(workdir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(workdir.join("package.json")).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    let scripts = json.get("scripts")?.as_object()?;
    ["dev", "start", "serve"]
        .into_iter()
        .find(|name| scripts.contains_key(*name))
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn cargo_project_runs_cargo() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("Cargo.toml"), "[package]\n").unwrap();
        assert_eq!(detect_run_command(dir.path()).as_deref(), Some("cargo run"));
    }

    #[test]
    fn go_module_runs_go() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("go.mod"), "module x\n").unwrap();
        assert_eq!(detect_run_command(dir.path()).as_deref(), Some("go run ."));
    }

    #[test]
    fn no_manifest_yields_none() {
        let dir = tempdir().unwrap();
        assert_eq!(detect_run_command(dir.path()), None);
    }

    #[test]
    fn node_picks_lockfile_manager_and_dev_script() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{"scripts": {"build": "x", "dev": "vite"}}"#,
        )
        .unwrap();
        fs::write(dir.path().join("pnpm-lock.yaml"), "").unwrap();
        assert_eq!(
            detect_run_command(dir.path()).as_deref(),
            Some("pnpm run dev")
        );
    }

    #[test]
    fn node_prefers_dev_then_start_then_serve() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{"scripts": {"serve": "x", "start": "y"}}"#,
        )
        .unwrap();
        assert_eq!(
            detect_run_command(dir.path()).as_deref(),
            Some("npm run start")
        );
    }

    #[test]
    fn node_defaults_to_dev_when_no_known_script() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("package.json"), r#"{"name": "x"}"#).unwrap();
        fs::write(dir.path().join("yarn.lock"), "").unwrap();
        assert_eq!(
            detect_run_command(dir.path()).as_deref(),
            Some("yarn run dev")
        );
    }

    #[test]
    fn node_bun_lockfile_detected() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{"scripts": {"dev": "x"}}"#,
        )
        .unwrap();
        fs::write(dir.path().join("bun.lockb"), "").unwrap();
        assert_eq!(
            detect_run_command(dir.path()).as_deref(),
            Some("bun run dev")
        );
    }

    #[test]
    fn port_offset_counts_up_from_the_base() {
        assert_eq!(resolved_port(3000, 0, None), 3000);
        assert_eq!(resolved_port(3000, 2, None), 3002);
        assert_eq!(resolved_port(8080, 1, None), 8081);
    }

    #[test]
    fn port_override_wins_over_the_offset() {
        assert_eq!(resolved_port(3000, 5, Some(4000)), 4000);
    }

    #[test]
    fn apply_port_substitutes_both_forms() {
        assert_eq!(
            apply_port("npm run dev -- --port $PORT", 3001),
            "npm run dev -- --port 3001"
        );
        assert_eq!(
            apply_port("serve --port=${PORT}", 3001),
            "serve --port=3001"
        );
        assert_eq!(apply_port("a $PORT b ${PORT}", 80), "a 80 b 80");
    }

    #[test]
    fn apply_port_leaves_glued_identifiers_and_plain_commands() {
        assert_eq!(apply_port("echo $PORTAL", 3001), "echo $PORTAL");
        assert_eq!(apply_port("$PORT_X", 3001), "$PORT_X");
        assert_eq!(apply_port("cargo run", 3001), "cargo run");
    }

    #[test]
    fn uses_port_detects_the_placeholder() {
        assert!(uses_port("vite --port $PORT"));
        assert!(uses_port("vite --port ${PORT}"));
        assert!(!uses_port("vite"));
        assert!(!uses_port("echo $PORTAL"));
    }

    #[test]
    fn cargo_takes_precedence_over_node() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("Cargo.toml"), "[package]\n").unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{"scripts": {"dev": "x"}}"#,
        )
        .unwrap();
        assert_eq!(detect_run_command(dir.path()).as_deref(), Some("cargo run"));
    }
}
