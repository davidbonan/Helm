use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffSource {
    Unstaged,
    Staged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineOrigin {
    Context,
    Addition,
    Deletion,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub origin: LineOrigin,
    pub content: String,
    pub old_lineno: Option<u32>,
    pub new_lineno: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    pub header: String,
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDiff {
    pub path: String,
    pub binary: bool,
    /// `true` when the diff exceeds the inline display thresholds (git.md §8): the
    /// hunks are not loaded, the UI shows a summary and staging stays at the file
    /// level (as for a binary).
    pub oversize: bool,
    pub hunks: Vec<Hunk>,
    /// Full content of the file on the **new** side of the diff (workdir, index or
    /// commit), one entry per line: the material for the diff view's context
    /// expansion (git.md §4). Outside the hunks both sides are identical, so these
    /// lines hold for both numberings. Empty if binary, oversize or the new side is
    /// absent (deleted file): nothing to expand.
    pub source_lines: Vec<String>,
    /// New-side bytes of an image file (binary with a recognized image extension),
    /// for the diff view's image preview (git.md §4). `None` for any other file, a
    /// deleted image (no new side) or one above `MAX_IMAGE_BYTES`.
    pub image: Option<ImageBlob>,
}

/// Decodable image content of a `FileDiff`, plus a fingerprint of the bytes so the
/// diff view can cache the decoded texture and re-decode only when it changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageBlob {
    pub bytes: Vec<u8>,
    pub fingerprint: u64,
}

/// Cap on the bytes we carry to the UI for an image preview: above it the texture
/// upload and decode would stall the frame; the file stays on the binary placeholder.
pub(crate) const MAX_IMAGE_BYTES: usize = 32 * 1024 * 1024;

fn is_image_path(path: &str) -> bool {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    matches!(
        ext.as_deref(),
        Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico" | "tiff" | "tif")
    )
}

/// Wraps the new-side bytes of an image file into an `ImageBlob`, or `None` when the
/// path is not an image extension or the content is empty / above `MAX_IMAGE_BYTES`.
fn image_blob(path: &str, bytes: Vec<u8>) -> Option<ImageBlob> {
    if bytes.is_empty() || bytes.len() > MAX_IMAGE_BYTES || !is_image_path(path) {
        return None;
    }
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    let fingerprint = hasher.finish();
    Some(ImageBlob { bytes, fingerprint })
}

/// New-side bytes of a path in a tree (commit / stash side), for the image preview.
fn tree_blob_bytes(repo: &git2::Repository, tree: &git2::Tree, path: &str) -> Option<Vec<u8>> {
    let entry = tree.get_path(Path::new(path)).ok()?;
    Some(repo.find_blob(entry.id()).ok()?.content().to_vec())
}

/// Beyond these thresholds we do not load the line-by-line diff (git.md §8): the
/// inline rendering would be unreadable and granular staging impractical.
pub(crate) const MAX_DIFF_BYTES: usize = 2 * 1024 * 1024;
pub(crate) const MAX_DIFF_LINES: usize = 50_000;

pub fn file_diff(
    repo: &git2::Repository,
    path: &str,
    source: DiffSource,
) -> Result<FileDiff, git2::Error> {
    let mut opts = git2::DiffOptions::new();
    opts.pathspec(path)
        .disable_pathspec_match(true)
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .show_binary(true);

    let diff = match source {
        DiffSource::Unstaged => repo.diff_index_to_workdir(None, Some(&mut opts))?,
        DiffSource::Staged => {
            let head_tree = repo.head().ok().and_then(|h| h.peel_to_tree().ok());
            repo.diff_tree_to_index(head_tree.as_ref(), None, Some(&mut opts))?
        }
    };

    let Some(idx) = delta_index(&diff, path) else {
        return Ok(FileDiff {
            path: path.to_string(),
            binary: false,
            oversize: false,
            hunks: Vec::new(),
            source_lines: Vec::new(),
            image: None,
        });
    };

    // libgit2 does not compute the line-by-line content of an untracked file in an
    // index→workdir diff: diff it against an empty buffer to get the additions.
    if diff.get_delta(idx).map(|d| d.status()) == Some(git2::Delta::Untracked) {
        return untracked_file_diff(repo, path);
    }
    let mut file = patch_to_file_diff(&diff, idx, path)?;
    if file.binary {
        if is_image_path(path) {
            file.image = new_side_bytes(repo, path, source).and_then(|b| image_blob(path, b));
        }
    } else if !file.hunks.is_empty() {
        file.source_lines = new_side_bytes(repo, path, source)
            .map(|bytes| source_lines_from(&bytes))
            .unwrap_or_default();
    }
    Ok(file)
}

/// Bytes of the file's new side per source: working tree (Unstaged) or the index
/// blob (Staged). `None` if that side does not exist (deletion).
fn new_side_bytes(repo: &git2::Repository, path: &str, source: DiffSource) -> Option<Vec<u8>> {
    match source {
        DiffSource::Unstaged => std::fs::read(repo.workdir()?.join(path)).ok(),
        DiffSource::Staged => {
            let entry = repo.index().ok()?.get_path(Path::new(path), 0)?;
            Some(repo.find_blob(entry.id).ok()?.content().to_vec())
        }
    }
}

fn source_lines_from(bytes: &[u8]) -> Vec<String> {
    if bytes.contains(&0) {
        return Vec::new();
    }
    String::from_utf8_lossy(bytes)
        .lines()
        .map(str::to_owned)
        .collect()
}

/// Diff a single file of a commit against its first parent (root commit ⇒ vs the
/// empty tree, so everything is an addition; merge ⇒ vs the first parent). This is
/// the read-only line-by-line diff M9-2 deferred: `commit_detail` lists the changed
/// files, this loads the hunks of one of them on demand (git.md §9).
pub fn commit_file_diff(
    repo: &git2::Repository,
    oid: git2::Oid,
    path: &str,
) -> Result<FileDiff, git2::Error> {
    let commit = repo.find_commit(oid)?;
    let new_tree = commit.tree()?;
    let parent_tree = match commit.parent(0) {
        Ok(parent) => Some(parent.tree()?),
        Err(_) => None,
    };

    // No pathspec: it would filter out the old path of a rename, which would stop
    // `find_similar` from pairing the two sides — the file would show as 100%
    // additions while the detail reports it as `Renamed`. Same detection as
    // `commit_detail::load_repo` to stay consistent.
    let mut opts = git2::DiffOptions::new();
    opts.show_binary(true);
    let mut diff =
        repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&new_tree), Some(&mut opts))?;
    let mut find = git2::DiffFindOptions::new();
    find.renames(true);
    diff.find_similar(Some(&mut find))?;

    let Some(idx) = delta_index(&diff, path) else {
        // Stash row (git.md §9): a stashed untracked file is absent from the
        // stash tree (it lives in the 3rd parent) — fall back on that tree.
        if let Some(file) = stash_untracked_file_diff(repo, &commit, path)? {
            return Ok(file);
        }
        return Ok(FileDiff {
            path: path.to_string(),
            binary: false,
            oversize: false,
            hunks: Vec::new(),
            source_lines: Vec::new(),
            image: None,
        });
    };
    let mut file = patch_to_file_diff(&diff, idx, path)?;
    if file.binary {
        if is_image_path(path) {
            file.image = tree_blob_bytes(repo, &new_tree, path).and_then(|b| image_blob(path, b));
        }
    } else if !file.hunks.is_empty() {
        file.source_lines = new_tree
            .get_path(Path::new(path))
            .ok()
            .and_then(|entry| repo.find_blob(entry.id()).ok())
            .map(|blob| source_lines_from(blob.content()))
            .unwrap_or_default();
    }
    Ok(file)
}

