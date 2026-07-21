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
    /// Line terminator of the file, re-applied when the editor saves (conflicts.md §5).
    pub eol: LineEnding,
    /// Working-tree content when it no longer matches the reconstruction — the file
    /// was hand-edited before the editor opened. Drives the divergence notice
    /// (*Load my version* / *Start from the merge*, conflicts.md §5); `None` when the
    /// file on disk is still the conflict git left there.
    pub disk_divergence: Option<String>,
}

/// How a conflicted file terminates its lines. The 3-way reconstruction splits on
/// `\n` and drops any `\r`, so the shape is detected once from the ours (else
/// theirs) blob and re-applied at compose time — otherwise Save would silently
/// rewrite a CRLF file to LF (conflicts.md §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineEnding {
    /// Lines end with `\r\n`.
    pub crlf: bool,
    /// The file ends with a newline (its last line is terminated).
    pub final_newline: bool,
}

impl Default for LineEnding {
    fn default() -> Self {
        LineEnding {
            crlf: false,
            final_newline: true,
        }
    }
}

impl LineEnding {
    /// Reads the shape off raw file bytes: CRLF as soon as the first `\n` is
    /// preceded by a `\r`. An empty side keeps the default (LF, terminated).
    pub fn detect(content: &[u8]) -> Self {
        if content.is_empty() {
            return LineEnding::default();
        }
        let first_nl = content.iter().position(|&b| b == b'\n');
        LineEnding {
            crlf: first_nl.is_some_and(|i| i > 0 && content[i - 1] == b'\r'),
            final_newline: content.ends_with(b"\n"),
        }
    }

    /// Rewrites an LF-joined buffer with this terminator. The trailing newline is
    /// left exactly as the buffer has it — Save writes the buffer verbatim.
    pub fn apply(&self, text: &str) -> String {
        if self.crlf {
            text.replace("\r\n", "\n").replace('\n', "\r\n")
        } else {
            text.to_owned()
        }
    }
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
    let (regions, eol) = match kind {
        ConflictKind::BothModified | ConflictKind::AddedByBoth => {
            reconstruct_regions(repo, &conflict)?
        }
        _ => (Vec::new(), LineEnding::default()),
    };

    let disk_divergence = if regions.is_empty() {
        None
    } else {
        hand_edited(repo, path, &regions)
    };

    Ok(ConflictFile {
        path: path.to_string(),
        kind,
        ours_label,
        theirs_label,
        regions,
        has_base,
        eol,
        disk_divergence,
    })
}

/// The working-tree file when it diverges from `regions` (conflicts.md §5). Both
/// sides go through `parse_regions`, so neither the line terminator nor the marker
/// style is a divergence: git writes the working tree in `merge` style with branch
/// labels, the reconstruction is diff3 with fixed ones. The content read back is
/// newline-normalised — the editor's Output buffer is LF and `LineEnding::apply`
/// restores the terminator on Save. Unreadable (deleted, binary) ⇒ no notice.
fn hand_edited(repo: &git2::Repository, path: &str, regions: &[Region]) -> Option<String> {
    let bytes = std::fs::read(repo.workdir()?.join(path)).ok()?;
    let text = String::from_utf8(bytes).ok()?;
    if same_sides(regions, &parse_regions(&text)) {
        return None;
    }
    Some(text.replace("\r\n", "\n"))
}

/// Region-by-region equality over the stable and the two conflicting sides, both
/// brought to the same shape by `hoisted` first. The base section is skipped: a
/// `merge`-style working-tree file carries none, which says nothing about a hand
/// edit.
fn same_sides(a: &[Region], b: &[Region]) -> bool {
    hoisted(a) == hoisted(b)
}

