//! Business e2e for `links::execute_with` (specs/terminal.md §12): real
//! subprocesses stand in for macOS `open` and for the configured editor, each
//! capturing its argv to a file — no browser, no editor launched. The editor is
//! spawned detached (reaper thread), so its capture is polled.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use helm::terminal::links::{execute_with, open_url_with, LinkAction, LinkError};

/// Writes an executable shell script that dumps each arg on its own line to
/// `capture`, then exits with `code`.
fn fake_tool(dir: &Path, name: &str, capture: &Path, code: i32) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(
        &path,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nexit {code}\n",
            capture.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

fn captured(capture: &Path) -> Vec<String> {
    std::fs::read_to_string(capture)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}

/// The detached editor writes on launch; poll its capture instead of racing it.
fn wait_for_capture(capture: &Path) -> Vec<String> {
    for _ in 0..200 {
        if let Ok(text) = std::fs::read_to_string(capture) {
            if !text.trim().is_empty() {
                return text.lines().map(str::to_owned).collect();
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("the fake editor never captured its argv");
}

#[test]
fn a_url_is_handed_to_open() {
    let tmp = tempfile::tempdir().unwrap();
    let capture = tmp.path().join("argv.txt");
    let open = fake_tool(tmp.path(), "open", &capture, 0);

    execute_with(
        &open,
        &LinkAction::Url("https://example.com/path".to_owned()),
        "code -g {file}:{line}",
    )
    .unwrap();

    assert_eq!(captured(&capture), ["https://example.com/path"]);
}

#[test]
fn open_url_with_hands_the_exact_url_to_open() {
    // The Create pull request seam (git.md §9): the prefilled forge URL reaches
    // `open` verbatim — query string and all — no browser launched here.
    let tmp = tempfile::tempdir().unwrap();
    let capture = tmp.path().join("argv.txt");
    let open = fake_tool(tmp.path(), "open", &capture, 0);
    let url = "https://bitbucket.org/team/repo/pull-requests/new?source=feat&dest=main";

    open_url_with(&open, url).unwrap();

    assert_eq!(captured(&capture), [url]);
}

#[test]
fn an_empty_template_opens_the_file_with_open() {
    let tmp = tempfile::tempdir().unwrap();
    let capture = tmp.path().join("argv.txt");
    let open = fake_tool(tmp.path(), "open", &capture, 0);
    let file = tmp.path().join("notes.md");

    execute_with(
        &open,
        &LinkAction::File {
            path: file.clone(),
            line: Some(5),
            column: None,
        },
        "",
    )
    .unwrap();

    assert_eq!(captured(&capture), [file.to_string_lossy()]);
}

#[test]
fn a_configured_editor_receives_the_substituted_file_and_line() {
    let tmp = tempfile::tempdir().unwrap();
    let capture = tmp.path().join("argv.txt");
    let editor = fake_tool(tmp.path(), "edit", &capture, 0);
    let file = tmp.path().join("src.rs");
    let template = format!("{} -g {{file}}:{{line}}", editor.display());

    execute_with(
        Path::new("open"),
        &LinkAction::File {
            path: file.clone(),
            line: Some(7),
            column: None,
        },
        &template,
    )
    .unwrap();

    assert_eq!(
        wait_for_capture(&capture),
        ["-g".to_owned(), format!("{}:7", file.display())]
    );
}

#[test]
fn a_missing_editor_binary_surfaces_a_not_found_error() {
    let tmp = tempfile::tempdir().unwrap();
    let missing = tmp.path().join("no-such-editor");
    let template = format!("{} {{file}}", missing.display());

    let result = execute_with(
        Path::new("open"),
        &LinkAction::File {
            path: tmp.path().join("x.rs"),
            line: None,
            column: None,
        },
        &template,
    );

    assert_eq!(
        result,
        Err(LinkError::NotFound(missing.to_string_lossy().into_owned()))
    );
}