/// Diff of one file of a stash's **untracked commit** (3rd parent) against the
/// empty tree — all additions, like the untracked working-tree diff. `None`
/// when the commit is not a stash or the file is not in its untracked tree.
fn stash_untracked_file_diff(
    repo: &git2::Repository,
    commit: &git2::Commit,
    path: &str,
) -> Result<Option<FileDiff>, git2::Error> {
    let Some(tree) = crate::git::stash::untracked_tree(repo, commit)? else {
        return Ok(None);
    };
    let mut opts = git2::DiffOptions::new();
    opts.show_binary(true).pathspec(path);
    let diff = repo.diff_tree_to_tree(None, Some(&tree), Some(&mut opts))?;
    let Some(idx) = delta_index(&diff, path) else {
        return Ok(None);
    };
    let mut file = patch_to_file_diff(&diff, idx, path)?;
    if file.binary {
        if is_image_path(path) {
            file.image = tree_blob_bytes(repo, &tree, path).and_then(|b| image_blob(path, b));
        }
    } else if !file.hunks.is_empty() {
        file.source_lines = tree
            .get_path(Path::new(path))
            .ok()
            .and_then(|entry| repo.find_blob(entry.id()).ok())
            .map(|blob| source_lines_from(blob.content()))
            .unwrap_or_default();
    }
    Ok(Some(file))
}

fn delta_index(diff: &git2::Diff, path: &str) -> Option<usize> {
    let target = Path::new(path);
    diff.deltas()
        .position(|delta| delta_path(&delta).is_some_and(|p| p == target))
}

