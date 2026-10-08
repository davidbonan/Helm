use std::fs;
use std::path::Path;

use helm::files::tint::{StatusTints, Tint};
use helm::files::tree::{self, EntryKind, FolderListing, TreeEntry};
use helm::files::{self, FileRow};
use helm::git::status;
use helm::git::worker::{GitCommand, GitResult, GitWorker};

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

fn entry<'a>(listing: &'a FolderListing, name: &str) -> &'a TreeEntry {
    listing
        .entries
        .iter()
        .find(|entry| entry.name == name)
        .unwrap()
}

#[test]
fn a_tree_folder_lists_its_entries_flagged_ignored_and_everything_inside_an_ignored_one() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    fs::write(tmp.path().join(".gitignore"), "target/\n*.log\n").unwrap();
    fs::create_dir_all(tmp.path().join("target/debug")).unwrap();
    fs::write(tmp.path().join("target/notes.md"), "").unwrap();
    fs::write(tmp.path().join("build.log"), "").unwrap();
    fs::write(tmp.path().join("main.rs"), "").unwrap();
    std::os::unix::fs::symlink("main.rs", tmp.path().join("link")).unwrap();

    let listed = tree::list_folders(&repo, &[String::new(), "target".to_owned()]);

    let (root, target) = (&listed[0].1, &listed[1].1);
    assert!(entry(root, "target").ignored);
    assert!(entry(root, "build.log").ignored);
    assert!(!entry(root, "main.rs").ignored);
    assert_eq!(entry(root, "link").kind, EntryKind::Symlink);
    assert_eq!(entry(root, "target").kind, EntryKind::Folder);
    assert!(target.entries.iter().all(|entry| entry.ignored));
    assert_eq!(target.total, 2);
}

#[test]
fn a_folder_gone_from_disk_lists_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();

    let listed = tree::list_folders(&repo, &["gone".to_owned()]);

    assert_eq!(listed, [("gone".to_owned(), FolderListing::default())]);
}

fn listed_names(worker: &GitWorker) -> Vec<String> {
    worker.send(GitCommand::ListFolders(vec![String::new()]));
    match worker.recv() {
        Some((_, GitResult::Folders(Ok(listed)))) => listed[0]
            .1
            .entries
            .iter()
            .map(|entry| entry.name.clone())
            .collect(),
        other => panic!("expected the root listed, got {other:?}"),
    }
}

#[test]
fn a_relisting_on_the_worker_picks_up_a_new_file() {
    let tmp = tempfile::tempdir().unwrap();
    git2::Repository::init(tmp.path()).unwrap();
    fs::write(tmp.path().join("a.txt"), "").unwrap();
    let worker = GitWorker::spawn(tmp.path(), || {});
    assert_eq!(listed_names(&worker), ["a.txt"]);

    fs::write(tmp.path().join("b.txt"), "").unwrap();

    assert_eq!(listed_names(&worker), ["a.txt", "b.txt"]);
}
