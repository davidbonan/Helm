use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use helm::git::edit::{self, EditError, MAX_EDIT_BYTES};

fn repo_with(name: &str, content: &[u8]) -> (tempfile::TempDir, git2::Repository, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    let full = tmp.path().join(name);
    fs::write(&full, content).unwrap();
    (tmp, repo, full)
}

fn owned(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|l| (*l).to_owned()).collect()
}

#[test]
fn write_range_replaces_the_anchored_lines_and_leaves_no_temporary_behind() {
    let (tmp, repo, full) = repo_with("a.txt", b"one\ntwo\nthree\n");

    edit::write_range(&repo, "a.txt", 1..2, &owned(&["two"]), "TWO\nextra").unwrap();

    assert_eq!(
        fs::read_to_string(&full).unwrap(),
        "one\nTWO\nextra\nthree\n"
    );
    let left: Vec<String> = fs::read_dir(tmp.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name != ".git")
        .collect();
    assert_eq!(left, vec!["a.txt".to_owned()]);
}

#[test]
fn write_range_keeps_crlf_and_a_missing_final_newline() {
    let (_tmp, repo, full) = repo_with("crlf.txt", b"one\r\ntwo\r\nthree");

    edit::write_range(&repo, "crlf.txt", 1..2, &owned(&["two"]), "TWO").unwrap();

    assert_eq!(fs::read(&full).unwrap(), b"one\r\nTWO\r\nthree");
}

#[test]
fn write_range_refuses_when_the_lines_moved_on_disk() {
    let (_tmp, repo, full) = repo_with("a.txt", b"one\ntwo\nthree\n");
    fs::write(&full, "one\nCHANGED\nthree\n").unwrap();

    let err = edit::write_range(&repo, "a.txt", 1..2, &owned(&["two"]), "TWO").unwrap_err();

    assert_eq!(err, EditError::Diverged);
    assert_eq!(fs::read_to_string(&full).unwrap(), "one\nCHANGED\nthree\n");
}

#[test]
fn write_range_refuses_a_range_past_the_end_of_the_file() {
    let (_tmp, repo, _full) = repo_with("a.txt", b"one\ntwo\n");

    let err = edit::write_range(&repo, "a.txt", 5..7, &owned(&["x", "y"]), "z").unwrap_err();

    assert_eq!(err, EditError::Diverged);
}

#[test]
fn write_range_refuses_a_symlink_without_following_it() {
    let (tmp, repo, target) = repo_with("target.txt", b"secret\n");
    let link = tmp.path().join("link.txt");
    std::os::unix::fs::symlink(&target, &link).unwrap();

    let err =
        edit::write_range(&repo, "link.txt", 0..1, &owned(&["secret"]), "SPILLED").unwrap_err();

    assert_eq!(err, EditError::NotRegular);
    assert_eq!(fs::read_to_string(&target).unwrap(), "secret\n");
}

#[test]
fn editable_refuses_a_file_holding_a_nul_byte() {
    let (_tmp, repo, _full) = repo_with("bin.dat", b"one\ntw\0o\n");

    assert_eq!(
        edit::editable(&repo, "bin.dat").unwrap_err(),
        EditError::Binary
    );
}

#[test]
fn editable_refuses_a_file_past_the_size_cap() {
    let (_tmp, repo, full) = repo_with("big.txt", b"");
    // Sparse: the cap is read off the metadata, before the content is ever read.
    fs::File::create(&full)
        .unwrap()
        .set_len(MAX_EDIT_BYTES + 1)
        .unwrap();

    assert_eq!(
        edit::editable(&repo, "big.txt").unwrap_err(),
        EditError::TooLarge
    );
}

#[test]
fn editable_refuses_a_file_without_a_write_bit() {
    let (_tmp, repo, full) = repo_with("ro.txt", b"one\n");
    fs::set_permissions(&full, fs::Permissions::from_mode(0o444)).unwrap();

    assert_eq!(
        edit::editable(&repo, "ro.txt").unwrap_err(),
        EditError::ReadOnly
    );
}

#[test]
fn editable_refuses_a_path_escaping_the_working_tree() {
    // Containment is decided on the path alone, before any disk access: neither
    // target needs to exist for the refusal to be the right one.
    let (tmp, repo, _full) = repo_with("a.txt", b"one\n");
    let outside = tmp.path().parent().unwrap().join("outside.txt");

    assert_eq!(
        edit::editable(&repo, "../outside.txt").unwrap_err(),
        EditError::OutsideWorkdir
    );
    assert_eq!(
        edit::editable(&repo, outside.to_str().unwrap()).unwrap_err(),
        EditError::OutsideWorkdir
    );
}

#[test]
fn write_range_keeps_the_executable_bit() {
    let (_tmp, repo, full) = repo_with("run.sh", b"#!/bin/sh\necho hi\n");
    fs::set_permissions(&full, fs::Permissions::from_mode(0o755)).unwrap();

    edit::write_range(&repo, "run.sh", 1..2, &owned(&["echo hi"]), "echo bye").unwrap();

    assert_eq!(fs::read_to_string(&full).unwrap(), "#!/bin/sh\necho bye\n");
    let mode = fs::metadata(&full).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o755);
}

#[test]
fn editable_accepts_a_plain_text_file() {
    let (_tmp, repo, _full) = repo_with("a.txt", b"one\ntwo\n");

    assert!(edit::editable(&repo, "a.txt").is_ok());
}