fn untracked_file_diff(repo: &git2::Repository, path: &str) -> Result<FileDiff, git2::Error> {
    let full = repo
        .workdir()
        .map(|wd| wd.join(path))
        .ok_or_else(|| git2::Error::from_str("bare repository has no workdir"))?;
    let content =
        std::fs::read(&full).map_err(|e| git2::Error::from_str(&format!("read {path}: {e}")))?;
    if content.contains(&0) {
        return Ok(FileDiff {
            path: path.to_string(),
            binary: true,
            oversize: false,
            hunks: Vec::new(),
            source_lines: Vec::new(),
            image: image_blob(path, content),
        });
    }
    let rel = Path::new(path);
    let patch = git2::Patch::from_buffers(b"", Some(rel), &content, Some(rel), None)?;
    let mut file = file_diff_from_patch(patch, path)?;
    if !file.hunks.is_empty() {
        file.source_lines = source_lines_from(&content);
    }
    Ok(file)
}

fn delta_path<'a>(delta: &'a git2::DiffDelta<'a>) -> Option<&'a Path> {
    delta.new_file().path().or_else(|| delta.old_file().path())
}

fn patch_to_file_diff(diff: &git2::Diff, idx: usize, path: &str) -> Result<FileDiff, git2::Error> {
    match git2::Patch::from_diff(diff, idx)? {
        Some(patch) => file_diff_from_patch(patch, path),
        None => Ok(FileDiff {
            path: path.to_string(),
            binary: true,
            oversize: false,
            hunks: Vec::new(),
            source_lines: Vec::new(),
            image: None,
        }),
    }
}

fn file_diff_from_patch(patch: git2::Patch, path: &str) -> Result<FileDiff, git2::Error> {
    // A binary file produces a patch with no text hunk but carries the BINARY
    // flag on its delta: that is the reliable signal (git.md §8). `from_diff`
    // returns `None` only in some of the binary cases.
    if patch.delta().flags().contains(git2::DiffFlags::BINARY) {
        return Ok(FileDiff {
            path: path.to_string(),
            binary: true,
            oversize: false,
            hunks: Vec::new(),
            source_lines: Vec::new(),
            image: None,
        });
    }
    if is_oversize(&patch)? {
        return Ok(FileDiff {
            path: path.to_string(),
            binary: false,
            oversize: true,
            hunks: Vec::new(),
            source_lines: Vec::new(),
            image: None,
        });
    }

    let mut hunks = Vec::with_capacity(patch.num_hunks());
    for hunk_idx in 0..patch.num_hunks() {
        let (hunk, line_count) = patch.hunk(hunk_idx)?;
        let mut lines = Vec::with_capacity(line_count);
        for line_idx in 0..line_count {
            let line = patch.line_in_hunk(hunk_idx, line_idx)?;
            if let Some(origin) = line_origin(line.origin_value()) {
                lines.push(DiffLine {
                    origin,
                    content: String::from_utf8_lossy(line.content()).into_owned(),
                    old_lineno: line.old_lineno(),
                    new_lineno: line.new_lineno(),
                });
            }
        }
        hunks.push(Hunk {
            header: String::from_utf8_lossy(hunk.header()).into_owned(),
            old_start: hunk.old_start(),
            old_lines: hunk.old_lines(),
            new_start: hunk.new_start(),
            new_lines: hunk.new_lines(),
            lines,
        });
    }

    Ok(FileDiff {
        path: path.to_string(),
        binary: false,
        oversize: false,
        hunks,
        source_lines: Vec::new(),
        image: None,
    })
}

/// A diff is "oversize" beyond ~2 MB of text or ~50,000 lines (git.md §8).
/// `Patch::size`/`line_stats` compute these volumes without materializing the hunks.
fn is_oversize(patch: &git2::Patch) -> Result<bool, git2::Error> {
    let bytes = patch.size(true, true, true);
    let (context, additions, deletions) = patch.line_stats()?;
    Ok(bytes > MAX_DIFF_BYTES || context + additions + deletions > MAX_DIFF_LINES)
}

fn line_origin(value: git2::DiffLineType) -> Option<LineOrigin> {
    use git2::DiffLineType as T;
    match value {
        T::Context | T::ContextEOFNL => Some(LineOrigin::Context),
        T::Addition | T::AddEOFNL => Some(LineOrigin::Addition),
        T::Deletion | T::DeleteEOFNL => Some(LineOrigin::Deletion),
        T::FileHeader | T::HunkHeader | T::Binary => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_origin_maps_eofnl_variants_to_base_kinds() {
        assert_eq!(
            line_origin(git2::DiffLineType::ContextEOFNL),
            Some(LineOrigin::Context)
        );
        assert_eq!(
            line_origin(git2::DiffLineType::AddEOFNL),
            Some(LineOrigin::Addition)
        );
        assert_eq!(
            line_origin(git2::DiffLineType::DeleteEOFNL),
            Some(LineOrigin::Deletion)
        );
    }

    #[test]
    fn line_origin_drops_header_and_binary_markers() {
        assert_eq!(line_origin(git2::DiffLineType::FileHeader), None);
        assert_eq!(line_origin(git2::DiffLineType::HunkHeader), None);
        assert_eq!(line_origin(git2::DiffLineType::Binary), None);
    }
}
