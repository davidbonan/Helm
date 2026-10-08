//! A worktree file as the read-only viewer shows it (specs/files.md §4): its lines,
//! its image, or the placeholder standing in for them.

use std::fs::{self, Metadata};
use std::path::Path;
use std::time::SystemTime;

use crate::git::diff::{is_image_path, ImageBlob, MAX_DIFF_BYTES, MAX_DIFF_LINES, MAX_IMAGE_BYTES};

/// What a re-read compares: a file whose mtime and size both held is not read again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamp {
    modified: Option<SystemTime>,
    size: u64,
}

impl Stamp {
    fn of(metadata: &Metadata) -> Self {
        Self {
            modified: metadata.modified().ok(),
            size: metadata.len(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Content {
    /// One entry per line, line terminators dropped.
    Text(Vec<String>),
    Image(ImageBlob),
    /// The link's target: a symlink is never followed (files.md §3).
    Symlink(String),
    Binary,
    TooLarge,
    Unreadable,
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSnapshot {
    /// Relative to the worktree, `/`-separated.
    pub path: String,
    pub size: u64,
    /// `None` while the file is gone.
    pub stamp: Option<Stamp>,
    pub content: Content,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadOutcome {
    /// The stamp of the content on screen still holds.
    Unchanged,
    Read(FileSnapshot),
}

/// Reads `relative` inside the worktree `root` unless `known`, the stamp of the
/// content on screen, still holds. A symlink is read as its target path.
pub fn read(root: &Path, relative: &str, known: Option<Stamp>) -> ReadOutcome {
    let full = root.join(relative);
    let Ok(metadata) = fs::symlink_metadata(&full) else {
        return ReadOutcome::Read(FileSnapshot {
            path: relative.to_owned(),
            size: 0,
            stamp: None,
            content: Content::Missing,
        });
    };
    let stamp = Stamp::of(&metadata);
    if known == Some(stamp) {
        return ReadOutcome::Unchanged;
    }
    ReadOutcome::Read(FileSnapshot {
        path: relative.to_owned(),
        size: metadata.len(),
        stamp: Some(stamp),
        content: content_of(&full, relative, &metadata),
    })
}

/// What the viewer shows for `bytes`, the content of `relative`: the diff's caps
/// (git.md §8) apply, an image previews up to the diff's image cap.
pub fn classify(relative: &str, bytes: Vec<u8>) -> Content {
    if bytes.len() as u64 > read_cap(relative) {
        return Content::TooLarge;
    }
    if is_image_path(relative) && !bytes.is_empty() {
        return Content::Image(ImageBlob::new(bytes));
    }
    if bytes.contains(&0) {
        return Content::Binary;
    }
    let Ok(text) = String::from_utf8(bytes) else {
        return Content::Binary;
    };
    let lines: Vec<String> = text.lines().map(str::to_owned).collect();
    if lines.len() > MAX_DIFF_LINES {
        return Content::TooLarge;
    }
    Content::Text(lines)
}

fn content_of(full: &Path, relative: &str, metadata: &Metadata) -> Content {
    if metadata.is_symlink() {
        return fs::read_link(full).map_or(Content::Unreadable, |target| {
            Content::Symlink(target.to_string_lossy().into_owned())
        });
    }
    if metadata.is_dir() {
        return Content::Missing;
    }
    // A fifo or a device would block the read for good.
    if !metadata.is_file() {
        return Content::Binary;
    }
    if metadata.len() > read_cap(relative) {
        return Content::TooLarge;
    }
    fs::read(full).map_or(Content::Unreadable, |bytes| classify(relative, bytes))
}

fn read_cap(relative: &str) -> u64 {
    let cap = if is_image_path(relative) {
        MAX_IMAGE_BYTES
    } else {
        MAX_DIFF_BYTES
    };
    cap as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_splits_into_lines_without_their_terminators() {
        let content = classify("a.rs", b"fn main() {\r\n}\n\nend".to_vec());

        assert_eq!(
            content,
            Content::Text(vec![
                "fn main() {".to_owned(),
                "}".to_owned(),
                String::new(),
                "end".to_owned(),
            ])
        );
    }

    #[test]
    fn a_nul_byte_or_invalid_utf8_is_binary() {
        assert_eq!(classify("blob.dat", vec![b'a', 0, b'b']), Content::Binary);
        assert_eq!(classify("latin1.txt", vec![b'c', 0xE9]), Content::Binary);
    }

    #[test]
    fn text_past_two_megabytes_or_fifty_thousand_lines_is_too_large() {
        let at_cap = "x\n".repeat(MAX_DIFF_LINES);
        let past_lines = "x\n".repeat(MAX_DIFF_LINES + 1);
        let past_bytes = vec![b'x'; MAX_DIFF_BYTES + 1];

        assert!(
            matches!(classify("a.txt", at_cap.into_bytes()), Content::Text(lines) if lines.len() == MAX_DIFF_LINES)
        );
        assert_eq!(
            classify("a.txt", past_lines.into_bytes()),
            Content::TooLarge
        );
        assert_eq!(classify("a.txt", past_bytes), Content::TooLarge);
    }

    #[test]
    fn an_image_previews_past_the_text_cap() {
        let bytes = vec![1; MAX_DIFF_BYTES + 1];

        assert_eq!(
            classify("shots/Logo.PNG", bytes.clone()),
            Content::Image(ImageBlob::new(bytes))
        );
    }
}
