use std::fs;
use std::path::Path;

use helm::git::diff::{self, DiffSource, LineOrigin};

fn commit_file(repo: &git2::Repository, name: &str, content: &str, message: &str) -> git2::Oid {
    let dir = repo.workdir().unwrap();
    fs::write(dir.join(name), content).unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new(name)).unwrap();
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let sig = git2::Signature::now("Test", "test@example.com").unwrap();
    let parents: Vec<git2::Commit> = repo
        .head()
        .ok()
        .and_then(|h| h.target())
        .and_then(|oid| repo.find_commit(oid).ok())
        .into_iter()
        .collect();
    let parent_refs: Vec<&git2::Commit> = parents.iter().collect();
    repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &parent_refs)
        .unwrap()
}

fn commit_bytes(repo: &git2::Repository, name: &str, bytes: &[u8], message: &str) -> git2::Oid {
    let dir = repo.workdir().unwrap();
    fs::write(dir.join(name), bytes).unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new(name)).unwrap();
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let sig = git2::Signature::now("Test", "test@example.com").unwrap();
    let parents: Vec<git2::Commit> = repo
        .head()
        .ok()
        .and_then(|h| h.target())
        .and_then(|oid| repo.find_commit(oid).ok())
        .into_iter()
        .collect();
    let parent_refs: Vec<&git2::Commit> = parents.iter().collect();
    repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &parent_refs)
        .unwrap()
}

/// A real 3×3 PNG so the diff layer's image detection runs against decodable bytes.
fn png_bytes(color: [u8; 4]) -> Vec<u8> {
    let img = image::RgbaImage::from_pixel(3, 3, image::Rgba(color));
    let mut bytes = Vec::new();
    image::DynamicImage::ImageRgba8(img)
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .unwrap();
    bytes
}

fn contents(d: &diff::FileDiff, origin: LineOrigin) -> Vec<String> {
    d.hunks
        .iter()
        .flat_map(|h| h.lines.iter())
        .filter(|l| l.origin == origin)
        .map(|l| l.content.trim_end().to_string())
        .collect()
}

#[test]
fn unstaged_diff_shows_worktree_change_against_index() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "a.txt", "line1\nline2\nline3\n", "init");
    fs::write(tmp.path().join("a.txt"), "line1\nCHANGED\nline3\n").unwrap();

    let d = diff::file_diff(&repo, "a.txt", DiffSource::Unstaged).unwrap();

    assert!(!d.binary);
    assert_eq!(d.hunks.len(), 1);
    assert_eq!(contents(&d, LineOrigin::Deletion), vec!["line2"]);
    assert_eq!(contents(&d, LineOrigin::Addition), vec!["CHANGED"]);
}

#[test]
fn staged_diff_shows_index_change_against_head() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "a.txt", "line1\nline2\nline3\n", "init");
    fs::write(tmp.path().join("a.txt"), "line1\nSTAGED\nline3\n").unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new("a.txt")).unwrap();
    index.write().unwrap();

    let d = diff::file_diff(&repo, "a.txt", DiffSource::Staged).unwrap();

    assert!(!d.binary);
    assert_eq!(d.hunks.len(), 1);
    assert_eq!(contents(&d, LineOrigin::Deletion), vec!["line2"]);
    assert_eq!(contents(&d, LineOrigin::Addition), vec!["STAGED"]);
}

#[test]
fn diff_direction_is_independent_per_section() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "a.txt", "base\n", "init");

    fs::write(tmp.path().join("a.txt"), "staged\n").unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new("a.txt")).unwrap();
    index.write().unwrap();

    fs::write(tmp.path().join("a.txt"), "worktree\n").unwrap();

    let staged = diff::file_diff(&repo, "a.txt", DiffSource::Staged).unwrap();
    assert_eq!(contents(&staged, LineOrigin::Deletion), vec!["base"]);
    assert_eq!(contents(&staged, LineOrigin::Addition), vec!["staged"]);

    let unstaged = diff::file_diff(&repo, "a.txt", DiffSource::Unstaged).unwrap();
    assert_eq!(contents(&unstaged, LineOrigin::Deletion), vec!["staged"]);
    assert_eq!(contents(&unstaged, LineOrigin::Addition), vec!["worktree"]);
}

