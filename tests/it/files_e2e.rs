use std::fs;
use std::path::Path;

use helm::files::tint::{StatusTints, Tint};
use helm::files::{self, FileRow};
use helm::git::status;

fn commit_all(repo: &git2::Repository) {
    let mut index = repo.index().unwrap();
    index
        .add_all(["*"], git2::IndexAddOption::DEFAULT, None)
        .unwrap();
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let sig = git2::Signature::now("Test", "test@example.com").unwrap();
    repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
        .unwrap();
}

fn row<'a>(rows: &'a [FileRow], name: &str) -> &'a FileRow {
    rows.iter().find(|row| row.name == name).unwrap()
}

#[test]
fn ignore_rules_flag_ignored_entries_and_everything_inside_an_ignored_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    fs::write(tmp.path().join(".gitignore"), "target/\n*.log\n").unwrap();
    fs::create_dir_all(tmp.path().join("target/debug")).unwrap();
    fs::write(tmp.path().join("target/debug/helm"), "").unwrap();
    fs::write(tmp.path().join("build.log"), "").unwrap();
    fs::write(tmp.path().join("main.rs"), "").unwrap();

    assert!(files::is_ignored(&repo, Path::new("target")));
    assert!(files::is_ignored(&repo, Path::new("target/debug/helm")));
    assert!(files::is_ignored(&repo, Path::new("build.log")));
    assert!(!files::is_ignored(&repo, Path::new("main.rs")));
    assert!(!files::is_ignored(&repo, Path::new(".gitignore")));
}

#[test]
fn a_symlink_is_listed_flagged_and_never_followed() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir(tmp.path().join("real")).unwrap();
    fs::write(tmp.path().join("real/inner.txt"), "").unwrap();
    std::os::unix::fs::symlink("real", tmp.path().join("link")).unwrap();

    let listing = files::list_tree(tmp.path()).unwrap();

    let link = row(&listing.entries, "link");
    assert!(link.symlink);
    assert!(!link.dir, "a link to a folder is not a folder to unfold");
    assert!(!row(&listing.entries, "real").symlink);
    assert_eq!(listing.total, 2);
}

#[test]
fn the_polled_status_tints_untracked_and_modified_files_and_their_folders() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    fs::create_dir(tmp.path().join("src")).unwrap();
    fs::write(tmp.path().join("src/lib.rs"), "one").unwrap();
    commit_all(&repo);
    fs::write(tmp.path().join("src/lib.rs"), "two").unwrap();
    fs::create_dir_all(tmp.path().join("docs/guide")).unwrap();
    fs::write(tmp.path().join("docs/guide/intro.md"), "").unwrap();
    fs::write(tmp.path().join("src/new.rs"), "").unwrap();

    let tints = StatusTints::of(&status::load(tmp.path()).unwrap());

    assert_eq!(tints.file("src/lib.rs"), Some(Tint::Modified));
    assert_eq!(tints.file("docs/guide/intro.md"), Some(Tint::Added));
    assert_eq!(tints.folder("src"), Some(Tint::Modified));
    assert_eq!(tints.folder("docs"), Some(Tint::Added));
    assert_eq!(tints.folder("docs/guide"), Some(Tint::Added));
}
