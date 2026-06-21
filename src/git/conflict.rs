use std::path::Path;

use crate::git::diff::{MAX_DIFF_BYTES, MAX_DIFF_LINES};
use crate::git::stage;

/// A conflicted file read from the index merge stages (conflicts.md §6, §8). The
/// index is the only source of truth — helm keeps no conflict state of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictFile {
    pub path: String,
    pub kind: ConflictKind,
    /// Semantic label of the "ours" side (stage 2), derived from the repo state:
    /// merge ⇒ *Current · ours*, rebase ⇒ *Current · onto* (stages inverted).
    pub ours_label: String,
    /// Semantic label of the "theirs" side (stage 3): merge ⇒ *Incoming · theirs*,
    /// rebase ⇒ *Incoming · your commit*.
    pub theirs_label: String,
    /// The 3-way reconstruction split into stable + conflict regions. Empty for
    /// kinds without a 3-zone editor (delete/modify, binary, oversize).
    pub regions: Vec<Region>,
    /// Stage 1 (common ancestor) present — drives the per-region base toggle.
    pub has_base: bool,
}

/// Conflict class, decided by the stages present in the index (conflicts.md §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictKind {
    /// Stages 1 + 2 + 3.
    BothModified,
    /// Stages 2 + 3, no ancestor.
    AddedByBoth,
    /// Stages 1 + 2, no stage 3 (the other side deleted the file).
    DeletedByThem,
    /// Stages 1 + 3, no stage 2 (our side deleted the file).
    DeletedByUs,
    /// At least one content side is a binary blob — file-level choice only (§7).
    Binary,
    /// A side exceeds the inline thresholds (git.md §8) — file-level only (§7).
    Oversize,
}

/// A run of the reconstructed file: either stable (auto-merged / context) lines
/// or a real conflict carrying the three sides line by line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Region {
    Stable(Vec<String>),
    Conflict {
        ours: Vec<String>,
        theirs: Vec<String>,
        base: Vec<String>,
    },
}

/// Reads the conflict at `path` from the index merge stages: classifies the kind,
/// derives the semantic side labels from the repo state, and (for the editable
/// kinds) reconstructs the regions from a `merge_file` diff3 buffer.
pub fn read_conflict(repo: &git2::Repository, path: &str) -> Result<ConflictFile, git2::Error> {
    let mut index = repo.index()?;
    index.read(false)?;
    let conflict = find_conflict(&index, path)?;

    let (ours_label, theirs_label) = labels_for_state(repo.state());
    let has_base = conflict.ancestor.is_some();
    let kind = classify(repo, &conflict)?;
    let regions = match kind {
        ConflictKind::BothModified | ConflictKind::AddedByBoth => {
            reconstruct_regions(repo, &conflict)?
        }
        _ => Vec::new(),
    };

    Ok(ConflictFile {
        path: path.to_string(),
        kind,
        ours_label,
        theirs_label,
        regions,
        has_base,
    })
}