#[test]
fn untracked_file_diffs_as_all_additions_in_unstaged() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    fs::write(tmp.path().join("new.txt"), "alpha\nbeta\n").unwrap();

    let d = diff::file_diff(&repo, "new.txt", DiffSource::Unstaged).unwrap();

    assert!(!d.binary);
    assert_eq!(contents(&d, LineOrigin::Addition), vec!["alpha", "beta"]);
    assert!(contents(&d, LineOrigin::Deletion).is_empty());
}

#[test]
fn staged_new_file_diffs_against_empty_head_tree_when_unborn() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    fs::write(tmp.path().join("a.txt"), "hello\n").unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new("a.txt")).unwrap();
    index.write().unwrap();

    let d = diff::file_diff(&repo, "a.txt", DiffSource::Staged).unwrap();

    assert!(!d.binary);
    assert_eq!(contents(&d, LineOrigin::Addition), vec!["hello"]);
    assert!(contents(&d, LineOrigin::Deletion).is_empty());
}

#[test]
fn pathspec_limits_diff_to_the_requested_file() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "a.txt", "a-base\n", "init a");
    commit_file(&repo, "b.txt", "b-base\n", "init b");
    fs::write(tmp.path().join("a.txt"), "a-new\n").unwrap();
    fs::write(tmp.path().join("b.txt"), "b-new\n").unwrap();

    let d = diff::file_diff(&repo, "a.txt", DiffSource::Unstaged).unwrap();

    assert_eq!(d.path, "a.txt");
    assert_eq!(contents(&d, LineOrigin::Addition), vec!["a-new"]);
    assert!(d
        .hunks
        .iter()
        .flat_map(|h| h.lines.iter())
        .all(|l| !l.content.contains("b-")));
}

#[test]
fn unchanged_file_yields_no_hunks() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "a.txt", "stable\n", "init");

    let d = diff::file_diff(&repo, "a.txt", DiffSource::Unstaged).unwrap();

    assert!(!d.binary);
    assert!(d.hunks.is_empty());
}

#[test]
fn editable_is_carried_by_the_diff_so_a_click_needs_no_disk_access() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "a.txt", "one\ntwo\n", "init");
    fs::write(tmp.path().join("a.txt"), "one\nTWO\n").unwrap();
    commit_bytes(&repo, "blob.bin", b"\x00one\x00", "bin");
    fs::write(tmp.path().join("blob.bin"), b"\x00two\x00").unwrap();

    assert!(
        diff::file_diff(&repo, "a.txt", DiffSource::Unstaged)
            .unwrap()
            .editable
    );
    // A binary diff shows no lines to click, so it never opens an editor.
    assert!(
        !diff::file_diff(&repo, "blob.bin", DiffSource::Unstaged)
            .unwrap()
            .editable
    );
}

#[test]
fn binary_file_is_flagged_with_no_hunks_for_file_level_staging() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    // A NUL byte makes libgit2 classify the file as binary.
    commit_file(&repo, "blob.bin", "\u{0}stable\u{0}data\n", "init");
    fs::write(tmp.path().join("blob.bin"), b"\x00CHANGED\x00bytes\n").unwrap();

    let d = diff::file_diff(&repo, "blob.bin", DiffSource::Unstaged).unwrap();

    assert!(d.binary, "a NUL-bearing file is detected as binary");
    assert!(!d.oversize);
    assert!(
        d.hunks.is_empty(),
        "a binary diff carries no hunks ⇒ no line/hunk staging, file-level only"
    );
}

#[test]
fn untracked_binary_file_is_flagged_with_no_hunks() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    fs::write(tmp.path().join("new.bin"), b"\x00\x01\x02logo\x00").unwrap();

    let d = diff::file_diff(&repo, "new.bin", DiffSource::Unstaged).unwrap();

    assert!(d.binary);
    assert!(d.hunks.is_empty());
    assert!(
        d.image.is_none(),
        "a non-image binary carries no preview blob"
    );
}