/// The regions with the lines common to both sides lifted out of each conflict.
/// git writes the working tree in `merge` style, which trims the shared head and
/// tail of a conflict into the surrounding context, while the diff3 reconstruction
/// keeps them inside the region — without this normalisation, any conflict whose
/// two sides share a boundary line reports a divergence on a file nobody touched.
/// The base is dropped: it is absent from the working-tree file by construction.
fn hoisted(regions: &[Region]) -> Vec<Region> {
    let mut out: Vec<Region> = Vec::new();
    let mut stable: Vec<String> = Vec::new();
    for region in regions {
        match region {
            Region::Stable(lines) => stable.extend(lines.iter().cloned()),
            Region::Conflict { ours, theirs, .. } => {
                let head = common_run(ours.iter(), theirs.iter());
                let tail = common_run(ours[head..].iter().rev(), theirs[head..].iter().rev());
                stable.extend(ours[..head].iter().cloned());
                if !stable.is_empty() {
                    out.push(Region::Stable(std::mem::take(&mut stable)));
                }
                out.push(Region::Conflict {
                    ours: ours[head..ours.len() - tail].to_vec(),
                    theirs: theirs[head..theirs.len() - tail].to_vec(),
                    base: Vec::new(),
                });
                stable.extend(ours[ours.len() - tail..].iter().cloned());
            }
        }
    }
    if !stable.is_empty() {
        out.push(Region::Stable(stable));
    }
    out
}

fn common_run<'a>(
    left: impl Iterator<Item = &'a String>,
    right: impl Iterator<Item = &'a String>,
) -> usize {
    left.zip(right).take_while(|(l, r)| l == r).count()
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
        // A conflicted submodule has `160000` stages whose oids are commits of the
        // submodule's own ODB: reading one back as a blob fails, and collecting the
        // rail into a `Result` would close the editor for every other file of the
        // repo. Submodules are delegated to the terminal (conflicts.md §7), so the
        // gitlink is simply left out of the rail.
        if side_mode(&conflict) == 0o160000 {
            continue;
        }
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
    // The stages are the resolution's own precondition: a path already resolved
    // (another pane, a terminal) or an operation aborted underneath the editor
    // must not be written over from a buffer composed against the old state —
    // the same refusal `resolve_file_side` makes.
    let conflict = find_conflict(&index, path)?;
    match content {
        Some(text) => {
            write_resolution(&full, text.as_bytes(), side_mode(&conflict))
                .map_err(|err| git2::Error::from_str(&err.to_string()))?;
            index.add_path(rel)?;
        }
        None => {
            remove_resolution(&full).map_err(|err| git2::Error::from_str(&err.to_string()))?;
            index.remove_path(rel)?;
        }
    }
    index.write()
}

/// Mode the resolved path must come back as: the ours stage, else theirs, else the
/// ancestor. Only `120000` vs a regular blob matters here — the exec bit of a
/// composed resolution follows the working-tree file, as `git add` would.
fn side_mode(conflict: &git2::IndexConflict) -> u32 {
    conflict
        .our
        .as_ref()
        .or(conflict.their.as_ref())
        .or(conflict.ancestor.as_ref())
        .map_or(0o100644, |entry| entry.mode)
}

/// Writes the resolution at `full`, replacing whatever is there. The path is
/// unlinked first: writing through an existing symlink would land in its target —
/// an arbitrary file, possibly outside the repository (cf. `stage`'s
/// `symlink_metadata`). A `120000` resolution comes back as a link, not as a
/// regular file holding the target's path.
fn write_resolution(full: &Path, content: &[u8], mode: u32) -> std::io::Result<()> {
    remove_resolution(full)?;
    if mode == 0o120000 {
        let target = String::from_utf8_lossy(content);
        std::os::unix::fs::symlink(target.trim_end_matches('\n'), full)
    } else {
        std::fs::write(full, content)
    }
}

