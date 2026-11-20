//! Files a page hands over (browser.md §13): where they land, what the shelf shows.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub struct Download {
    /// Unique within the page that started it.
    pub id: u64,
    pub file_name: String,
    /// Once WebKit asked where to write it.
    pub path: Option<PathBuf>,
    pub status: DownloadStatus,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DownloadStatus {
    /// 0.0‥1.0, `None` while the size is unknown.
    Running(Option<f32>),
    Done,
    Failed(String),
}

/// The user's Downloads folder.
pub fn downloads_dir() -> PathBuf {
    directories::UserDirs::new()
        .and_then(|dirs| dirs.download_dir().map(Path::to_path_buf))
        .or_else(|| directories::BaseDirs::new().map(|dirs| dirs.home_dir().join("Downloads")))
        .unwrap_or_else(std::env::temp_dir)
}

/// Where `suggested` lands in `dir`: its last path segment, numbered like
/// Finder (`report (1).csv`) while the name is taken.
pub fn destination(dir: &Path, suggested: &str) -> PathBuf {
    let name = file_name(suggested);
    let (stem, extension) = match name.rfind('.') {
        Some(dot) if dot > 0 => name.split_at(dot),
        _ => (name.as_str(), ""),
    };
    let mut path = dir.join(&name);
    let mut copy = 1;
    while path.exists() {
        path = dir.join(format!("{stem} ({copy}){extension}"));
        copy += 1;
    }
    path
}

fn file_name(suggested: &str) -> String {
    let name = suggested
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .trim()
        .trim_start_matches('.');
    if name.is_empty() {
        "download".to_owned()
    } else {
        name.to_owned()
    }
}

/// A `Content-Disposition` asking to save the response instead of showing it.
pub fn is_attachment(disposition: &str) -> bool {
    disposition
        .split(';')
        .next()
        .is_some_and(|kind| kind.trim().eq_ignore_ascii_case("attachment"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_taken_name_is_numbered_before_its_extension() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("report.csv"), "").unwrap();
        std::fs::write(dir.path().join("report (1).csv"), "").unwrap();
        assert_eq!(
            destination(dir.path(), "report.csv"),
            dir.path().join("report (2).csv")
        );
        std::fs::write(dir.path().join("LICENSE"), "").unwrap();
        assert_eq!(
            destination(dir.path(), "LICENSE"),
            dir.path().join("LICENSE (1)")
        );
    }

    #[test]
    fn a_suggested_name_never_leaves_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            destination(dir.path(), "../../etc/passwd"),
            dir.path().join("passwd")
        );
        assert_eq!(destination(dir.path(), ".."), dir.path().join("download"));
        assert_eq!(
            destination(dir.path(), ".bashrc"),
            dir.path().join("bashrc")
        );
    }

    #[test]
    fn only_an_attachment_disposition_downloads() {
        assert!(is_attachment("attachment; filename=\"a.csv\""));
        assert!(is_attachment(" Attachment"));
        assert!(!is_attachment("inline; filename=\"a.pdf\""));
        assert!(!is_attachment("attachments"));
    }
}
