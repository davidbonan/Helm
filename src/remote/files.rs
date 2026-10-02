//! The files of an agent's worktree, as the phone browses and opens them
//! (specs/remote.md §7.3): read only, never outside the worktree, never its `.git`.

use std::fs::{DirEntry, File};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::Serialize;

/// Entries a single listing carries: the newest ones.
pub const MAX_LISTED: usize = 500;

/// Read from a file of unknown extension to tell text from binary.
const SNIFFED_BYTES: u64 = 8 * 1024;

const GIT_DIR: &str = ".git";
const TEXT: &str = "text/plain; charset=utf-8";
const BINARY: &str = "application/octet-stream";
const HTML: &str = "text/html; charset=utf-8";
const SVG: &str = "image/svg+xml";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileRow {
    pub name: String,
    pub dir: bool,
    pub size: u64,
    pub modified_ms: u64,
}

impl FileRow {
    /// `None` for what the phone never sees: `.git`, a symlink, a name that is not UTF-8.
    fn of(entry: &DirEntry) -> Option<Self> {
        let name = entry.file_name().into_string().ok()?;
        let metadata = entry.metadata().ok()?;
        if metadata.is_symlink() || name == GIT_DIR {
            return None;
        }
        let modified = metadata.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
        Some(Self {
            name,
            dir: metadata.is_dir(),
            size: metadata.len(),
            modified_ms: modified.as_millis() as u64,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Listing {
    pub entries: Vec<FileRow>,
    /// Every entry of the directory, listed or not.
    pub total: usize,
}

/// `relative` inside the worktree `root`, symlinks followed: `None` when it leaves
/// the worktree, enters `.git` or does not exist.
pub fn resolve(root: &Path, relative: &str) -> Option<PathBuf> {
    let root = root.canonicalize().ok()?;
    let path = root.join(relative).canonicalize().ok()?;
    let inside = path.strip_prefix(&root).ok()?;
    let in_git = inside.components().any(|part| part.as_os_str() == GIT_DIR);
    (!in_git).then_some(path)
}

/// The directory's entries, newest first.
pub fn list(dir: &Path) -> io::Result<Listing> {
    let mut entries: Vec<FileRow> = std::fs::read_dir(dir)?
        .filter_map(|entry| FileRow::of(&entry.ok()?))
        .collect();
    entries.sort_by(|a, b| {
        b.modified_ms
            .cmp(&a.modified_ms)
            .then_with(|| a.name.cmp(&b.name))
    });
    let total = entries.len();
    entries.truncate(MAX_LISTED);
    Ok(Listing { entries, total })
}

/// A regular file ready to be sent, read from its start.
pub struct OpenedFile {
    pub file: File,
    pub len: u64,
    pub content_type: &'static str,
}

impl OpenedFile {
    /// `None` for a directory or a file that cannot be read.
    pub fn open(path: &Path) -> Option<Self> {
        let mut file = File::open(path).ok()?;
        let metadata = file.metadata().ok()?;
        if !metadata.is_file() {
            return None;
        }
        let content_type = match native_type(path) {
            Some(native) => native,
            None => sniffed_type(&mut file).ok()?,
        };
        Some(Self {
            file,
            len: metadata.len(),
            content_type,
        })
    }

    /// Opened as a page, it would run its scripts in the phone page's origin.
    pub fn runs_scripts(&self) -> bool {
        matches!(self.content_type, HTML | SVG)
    }
}

/// What a browser shows by itself, from the extension.
fn native_type(path: &Path) -> Option<&'static str> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "heic" => "image/heic",
        "svg" => SVG,
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "aac" => "audio/aac",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "flac" => "audio/flac",
        "pdf" => "application/pdf",
        "html" | "htm" => HTML,
        _ => return None,
    })
}

/// Text when the file's first bytes are UTF-8 without a NUL; leaves the file at its start.
fn sniffed_type(file: &mut File) -> io::Result<&'static str> {
    let mut head = Vec::new();
    file.by_ref().take(SNIFFED_BYTES).read_to_end(&mut head)?;
    file.seek(SeekFrom::Start(0))?;
    let is_utf8 = match std::str::from_utf8(&head) {
        Ok(_) => true,
        // A character cut by the sniffed length, not an invalid byte.
        Err(err) => err.error_len().is_none(),
    };
    Ok(if is_utf8 && !head.contains(&0) {
        TEXT
    } else {
        BINARY
    })
}