/// `remove_file` on the path itself, never on what it points to: `Path::exists`
/// follows the link and would leave a dangling one in the tree.
fn remove_resolution(full: &Path) -> std::io::Result<()> {
    match std::fs::symlink_metadata(full) {
        Ok(_) => std::fs::remove_file(full),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
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
    let mut index = stage::fresh_index(repo)?;
    let conflict = find_conflict(&index, path)?;
    let entry = if ours { conflict.our } else { conflict.their }
        .ok_or_else(|| git2::Error::from_str("the chosen side is absent"))?;
    let content = repo.find_blob(entry.id)?.content().to_vec();

    let rel = Path::new(path);
    let full = workdir.join(rel);
    write_resolution(&full, &content, entry.mode)
        .map_err(|err| git2::Error::from_str(&err.to_string()))?;
    // `git checkout --ours/--theirs` restores that side's mode too, and `add_path`
    // stats the working tree: without the chmod the merge's own mode would be
    // recorded and the side would silently gain or lose its exec bit.
    apply_side_mode(&full, entry.mode).map_err(|err| git2::Error::from_str(&err.to_string()))?;
    index.add_path(rel)?;
    index.write()
}

fn apply_side_mode(full: &Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let bits = match mode {
        0o100755 => 0o755,
        0o100644 => 0o644,
        _ => return Ok(()),
    };
    std::fs::set_permissions(full, std::fs::Permissions::from_mode(bits))
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
) -> Result<(Vec<Region>, LineEnding), git2::Error> {
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
    let eol = LineEnding::detect(if ours.is_empty() { &theirs } else { &ours });
    Ok((
        parse_regions(&String::from_utf8_lossy(merged.content())),
        eol,
    ))
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
    fn line_ending_detects_the_terminator_and_the_final_newline() {
        assert_eq!(
            LineEnding::detect(b"alpha\r\nbravo\r\n"),
            LineEnding {
                crlf: true,
                final_newline: true
            }
        );
        assert_eq!(
            LineEnding::detect(b"alpha\r\nbravo"),
            LineEnding {
                crlf: true,
                final_newline: false
            }
        );
        assert_eq!(
            LineEnding::detect(b"alpha\nbravo\n"),
            LineEnding {
                crlf: false,
                final_newline: true
            }
        );
        assert_eq!(
            LineEnding::detect(b"one line, no terminator"),
            LineEnding {
                crlf: false,
                final_newline: false
            }
        );
        assert_eq!(LineEnding::detect(b""), LineEnding::default());
    }

    #[test]
    fn a_crlf_buffer_round_trips_through_parse_and_apply() {
        let buffer = "alpha\r\n\
<<<<<<< ours\r\n\
bravo-ours\r\n\
||||||| base\r\n\
bravo\r\n\
=======\r\n\
bravo-theirs\r\n\
>>>>>>> theirs\r\n\
charlie\r\n";
        let eol = LineEnding::detect(buffer.as_bytes());
        assert_eq!(
            parse_regions(buffer),
            vec![
                Region::Stable(vec!["alpha".to_string()]),
                Region::Conflict {
                    ours: vec!["bravo-ours".to_string()],
                    theirs: vec!["bravo-theirs".to_string()],
                    base: vec!["bravo".to_string()],
                },
                Region::Stable(vec!["charlie".to_string()]),
            ]
        );
        // The editor composes LF; Save puts the terminator back.
        assert_eq!(
            eol.apply("alpha\nbravo-ours\ncharlie\n"),
            "alpha\r\nbravo-ours\r\ncharlie\r\n"
        );
        // Idempotent on a buffer that already carries CRLF (a hand-loaded file).
        assert_eq!(
            eol.apply("alpha\r\nbravo-ours\r\n"),
            "alpha\r\nbravo-ours\r\n"
        );
        // An LF file is written verbatim, trailing newline included or not.
        let lf = LineEnding::detect(b"alpha\nbravo");
        assert_eq!(lf.apply("alpha\nbravo"), "alpha\nbravo");
    }

    #[test]
    fn same_sides_ignores_the_base_and_the_marker_labels() {
        let diff3 = parse_regions(
            "stable\n\
<<<<<<< ours\n\
mine\n\
||||||| base\n\
old\n\
=======\n\
yours\n\
>>>>>>> theirs\n",
        );
        // What git leaves in the working tree: `merge` style, branch labels, CRLF.
        let on_disk = parse_regions(
            "stable\r\n\
<<<<<<< HEAD\r\n\
mine\r\n\
=======\r\n\
yours\r\n\
>>>>>>> feature\r\n",
        );
        assert!(same_sides(&diff3, &on_disk));

        let edited = parse_regions("stable\nmine\n");
        assert!(!same_sides(&diff3, &edited));
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