#[test]
fn untracked_image_file_carries_a_preview_blob() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    let png = png_bytes([200, 40, 40, 255]);
    fs::write(tmp.path().join("logo.png"), &png).unwrap();

    let d = diff::file_diff(&repo, "logo.png", DiffSource::Unstaged).unwrap();

    assert!(d.binary);
    let image = d
        .image
        .expect("an image file carries its bytes for the preview");
    assert_eq!(
        image.bytes, png,
        "the new-side bytes are preserved verbatim"
    );
    assert_ne!(image.fingerprint, 0);
}

#[test]
fn modified_tracked_image_carries_the_worktree_bytes() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_bytes(&repo, "logo.png", &png_bytes([10, 10, 10, 255]), "init");
    let edited = png_bytes([240, 240, 0, 255]);
    fs::write(tmp.path().join("logo.png"), &edited).unwrap();

    let d = diff::file_diff(&repo, "logo.png", DiffSource::Unstaged).unwrap();

    assert!(d.binary);
    assert_eq!(
        d.image
            .expect("modified image carries a preview blob")
            .bytes,
        edited,
        "the preview shows the working-tree side, not the committed one"
    );
}

#[test]
fn committed_image_carries_a_preview_blob() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    let png = png_bytes([0, 120, 200, 255]);
    let oid = commit_bytes(&repo, "logo.png", &png, "add logo");

    let d = diff::commit_file_diff(&repo, oid, "logo.png").unwrap();

    assert!(d.binary);
    assert_eq!(
        d.image
            .expect("committed image carries a preview blob")
            .bytes,
        png
    );
}

#[test]
fn oversize_diff_is_flagged_with_no_hunks() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "big.txt", "seed\n", "init");
    // > 50,000 added lines ⇒ beyond the inline display threshold (git.md §8).
    let huge: String = (0..60_000).map(|i| format!("line {i}\n")).collect();
    fs::write(tmp.path().join("big.txt"), huge).unwrap();

    let d = diff::file_diff(&repo, "big.txt", DiffSource::Unstaged).unwrap();

    assert!(!d.binary);
    assert!(d.oversize, "a >50k-line diff is flagged oversize");
    assert!(
        d.hunks.is_empty(),
        "an oversize diff carries no hunks ⇒ file-level staging only"
    );
}

#[test]
fn source_lines_carry_the_new_side_for_each_diff_source() {
    // Material for context extension (git.md §4): the full new side — worktree
    // for Unstaged, index blob for Staged, commit blob for a commit diff.
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "a.txt", "line1\nline2\nline3\n", "init");
    let second = commit_file(&repo, "a.txt", "line1\nCOMMITTED\nline3\n", "change");
    fs::write(tmp.path().join("a.txt"), "line1\nSTAGED\nline3\n").unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new("a.txt")).unwrap();
    index.write().unwrap();
    fs::write(tmp.path().join("a.txt"), "line1\nWORKTREE\nline3\n").unwrap();

    let unstaged = diff::file_diff(&repo, "a.txt", DiffSource::Unstaged).unwrap();
    assert_eq!(unstaged.source_lines, vec!["line1", "WORKTREE", "line3"]);

    let staged = diff::file_diff(&repo, "a.txt", DiffSource::Staged).unwrap();
    assert_eq!(staged.source_lines, vec!["line1", "STAGED", "line3"]);

    let committed = diff::commit_file_diff(&repo, second, "a.txt").unwrap();
    assert_eq!(committed.source_lines, vec!["line1", "COMMITTED", "line3"]);
}

#[test]
fn source_lines_cover_untracked_files_and_stay_empty_without_hunks() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    fs::write(tmp.path().join("new.txt"), "alpha\nbeta\n").unwrap();

    let untracked = diff::file_diff(&repo, "new.txt", DiffSource::Unstaged).unwrap();
    assert_eq!(untracked.source_lines, vec!["alpha", "beta"]);

    commit_file(&repo, "stable.txt", "stable\n", "init");
    let unchanged = diff::file_diff(&repo, "stable.txt", DiffSource::Unstaged).unwrap();
    assert!(
        unchanged.source_lines.is_empty(),
        "no hunk ⇒ no embedded content"
    );
}