/// Reads every conflicted file of the index (conflicts.md §8) for the editor's
/// file rail. The merge stages are the source of truth: paths are listed once in
/// index order, then each read individually. One reply for the whole rail keeps
/// it clear of the per-kind staleness gate (worker M17-13).
pub fn read_conflicts(repo: &git2::Repository) -> Result<Vec<ConflictFile>, git2::Error> {
    let mut index = repo.index()?;
    index.read(false)?;
    let mut paths: Vec<String> = Vec::new();
    for conflict in index.conflicts()? {
        let conflict = conflict?;
        if let Some(bytes) = conflict_path(&conflict) {
            let path = String::from_utf8_lossy(bytes).into_owned();
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
    }
    paths.iter().map(|path| read_conflict(repo, path)).collect()
}

/// Writes the resolution chosen in the editor (conflicts.md §2, §8): `content`
/// replaces the working-tree file and `index.add_path` collapses stages 1/2/3 to
/// stage 0; `None` is a delete resolution — the path leaves both the working tree
/// and the index. Runs on the git2 worker, mirroring `stage`'s index write.
pub fn resolve_file(
    repo: &git2::Repository,
    path: &str,
    content: Option<&str>,
) -> Result<(), git2::Error> {
    let workdir = repo
        .workdir()
        .ok_or_else(|| git2::Error::from_str("bare repository has no working tree"))?;
    let rel = Path::new(path);
    let full = workdir.join(rel);
    let mut index = stage::fresh_index(repo)?;
    match content {
        Some(text) => {
            std::fs::write(&full, text).map_err(|err| git2::Error::from_str(&err.to_string()))?;
            index.add_path(rel)?;
        }
        None => {
            if full.exists() {
                std::fs::remove_file(&full)
                    .map_err(|err| git2::Error::from_str(&err.to_string()))?;
            }
            index.remove_path(rel)?;
        }
    }
    index.write()
}

/// Resolves a conflict by taking one whole side (conflicts.md §5): writes that
/// side's blob from the index to the working tree and collapses stages 1/2/3 to
/// stage 0. For binary / oversize files the inline editor cannot compose.
pub fn resolve_file_side(
    repo: &git2::Repository,
    path: &str,
    ours: bool,
) -> Result<(), git2::Error> {
    let workdir = repo
        .workdir()
        .ok_or_else(|| git2::Error::from_str("bare repository has no working tree"))?;
    let conflict = find_conflict(&repo.index()?, path)?;
    let entry = if ours { conflict.our } else { conflict.their }
        .ok_or_else(|| git2::Error::from_str("the chosen side is absent"))?;
    let content = repo.find_blob(entry.id)?.content().to_vec();

    let rel = Path::new(path);
    std::fs::write(workdir.join(rel), content)
        .map_err(|err| git2::Error::from_str(&err.to_string()))?;
    let mut index = stage::fresh_index(repo)?;
    index.add_path(rel)?;
    index.write()
}

fn find_conflict(index: &git2::Index, path: &str) -> Result<git2::IndexConflict, git2::Error> {
    let target = path.as_bytes();
    for conflict in index.conflicts()? {
        let conflict = conflict?;
        if conflict_path(&conflict) == Some(target) {
            return Ok(conflict);
        }
    }
    Err(git2::Error::from_str("no conflict for the requested path"))
}

fn conflict_path(conflict: &git2::IndexConflict) -> Option<&[u8]> {
    conflict
        .our
        .as_ref()
        .or(conflict.ancestor.as_ref())
        .or(conflict.their.as_ref())
        .map(|entry| entry.path.as_slice())
}

fn classify(
    repo: &git2::Repository,
    conflict: &git2::IndexConflict,
) -> Result<ConflictKind, git2::Error> {
    let (binary, oversize) = if conflict.our.is_some() && conflict.their.is_some() {
        let blobs = present_blobs(repo, conflict)?;
        (
            blobs.iter().any(|blob| blob.is_binary()),
            blobs.iter().any(is_oversize),
        )
    } else {
        (false, false)
    };
    Ok(classify_stages(
        conflict.ancestor.is_some(),
        conflict.our.is_some(),
        conflict.their.is_some(),
        binary,
        oversize,
    ))
}

fn classify_stages(
    has_ancestor: bool,
    has_our: bool,
    has_their: bool,
    binary: bool,
    oversize: bool,
) -> ConflictKind {
    match (has_our, has_their) {
        (true, true) => {
            if binary {
                ConflictKind::Binary
            } else if oversize {
                ConflictKind::Oversize
            } else if has_ancestor {
                ConflictKind::BothModified
            } else {
                ConflictKind::AddedByBoth
            }
        }
        (true, false) => ConflictKind::DeletedByThem,
        (false, _) => ConflictKind::DeletedByUs,
    }
}

fn present_blobs<'r>(
    repo: &'r git2::Repository,
    conflict: &git2::IndexConflict,
) -> Result<Vec<git2::Blob<'r>>, git2::Error> {
    [&conflict.ancestor, &conflict.our, &conflict.their]
        .into_iter()
        .flatten()
        .map(|entry| repo.find_blob(entry.id))
        .collect()
}

fn is_oversize(blob: &git2::Blob) -> bool {
    blob.size() > MAX_DIFF_BYTES
        || blob.content().iter().filter(|&&b| b == b'\n').count() > MAX_DIFF_LINES
}

fn labels_for_state(state: git2::RepositoryState) -> (String, String) {
    use git2::RepositoryState as State;
    match state {
        State::Rebase | State::RebaseInteractive | State::RebaseMerge => (
            "Current · onto".to_string(),
            "Incoming · your commit".to_string(),
        ),
        _ => (
            "Current · ours".to_string(),
            "Incoming · theirs".to_string(),
        ),
    }
}

fn reconstruct_regions(
    repo: &git2::Repository,
    conflict: &git2::IndexConflict,
) -> Result<Vec<Region>, git2::Error> {
    let ancestor = blob_bytes(repo, conflict.ancestor.as_ref())?;
    let ours = blob_bytes(repo, conflict.our.as_ref())?;
    let theirs = blob_bytes(repo, conflict.their.as_ref())?;

    let mut ancestor_in = git2::MergeFileInput::new();
    ancestor_in.content(&ancestor);
    let mut ours_in = git2::MergeFileInput::new();
    ours_in.content(&ours);
    let mut theirs_in = git2::MergeFileInput::new();
    theirs_in.content(&theirs);

    let mut opts = git2::MergeFileOptions::new();
    opts.style_diff3(true);

    let merged = git2::merge_file(&ancestor_in, &ours_in, &theirs_in, Some(&mut opts))?;
    Ok(parse_regions(&String::from_utf8_lossy(merged.content())))
}

