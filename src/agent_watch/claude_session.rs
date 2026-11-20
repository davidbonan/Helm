//! A Claude Code process's session, read from the files Claude keeps under its
//! config dir (specs/agents.md §6): `sessions/<pid>.json` names the session and its
//! cwd, `projects/<cwd>/<session>.jsonl` is its transcript. Claude's own internal
//! format: anything missing or unparsable reads as no prompt.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Tail of the transcript read for the last prompt: the `last-prompt` entry is
/// re-appended at every turn, so the end of the file always holds one.
const TAIL_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub session_id: String,
    pub cwd: PathBuf,
}

/// Claude's config dir: `CLAUDE_CONFIG_DIR` as the process sees it, else `~/.claude`.
pub fn config_dir(config_env: Option<String>, home: &Path) -> PathBuf {
    match config_env.filter(|dir| !dir.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => home.join(".claude"),
    }
}

/// The session process `pid` runs, from `<config>/sessions/<pid>.json`.
pub fn read_session(config: &Path, pid: i32) -> Option<Session> {
    let text = std::fs::read_to_string(config.join("sessions").join(format!("{pid}.json"))).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    Some(Session {
        session_id: value.get("sessionId")?.as_str()?.to_owned(),
        cwd: PathBuf::from(value.get("cwd")?.as_str()?),
    })
}

/// Folder Claude files a cwd's transcripts under: every non-alphanumeric → `-`.
pub fn project_dir_name(cwd: &Path) -> String {
    cwd.to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// The session's transcript: under its cwd's folder, else found by name among the
/// project folders (Claude shortens the folder of a very long cwd).
pub fn transcript_path(config: &Path, session: &Session) -> Option<PathBuf> {
    let projects = config.join("projects");
    let file = format!("{}.jsonl", session.session_id);
    let direct = projects.join(project_dir_name(&session.cwd)).join(&file);
    if direct.is_file() {
        return Some(direct);
    }
    std::fs::read_dir(&projects)
        .ok()?
        .flatten()
        .map(|dir| dir.path().join(&file))
        .find(|path| path.is_file())
}

/// The last `last-prompt` entry of a transcript tail (JSON lines; a cut first line
/// simply fails to parse).
pub fn last_prompt(tail: &str) -> Option<String> {
    tail.lines().rev().find_map(|line| {
        let value: serde_json::Value = serde_json::from_str(line).ok()?;
        if value.get("type")?.as_str()? != "last-prompt" {
            return None;
        }
        let prompt = value.get("lastPrompt")?.as_str()?.trim();
        (!prompt.is_empty()).then(|| prompt.to_owned())
    })
}

fn read_tail(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(TAIL_BYTES)))
        .ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Last prompts of live Claude processes, each transcript re-read only when its
/// mtime moves (the watcher asks every second).
#[derive(Default)]
pub struct PromptReader {
    cache: HashMap<PathBuf, (SystemTime, Option<String>)>,
}

impl PromptReader {
    pub fn last_prompt(&mut self, config: &Path, pid: i32) -> Option<String> {
        let session = read_session(config, pid)?;
        let path = transcript_path(config, &session)?;
        let modified = std::fs::metadata(&path).ok()?.modified().ok()?;
        if let Some((seen, prompt)) = self.cache.get(&path) {
            if *seen == modified {
                return prompt.clone();
            }
        }
        let prompt = read_tail(&path).as_deref().and_then(last_prompt);
        self.cache.insert(path, (modified, prompt.clone()));
        prompt
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    /// Claude's files for pid 42 running session `s-1` in `cwd`, its transcript filed
    /// under `project_dir` as Claude names it.
    fn claude_config(cwd: &str, project_dir: &str, transcript: &str) -> tempfile::TempDir {
        let config = tempfile::tempdir().unwrap();
        write(
            &config.path().join("sessions/42.json"),
            &format!(r#"{{"pid":42,"sessionId":"s-1","cwd":"{cwd}","status":"idle"}}"#),
        );
        write(
            &config
                .path()
                .join("projects")
                .join(project_dir)
                .join("s-1.jsonl"),
            transcript,
        );
        config
    }

    #[test]
    fn config_dir_follows_the_process_env_else_home() {
        let home = Path::new("/Users/me");
        assert_eq!(
            config_dir(Some("/Users/me/.claude-perso".into()), home),
            PathBuf::from("/Users/me/.claude-perso")
        );
        assert_eq!(config_dir(None, home), PathBuf::from("/Users/me/.claude"));
    }

    #[test]
    fn last_prompt_reads_the_newest_entry_past_a_cut_first_line() {
        let tail = concat!(
            "ated\":true}\n",
            r#"{"type":"last-prompt","lastPrompt":"first ask","sessionId":"s"}"#,
            "\n",
            r#"{"type":"assistant","message":{}}"#,
            "\n",
            r#"{"type":"last-prompt","lastPrompt":"fix the pager","sessionId":"s"}"#,
            "\n",
            r#"{"type":"ai-title","aiTitle":"Pager","sessionId":"s"}"#,
            "\n",
        );
        assert_eq!(last_prompt(tail).as_deref(), Some("fix the pager"));
        assert_eq!(last_prompt(r#"{"type":"user"}"#), None);
    }

    #[test]
    fn reader_follows_pid_to_session_to_transcript() {
        let config = claude_config(
            "/tmp/repo.wt/feat_x",
            "-tmp-repo-wt-feat-x",
            "{\"type\":\"last-prompt\",\"lastPrompt\":\"add the chevron\"}\n",
        );
        let mut reader = PromptReader::default();
        assert_eq!(
            reader.last_prompt(config.path(), 42).as_deref(),
            Some("add the chevron")
        );
        assert_eq!(
            reader.last_prompt(config.path(), 7),
            None,
            "no session file"
        );
    }

    #[test]
    fn transcript_found_by_name_when_its_folder_is_shortened() {
        let config = claude_config("/tmp/x", "-tmp-x", "");
        let moved = config.path().join("projects/-tmp-x-ab12cd/s-1.jsonl");
        write(
            &moved,
            "{\"type\":\"last-prompt\",\"lastPrompt\":\"long cwd\"}\n",
        );
        std::fs::remove_file(config.path().join("projects/-tmp-x/s-1.jsonl")).unwrap();

        let session = read_session(config.path(), 42).unwrap();
        assert_eq!(transcript_path(config.path(), &session), Some(moved));
    }
}