#[test]
fn source_lines_stay_empty_for_binary_and_deleted_files() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "blob.bin", "\u{0}stable\u{0}data\n", "init");
    fs::write(tmp.path().join("blob.bin"), b"\x00CHANGED\x00bytes\n").unwrap();
    let binary = diff::file_diff(&repo, "blob.bin", DiffSource::Unstaged).unwrap();
    assert!(binary.source_lines.is_empty());

    commit_file(&repo, "gone.txt", "doomed\n", "add doomed");
    fs::remove_file(tmp.path().join("gone.txt")).unwrap();
    let deleted = diff::file_diff(&repo, "gone.txt", DiffSource::Unstaged).unwrap();
    assert!(
        !deleted.hunks.is_empty(),
        "the deletion does produce a diff"
    );
    assert!(
        deleted.source_lines.is_empty(),
        "new side absent (deleted file) ⇒ nothing to extend"
    );
}

#[test]
fn commit_file_diff_shows_change_against_first_parent() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "a.txt", "line1\nline2\nline3\n", "init");
    let second = commit_file(&repo, "a.txt", "line1\nCHANGED\nline3\n", "change");

    let d = diff::commit_file_diff(&repo, second, "a.txt").unwrap();

    assert!(!d.binary);
    assert_eq!(d.path, "a.txt");
    assert_eq!(d.hunks.len(), 1);
    assert_eq!(contents(&d, LineOrigin::Deletion), vec!["line2"]);
    assert_eq!(contents(&d, LineOrigin::Addition), vec!["CHANGED"]);
}

#[test]
fn commit_file_diff_for_root_commit_is_all_additions() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    let root = commit_file(&repo, "a.txt", "alpha\nbeta\n", "root");

    let d = diff::commit_file_diff(&repo, root, "a.txt").unwrap();

    assert!(!d.binary);
    assert_eq!(contents(&d, LineOrigin::Addition), vec!["alpha", "beta"]);
    assert!(contents(&d, LineOrigin::Deletion).is_empty());
}

#[test]
fn commit_file_diff_flags_binary_change_with_no_hunks() {
    // Edge case M9-8: a binary file modified in a commit reuses the M6-4
    // handling (`binary` flag, no hunk) ⇒ the full-screen diff (M9-7) renders
    // **Binary file** instead of raw bytes (git.md §8–9).
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "blob.bin", "\u{0}stable\u{0}data\n", "init binary");
    let second = commit_file(&repo, "blob.bin", "\u{0}CHANGED\u{0}bytes\n", "edit binary");

    let d = diff::commit_file_diff(&repo, second, "blob.bin").unwrap();

    assert!(
        d.binary,
        "a NUL-bearing commit change is detected as binary"
    );
    assert!(!d.oversize);
    assert!(
        d.hunks.is_empty(),
        "a binary commit diff carries no hunks ⇒ Binary file, no line view"
    );
}

#[test]
fn unstaged_rename_diffs_as_a_rename_instead_of_full_additions() {
    // The sidebar pairs the move as a rename: the diff must show the edit that
    // rides along with it, not the whole file as additions (git.md §8). The
    // pathspec alone keeps only the new (untracked) side, so `find_similar` has
    // nothing to pair — the old path has to be diffed in too.
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "old.txt", "line1\nline2\nline3\nline4\n", "init");

    fs::remove_file(tmp.path().join("old.txt")).unwrap();
    fs::write(tmp.path().join("new.txt"), "line1\nCHANGED\nline3\nline4\n").unwrap();

    let d = diff::file_diff(&repo, "new.txt", DiffSource::Unstaged).unwrap();

    assert_eq!(d.path, "new.txt");
    assert_eq!(contents(&d, LineOrigin::Deletion), vec!["line2"]);
    assert_eq!(contents(&d, LineOrigin::Addition), vec!["CHANGED"]);
}