fn blob_bytes(
    repo: &git2::Repository,
    entry: Option<&git2::IndexEntry>,
) -> Result<Vec<u8>, git2::Error> {
    match entry {
        Some(entry) => Ok(repo.find_blob(entry.id)?.content().to_vec()),
        None => Ok(Vec::new()),
    }
}

enum Mode {
    Stable,
    Ours,
    Base,
    Theirs,
}

/// Parses a diff3-style `merge_file` buffer (helm-generated, hence safe to parse,
/// conflicts.md §6) into stable and conflict regions. Markers are matched only in
/// the mode where they are expected, so stable content that happens to look like a
/// separator is not misread.
fn parse_regions(text: &str) -> Vec<Region> {
    let mut regions = Vec::new();
    let mut stable = Vec::new();
    let mut ours = Vec::new();
    let mut base = Vec::new();
    let mut theirs = Vec::new();
    let mut mode = Mode::Stable;

    for line in text.lines() {
        match mode {
            Mode::Stable => {
                if line.starts_with("<<<<<<<") {
                    if !stable.is_empty() {
                        regions.push(Region::Stable(std::mem::take(&mut stable)));
                    }
                    mode = Mode::Ours;
                } else {
                    stable.push(line.to_string());
                }
            }
            Mode::Ours => {
                if line.starts_with("|||||||") {
                    mode = Mode::Base;
                } else if line.starts_with("=======") {
                    mode = Mode::Theirs;
                } else {
                    ours.push(line.to_string());
                }
            }
            Mode::Base => {
                if line.starts_with("=======") {
                    mode = Mode::Theirs;
                } else {
                    base.push(line.to_string());
                }
            }
            Mode::Theirs => {
                if line.starts_with(">>>>>>>") {
                    regions.push(Region::Conflict {
                        ours: std::mem::take(&mut ours),
                        theirs: std::mem::take(&mut theirs),
                        base: std::mem::take(&mut base),
                    });
                    mode = Mode::Stable;
                } else {
                    theirs.push(line.to_string());
                }
            }
        }
    }
    if !stable.is_empty() {
        regions.push(Region::Stable(stable));
    }
    regions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_stages_maps_each_present_pattern() {
        assert_eq!(
            classify_stages(true, true, true, false, false),
            ConflictKind::BothModified
        );
        assert_eq!(
            classify_stages(false, true, true, false, false),
            ConflictKind::AddedByBoth
        );
        assert_eq!(
            classify_stages(true, true, false, false, false),
            ConflictKind::DeletedByThem
        );
        assert_eq!(
            classify_stages(true, false, true, false, false),
            ConflictKind::DeletedByUs
        );
    }

    #[test]
    fn classify_stages_gates_binary_before_oversize() {
        assert_eq!(
            classify_stages(true, true, true, true, true),
            ConflictKind::Binary
        );
        assert_eq!(
            classify_stages(true, true, true, false, true),
            ConflictKind::Oversize
        );
    }

    #[test]
    fn labels_invert_under_rebase() {
        assert_eq!(
            labels_for_state(git2::RepositoryState::Merge),
            (
                "Current · ours".to_string(),
                "Incoming · theirs".to_string()
            )
        );
        assert_eq!(
            labels_for_state(git2::RepositoryState::CherryPick),
            (
                "Current · ours".to_string(),
                "Incoming · theirs".to_string()
            )
        );
        assert_eq!(
            labels_for_state(git2::RepositoryState::RebaseMerge),
            (
                "Current · onto".to_string(),
                "Incoming · your commit".to_string()
            )
        );
    }

    #[test]
    fn parse_regions_splits_diff3_into_stable_and_conflict() {
        let buffer = "alpha\n\
<<<<<<< ours\n\
bravo-ours\n\
||||||| base\n\
bravo-base\n\
=======\n\
bravo-theirs\n\
>>>>>>> theirs\n\
charlie\n";
        assert_eq!(
            parse_regions(buffer),
            vec![
                Region::Stable(vec!["alpha".to_string()]),
                Region::Conflict {
                    ours: vec!["bravo-ours".to_string()],
                    theirs: vec!["bravo-theirs".to_string()],
                    base: vec!["bravo-base".to_string()],
                },
                Region::Stable(vec!["charlie".to_string()]),
            ]
        );
    }

    #[test]
    fn parse_regions_handles_an_empty_base_section() {
        let buffer = "<<<<<<< ours\n\
only-ours\n\
||||||| base\n\
=======\n\
only-theirs\n\
>>>>>>> theirs\n";
        assert_eq!(
            parse_regions(buffer),
            vec![Region::Conflict {
                ours: vec!["only-ours".to_string()],
                theirs: vec!["only-theirs".to_string()],
                base: vec![],
            }]
        );
    }
}
