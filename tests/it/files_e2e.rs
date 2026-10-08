use std::fs;
use std::path::Path;

use helm::files::content::{self, Content, FileSnapshot, ReadOutcome, Stamp};
use helm::files::tint::{StatusTints, Tint};
use helm::files::tree::{self, EntryKind, FolderListing, TreeEntry};
use helm::files::{self, FileRow};
use helm::git::edit::{EditError, EditRequest};
use helm::git::status;
use helm::git::worker::{GitCommand, GitResult, GitWorker};
use helm::ui::inline_editor::EditTarget;

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

fn read_on(worker: &GitWorker, path: &str, known: Option<Stamp>) -> ReadOutcome {
    worker.send(GitCommand::ReadFile {
        path: path.to_owned(),
        known,
    });
    match worker.recv() {
        Some((_, GitResult::File(Ok(outcome)))) => outcome,
        other => panic!("expected the file read, got {other:?}"),
    }
}

fn snapshot(outcome: ReadOutcome) -> FileSnapshot {
    match outcome {
        ReadOutcome::Read(snapshot) => snapshot,
        ReadOutcome::Unchanged => panic!("expected a read"),
    }
}

#[test]
fn the_worker_rereads_a_file_only_once_it_changed_and_reports_it_gone() {
    let tmp = tempfile::tempdir().unwrap();
    git2::Repository::init(tmp.path()).unwrap();
    fs::create_dir(tmp.path().join("src")).unwrap();
    fs::write(tmp.path().join("src/main.rs"), "fn main() {}\n").unwrap();
    let worker = GitWorker::spawn(tmp.path(), || {});

    let first = snapshot(read_on(&worker, "src/main.rs", None));
    assert_eq!(
        first.content,
        Content::Text(vec!["fn main() {}".to_owned()])
    );
    assert_eq!(first.size, 13);
    assert_eq!(
        read_on(&worker, "src/main.rs", first.stamp),
        ReadOutcome::Unchanged
    );

    fs::write(tmp.path().join("src/main.rs"), "fn main() {}\n// two\n").unwrap();
    let second = snapshot(read_on(&worker, "src/main.rs", first.stamp));
    assert_eq!(
        second.content,
        Content::Text(vec!["fn main() {}".to_owned(), "// two".to_owned()])
    );

    fs::remove_file(tmp.path().join("src/main.rs")).unwrap();
    let gone = snapshot(read_on(&worker, "src/main.rs", second.stamp));
    assert_eq!(gone.content, Content::Missing);
    assert_eq!(gone.stamp, None);
}

#[test]
fn a_symlink_reads_as_its_target_and_a_file_past_two_megabytes_as_too_large() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("big.log"), vec![b'x'; 3 * 1024 * 1024]).unwrap();
    std::os::unix::fs::symlink("big.log", tmp.path().join("link")).unwrap();

    let link = snapshot(content::read(tmp.path(), "link", None));
    let big = snapshot(content::read(tmp.path(), "big.log", None));

    assert_eq!(link.content, Content::Symlink("big.log".to_owned()));
    assert_eq!(big.content, Content::TooLarge);
    assert_eq!(big.size, 3 * 1024 * 1024);
}

/// The viewer's whole-file write of `buffer`, against the lines `snapshot` read.
fn whole_file_write(snapshot: &FileSnapshot, buffer: &str) -> EditRequest {
    let Content::Text(lines) = &snapshot.content else {
        panic!("expected text, got {:?}", snapshot.content);
    };
    EditTarget::whole_file(&snapshot.path, lines).request(buffer)
}

fn edit_on(worker: &GitWorker, request: EditRequest) -> Result<(), EditError> {
    worker.send(GitCommand::EditFile(request));
    match worker.recv() {
        Some((_, GitResult::Edit { result, .. })) => result.map(|_| ()),
        other => panic!("expected the edit reply, got {other:?}"),
    }
}

#[test]
fn the_viewer_writes_the_whole_file_back_keeping_its_line_endings() {
    let tmp = tempfile::tempdir().unwrap();
    git2::Repository::init(tmp.path()).unwrap();
    fs::write(tmp.path().join("a.txt"), "one\r\ntwo\r\n").unwrap();
    let worker = GitWorker::spawn(tmp.path(), || {});
    let read = snapshot(read_on(&worker, "a.txt", None));
    assert!(read.writable);

    edit_on(&worker, whole_file_write(&read, "one\nTWO\nthree")).unwrap();

    assert_eq!(
        fs::read_to_string(tmp.path().join("a.txt")).unwrap(),
        "one\r\nTWO\r\nthree\r\n"
    );
    let reread = snapshot(read_on(&worker, "a.txt", read.stamp));
    assert_eq!(
        reread.content,
        Content::Text(vec!["one".into(), "TWO".into(), "three".into()]),
        "the re-read is the buffer, line for line"
    );
}

#[test]
fn a_whole_file_write_over_a_moved_file_is_refused_until_overwritten() {
    let tmp = tempfile::tempdir().unwrap();
    git2::Repository::init(tmp.path()).unwrap();
    let file = tmp.path().join("a.txt");
    fs::write(&file, "one\ntwo\n").unwrap();
    let worker = GitWorker::spawn(tmp.path(), || {});
    let read = snapshot(read_on(&worker, "a.txt", None));
    fs::write(&file, "one\ntwo\nadded elsewhere\n").unwrap();
    let write = whole_file_write(&read, "one\nTWO");

    assert_eq!(edit_on(&worker, write.clone()), Err(EditError::Diverged));
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "one\ntwo\nadded elsewhere\n",
        "a line added past the lines the editor opened on is a divergence too"
    );

    edit_on(
        &worker,
        EditRequest {
            force: true,
            ..write
        },
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "one\nTWO\n",
        "Overwrite makes the file what was typed, the other change included"
    );
}

#[test]
fn a_file_without_a_write_bit_reads_as_not_writable() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("locked.txt");
    fs::write(&file, "x\n").unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o444)).unwrap();

    let ReadOutcome::Read(read) = content::read(tmp.path(), "locked.txt", None) else {
        panic!("expected a read");
    };

    assert!(!read.writable);
}