#[test]
fn staged_rename_diffs_as_a_rename_instead_of_full_additions() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "old.txt", "line1\nline2\nline3\nline4\n", "init");

    fs::remove_file(tmp.path().join("old.txt")).unwrap();
    fs::write(tmp.path().join("new.txt"), "line1\nCHANGED\nline3\nline4\n").unwrap();
    let mut index = repo.index().unwrap();
    index.remove_path(Path::new("old.txt")).unwrap();
    index.add_path(Path::new("new.txt")).unwrap();
    index.write().unwrap();

    let d = diff::file_diff(&repo, "new.txt", DiffSource::Staged).unwrap();

    assert_eq!(d.path, "new.txt");
    assert_eq!(contents(&d, LineOrigin::Deletion), vec!["line2"]);
    assert_eq!(contents(&d, LineOrigin::Addition), vec!["CHANGED"]);
}

#[test]
fn commit_file_diff_follows_renames_instead_of_showing_full_additions() {
    // The commit detail reports the file as `Renamed` (find_similar): the
    // full-screen diff must show the rename's real edits, not the whole file as
    // additions (git.md §9).
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "old.txt", "line1\nline2\nline3\n", "init");

    fs::remove_file(tmp.path().join("old.txt")).unwrap();
    fs::write(tmp.path().join("new.txt"), "line1\nCHANGED\nline3\n").unwrap();
    let mut index = repo.index().unwrap();
    index.remove_path(Path::new("old.txt")).unwrap();
    index.add_path(Path::new("new.txt")).unwrap();
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let sig = git2::Signature::now("Test", "test@example.com").unwrap();
    let head = repo.head().unwrap().target().unwrap();
    let parent = repo.find_commit(head).unwrap();
    let rename = repo
        .commit(Some("HEAD"), &sig, &sig, "rename", &tree, &[&parent])
        .unwrap();

    let d = diff::commit_file_diff(&repo, rename, "new.txt").unwrap();

    assert_eq!(d.path, "new.txt");
    assert_eq!(contents(&d, LineOrigin::Deletion), vec!["line2"]);
    assert_eq!(contents(&d, LineOrigin::Addition), vec!["CHANGED"]);
}

#[test]
fn commit_file_diff_limits_to_the_requested_path() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "a.txt", "a-base\n", "init a");
    fs::write(tmp.path().join("a.txt"), "a-next\n").unwrap();
    fs::write(tmp.path().join("b.txt"), "b-new\n").unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new("a.txt")).unwrap();
    index.add_path(Path::new("b.txt")).unwrap();
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let sig = git2::Signature::now("Test", "test@example.com").unwrap();
    let head = repo.head().unwrap().target().unwrap();
    let parent = repo.find_commit(head).unwrap();
    let second = repo
        .commit(Some("HEAD"), &sig, &sig, "both", &tree, &[&parent])
        .unwrap();

    let d = diff::commit_file_diff(&repo, second, "a.txt").unwrap();

    assert_eq!(d.path, "a.txt");
    assert_eq!(contents(&d, LineOrigin::Addition), vec!["a-next"]);
    assert!(d
        .hunks
        .iter()
        .flat_map(|h| h.lines.iter())
        .all(|l| !l.content.contains("b-")));
}

/// A stashed **untracked** file lives in the stash's 3rd parent, not in the
/// stash tree: the fullscreen diff must fall back on that tree (all additions)
/// instead of coming back empty (D-2026-06-05).
#[test]
fn commit_file_diff_reads_a_stashed_untracked_file_from_the_third_parent() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    let mut cfg = repo.config().unwrap();
    cfg.set_str("user.name", "Test").unwrap();
    cfg.set_str("user.email", "test@example.com").unwrap();
    commit_file(&repo, "a.txt", "base\n", "init");
    fs::write(tmp.path().join("new.txt"), "one\ntwo\n").unwrap();
    helm::git::stash::stash(&repo).unwrap();
    let oid = repo.reflog("refs/stash").unwrap().get(0).unwrap().id_new();

    let d = diff::commit_file_diff(&repo, oid, "new.txt").unwrap();

    assert_eq!(contents(&d, LineOrigin::Addition), vec!["one", "two"]);
    assert_eq!(d.source_lines, vec!["one", "two"]);
}
