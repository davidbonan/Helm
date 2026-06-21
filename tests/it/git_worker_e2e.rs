use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use helm::git::cli;
use helm::git::sync::{PullMode, SyncError, SyncOutcome};
use helm::git::worker::{
    drain_sync_refresh, GitCommand, GitResult, GitWorker, MutationLock, ResultKind, SyncCommand,
    SyncRunner,
};

#[test]
fn status_command_returns_repo_status_over_channel() {
    let tmp = tempfile::tempdir().unwrap();
    git2::Repository::init(tmp.path()).unwrap();
    fs::write(tmp.path().join("a.txt"), "hello").unwrap();

    let worker = GitWorker::spawn(tmp.path(), || {});
    worker.send(GitCommand::Status);

    match worker.recv() {
        Some((
            _,
            GitResult::Status {
                result: Ok(snap), ..
            },
        )) => {
            assert!(snap.status.unstaged.iter().any(|f| f.path == "a.txt"));
            assert!(snap.status.staged.is_empty());
        }
        other => panic!("expected Ok status, got {other:?}"),
    }
}

#[test]
fn worker_reuses_owned_repository_across_commands() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    fs::write(tmp.path().join("a.txt"), "hello").unwrap();

    let worker = GitWorker::spawn(tmp.path(), || {});

    worker.send(GitCommand::Status);
    match worker.recv() {
        Some((
            _,
            GitResult::Status {
                result: Ok(snap), ..
            },
        )) => {
            assert!(snap.status.unstaged.iter().any(|f| f.path == "a.txt"));
            assert!(snap.status.staged.is_empty());
        }
        other => panic!("expected first Ok status, got {other:?}"),
    }

    let mut index = repo.index().unwrap();
    index.add_path(Path::new("a.txt")).unwrap();
    index.write().unwrap();

    worker.send(GitCommand::Status);
    match worker.recv() {
        Some((
            _,
            GitResult::Status {
                result: Ok(snap), ..
            },
        )) => {
            assert!(snap.status.staged.iter().any(|f| f.path == "a.txt"));
        }
        other => panic!("expected second Ok status, got {other:?}"),
    }
}

#[test]
fn each_result_wakes_the_ui_via_the_callback() {
    let tmp = tempfile::tempdir().unwrap();
    git2::Repository::init(tmp.path()).unwrap();

    let wakeups = Arc::new(AtomicUsize::new(0));
    let counter = wakeups.clone();
    let worker = GitWorker::spawn(tmp.path(), move || {
        counter.fetch_add(1, Ordering::SeqCst);
    });

    worker.send(GitCommand::Status);
    worker.recv().expect("a result");
    // The wakeup follows sending the result: wait until it is observed.
    for _ in 0..200 {
        if wakeups.load(Ordering::SeqCst) >= 1 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(
        wakeups.load(Ordering::SeqCst),
        1,
        "the worker wakes the UI once per result — otherwise the git panel stays stale"
    );
}

#[test]
fn non_repo_path_yields_error_over_channel() {
    let tmp = tempfile::tempdir().unwrap();

    let worker = GitWorker::spawn(tmp.path(), || {});
    worker.send(GitCommand::Status);

    match worker.recv() {
        Some((_, GitResult::Status { result: Err(_), .. })) => {}
        other => panic!("expected Err status, got {other:?}"),
    }
}

#[test]
fn status_results_carry_their_source_command() {
    let tmp = tempfile::tempdir().unwrap();
    git2::Repository::init(tmp.path()).unwrap();

    let worker = GitWorker::spawn(tmp.path(), || {});
    // Clean tree ⇒ the stash fails; the app routes the error by `source`
    // ("Stash failed" toast, not an inline error of the Branch popover).
    worker.send(GitCommand::Stash);

    match worker.recv() {
        Some((
            _,
            GitResult::Status {
                source: GitCommand::Stash,
                result: Err(err),
            },
        )) => assert_eq!(err.message(), "nothing to stash"),
        other => panic!("expected tagged Err status, got {other:?}"),
    }
}

#[test]
fn drop_applies_queued_mutations_and_skips_queued_reads() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    fs::write(tmp.path().join("a.txt"), "hello").unwrap();

    let worker = GitWorker::spawn(tmp.path(), || {});
    // Typical queue on repo switch: poll reads ahead of a mutation. The drop
    // does not wait for the read replies (skipped once the session is
    // cancelled), but the requested Stage must complete (write safety).
    worker.send(GitCommand::Graph { limit: 0 });
    worker.send(GitCommand::Status);
    worker.send(GitCommand::Stage("a.txt".into()));
    drop(worker);

    let status = repo.status_file(Path::new("a.txt")).unwrap();
    assert!(
        status.contains(git2::Status::INDEX_NEW),
        "the queued mutation is applied despite the session being abandoned"
    );
}

/// Commit `content` of `name` on top of HEAD, returning the new commit oid.
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

#[test]
fn status_snapshot_tracks_remote_presence() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();

    let worker = GitWorker::spawn(tmp.path(), || {});
    worker.send(GitCommand::Status);
    match worker.recv() {
        Some((
            _,
            GitResult::Status {
                result: Ok(snap), ..
            },
        )) => {
            assert!(!snap.has_remote, "no remote configured");
        }
        other => panic!("expected Ok status, got {other:?}"),
    }

    repo.remote("origin", "file:///nowhere").unwrap();
    worker.send(GitCommand::Status);
    match worker.recv() {
        Some((
            _,
            GitResult::Status {
                result: Ok(snap), ..
            },
        )) => {
            assert!(snap.has_remote, "origin was just added");
        }
        other => panic!("expected Ok status, got {other:?}"),
    }
}

#[test]
fn status_snapshot_detects_the_pull_request_forge_from_origin() {
    use helm::git::forge::Forge;
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();

    let worker = GitWorker::spawn(tmp.path(), || {});
    worker.send(GitCommand::Status);
    match worker.recv() {
        Some((
            _,
            GitResult::Status {
                result: Ok(snap), ..
            },
        )) => assert_eq!(snap.pr_remote, None, "no origin ⇒ no create-PR forge"),
        other => panic!("expected Ok status, got {other:?}"),
    }

    repo.remote("origin", "git@bitbucket.org:team/repo.git")
        .unwrap();
    worker.send(GitCommand::Status);
    match worker.recv() {
        Some((
            _,
            GitResult::Status {
                result: Ok(snap), ..
            },
        )) => assert_eq!(
            snap.pr_remote,
            Some(Forge::Bitbucket {
                workspace: "team".into(),
                repo: "repo".into(),
            }),
            "the origin host resolves to the create-PR forge"
        ),
        other => panic!("expected Ok status, got {other:?}"),
    }
}

#[test]
fn status_snapshot_carries_an_in_progress_merge() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    let base = commit_file(&repo, "f.txt", "base\n", "base");
    let base_commit = repo.find_commit(base).unwrap();
    let ours_branch = repo.head().unwrap().name().unwrap().to_string();

    let theirs = repo.branch("theirs", &base_commit, false).unwrap();
    repo.set_head(theirs.get().name().unwrap()).unwrap();
    repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
        .unwrap();
    let theirs_commit = commit_file(&repo, "f.txt", "theirs\n", "theirs");

    repo.set_head(&ours_branch).unwrap();
    repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
        .unwrap();
    commit_file(&repo, "f.txt", "ours\n", "ours");

    let annotated = repo
        .find_annotated_commit(repo.find_commit(theirs_commit).unwrap().id())
        .unwrap();
    repo.merge(&[&annotated], None, None).unwrap();

    let worker = GitWorker::spawn(tmp.path(), || {});
    worker.send(GitCommand::Status);

    match worker.recv() {
        Some((
            _,
            GitResult::Status {
                result: Ok(snap), ..
            },
        )) => {
            assert!(
                snap.op_in_progress,
                "the snapshot exposes the in-progress merge"
            );
        }
        other => panic!("expected Ok status, got {other:?}"),
    }
}

#[test]
fn pending_mutation_tracks_the_worker_queue() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    let mut config = repo.config().unwrap();
    config.set_str("user.name", "Test").unwrap();
    config.set_str("user.email", "test@example.com").unwrap();
    commit_file(&repo, "f.txt", "base\n", "base");
    fs::write(tmp.path().join("dirty.txt"), "wip").unwrap();

    let worker = GitWorker::spawn(tmp.path(), || {});
    assert_eq!(worker.pending_mutation(), None);

    worker.send(GitCommand::Status);
    assert_eq!(
        worker.pending_mutation(),
        None,
        "a read does not make the toolbar busy"
    );

    worker.send(GitCommand::Stash);
    assert_eq!(worker.pending_mutation(), Some(GitCommand::Stash));

    worker.recv().expect("reply from Status");
    assert_eq!(
        worker.pending_mutation(),
        Some(GitCommand::Stash),
        "the stash is still queued behind the drained read"
    );

    worker.recv().expect("reply from Stash");
    assert_eq!(worker.pending_mutation(), None, "empty queue after drain");
}

#[test]
fn graph_command_returns_graph_over_channel() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "a.txt", "one\n", "first");
    let second = commit_file(&repo, "a.txt", "two\n", "second");

    let worker = GitWorker::spawn(tmp.path(), || {});
    worker.send(GitCommand::Graph { limit: 0 });

    match worker.recv() {
        Some((
            _,
            GitResult::Graph {
                limit,
                result: Ok(graph),
            },
        )) => {
            assert_eq!(limit, 0, "the reply carries the requested limit");
            assert_eq!(graph.commits.len(), 2);
            // Topo + time ordering yields the tip (second) first.
            assert_eq!(graph.commits[0].oid, second);
            assert_eq!(graph.commits[0].summary, "second");
            assert!(!graph.has_more);
        }
        other => panic!("expected Ok graph, got {other:?}"),
    }
}

#[test]
fn graph_command_respects_the_limit_and_flags_more() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "a.txt", "one\n", "first");
    commit_file(&repo, "a.txt", "two\n", "second");
    commit_file(&repo, "a.txt", "three\n", "third");

    let worker = GitWorker::spawn(tmp.path(), || {});
    worker.send(GitCommand::Graph { limit: 2 });

    match worker.recv() {
        Some((
            _,
            GitResult::Graph {
                limit,
                result: Ok(graph),
            },
        )) => {
            assert_eq!(limit, 2, "the reply carries the requested limit");
            assert_eq!(graph.commits.len(), 2);
            assert!(graph.has_more, "older commits remain past the limit");
        }
        other => panic!("expected Ok graph, got {other:?}"),
    }
}

#[test]
fn commit_detail_command_returns_detail_over_channel() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    let base = commit_file(&repo, "keep.txt", "v1\n", "base");
    let second = commit_file(&repo, "keep.txt", "v2\n", "edit");

    let worker = GitWorker::spawn(tmp.path(), || {});
    worker.send(GitCommand::CommitDetail(second));

    match worker.recv() {
        Some((_, GitResult::CommitDetail(Ok(detail)))) => {
            assert_eq!(detail.meta.oid, second);
            assert_eq!(detail.meta.summary, "edit");
            assert_eq!(detail.meta.parents, vec![base]);
            assert_eq!(detail.files.len(), 1);
            assert_eq!(detail.files[0].path, "keep.txt");
        }
        other => panic!("expected Ok commit detail, got {other:?}"),
    }
}

#[test]
fn commit_file_diff_command_returns_read_only_file_diff() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "a.txt", "line1\nline2\nline3\n", "init");
    let second = commit_file(&repo, "a.txt", "line1\nCHANGED\nline3\n", "change");

    let worker = GitWorker::spawn(tmp.path(), || {});
    worker.send(GitCommand::CommitFileDiff {
        oid: second,
        path: "a.txt".to_string(),
    });

    match worker.recv() {
        Some((
            _,
            GitResult::CommitFileDiff {
                oid,
                result: Ok(file),
            },
        )) => {
            assert_eq!(oid, second);
            assert_eq!(file.path, "a.txt");
            assert!(!file.binary);
            assert_eq!(file.hunks.len(), 1);
        }
        other => panic!("expected Ok commit file diff, got {other:?}"),
    }
}

#[test]
fn rapid_fire_graph_requests_supersede_the_earlier_reply() {
    // **Load more** immediately followed by the reset on re-entering Graph
    // mode: the FIFO worker answers both, the gate (M17-13) must flag the
    // first reply as superseded whatever its echoed limit says.
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "a.txt", "one\n", "c1");

    let worker = GitWorker::spawn(tmp.path(), || {});
    worker.send(GitCommand::Graph { limit: 10 });
    worker.send(GitCommand::Graph { limit: 1 });

    let (generation, reply) = worker.recv().expect("first graph reply");
    assert!(matches!(reply, GitResult::Graph { result: Ok(_), .. }));
    assert!(
        worker.superseded(generation, reply.kind()),
        "the load-more reply answers a superseded request"
    );
    let (generation, reply) = worker.recv().expect("second graph reply");
    assert!(
        !worker.superseded(generation, reply.kind()),
        "the latest request's reply is the one to adopt"
    );
}

#[test]
fn rapid_fire_file_clicks_supersede_the_earlier_diff_reply() {
    // Click file A then file B before A's diff arrives (M9-7): A's reply is
    // superseded; B's — the latest — is not, regardless of the oids echoed.
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "a.txt", "one\n", "c1");
    let second = commit_file(&repo, "a.txt", "two\n", "c2");
    let third = commit_file(&repo, "b.txt", "bee\n", "c3");

    let worker = GitWorker::spawn(tmp.path(), || {});
    worker.send(GitCommand::CommitFileDiff {
        oid: second,
        path: "a.txt".to_string(),
    });
    worker.send(GitCommand::CommitFileDiff {
        oid: third,
        path: "b.txt".to_string(),
    });

    let (generation, reply) = worker.recv().expect("file A's reply");
    assert!(matches!(
        reply,
        GitResult::CommitFileDiff { result: Ok(_), .. }
    ));
    assert!(worker.superseded(generation, reply.kind()));
    let (generation, reply) = worker.recv().expect("file B's reply");
    assert!(!worker.superseded(generation, reply.kind()));
    assert!(
        !worker.superseded(generation, ResultKind::Status),
        "diff clicks never supersede an in-flight status request"
    );
}

#[test]
fn graph_on_non_repo_path_yields_error_over_channel() {
    let tmp = tempfile::tempdir().unwrap();

    let worker = GitWorker::spawn(tmp.path(), || {});
    worker.send(GitCommand::Graph { limit: 0 });

    match worker.recv() {
        Some((_, GitResult::Graph { result: Err(_), .. })) => {}
        other => panic!("expected Err graph, got {other:?}"),
    }
}

/// Waits for the network op to finish by draining like the app on each frame.
fn drain_until_reply(
    runner: &mut SyncRunner,
    worker: &GitWorker,
    graph_mode: bool,
) -> Vec<helm::git::worker::SyncReply> {
    for _ in 0..400 {
        let replies = drain_sync_refresh(runner, worker, graph_mode, 10);
        if !replies.is_empty() {
            return replies;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("sync op never completed");
}

#[test]
fn finished_sync_op_reloads_status_and_graph() {
    let tmp = tempfile::tempdir().unwrap();
    let bare = tmp.path().join("remote.git");
    git2::Repository::init_bare(&bare).unwrap();
    let a_dir = tmp.path().join("a");
    let repo_a = git2::Repository::init(&a_dir).unwrap();
    commit_file(&repo_a, "a.txt", "one\n", "c1");
    let branch = repo_a.head().unwrap().shorthand().unwrap().to_string();
    let url = format!("file://{}", bare.display());
    repo_a.remote("origin", &url).unwrap();
    assert!(cli::run(&a_dir, &["push", "-u", "origin", &branch])
        .unwrap()
        .success());
    assert!(cli::run(tmp.path(), &["clone", &url, "b"])
        .unwrap()
        .success());
    let b_dir = tmp.path().join("b");
    let c2 = commit_file(&repo_a, "a.txt", "two\n", "c2");
    assert!(cli::run(&a_dir, &["push"]).unwrap().success());

    let worker = GitWorker::spawn(&b_dir, || {});
    let mut runner = SyncRunner::new(&b_dir, || {});
    assert!(runner.request(SyncCommand::Pull(PullMode::Ff)));

    let replies = drain_until_reply(&mut runner, &worker, true);
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0].command, SyncCommand::Pull(PullMode::Ff));
    assert_eq!(replies[0].result, Ok(SyncOutcome::Updated));
    assert!(!runner.busy());

    match worker.recv() {
        Some((_, GitResult::Status { result: Ok(_), .. })) => {}
        other => panic!("expected status refresh, got {other:?}"),
    }
    match worker.recv() {
        Some((
            _,
            GitResult::Graph {
                result: Ok(graph), ..
            },
        )) => {
            assert_eq!(graph.commits[0].oid, c2, "graph reloaded after the pull");
        }
        other => panic!("expected graph refresh, got {other:?}"),
    }
}

#[test]
fn combined_remote_local_delete_enqueues_local_delete_after_remote_success() {
    let tmp = tempfile::tempdir().unwrap();
    let bare = tmp.path().join("remote.git");
    git2::Repository::init_bare(&bare).unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    let head = commit_file(&repo, "a.txt", "one\n", "c1");
    repo.branch("feature", &repo.find_commit(head).unwrap(), false)
        .unwrap();
    let url = format!("file://{}", bare.display());
    repo.remote("origin", &url).unwrap();
    assert!(cli::run(tmp.path(), &["push", "origin", "feature"])
        .unwrap()
        .success());
    assert!(cli::run(tmp.path(), &["fetch", "origin"])
        .unwrap()
        .success());

    let worker = GitWorker::spawn(tmp.path(), || {});
    let mut runner = SyncRunner::new(tmp.path(), || {});
    assert!(runner.request(SyncCommand::DeleteRemoteThenLocalBranch {
        remote: "origin/feature".to_owned(),
        local: "feature".to_owned(),
    }));

    let replies = drain_until_reply(&mut runner, &worker, false);
    assert_eq!(replies.len(), 1);
    assert!(replies[0].result.is_ok(), "{:?}", replies[0].result);
    match worker.recv() {
        Some((
            _,
            GitResult::Status {
                result: Ok(_),
                source,
            },
        )) => {
            assert_eq!(source, GitCommand::DeleteBranch("feature".to_owned()));
        }
        other => panic!("expected local delete after remote success, got {other:?}"),
    }
    match worker.recv() {
        Some((_, GitResult::Status { result: Ok(_), .. })) => {}
        other => panic!("expected follow-up status refresh, got {other:?}"),
    }
    assert!(repo
        .find_branch("feature", git2::BranchType::Local)
        .is_err());
    assert!(git2::Repository::open(&bare)
        .unwrap()
        .find_reference("refs/heads/feature")
        .is_err());
}

#[test]
fn combined_delete_keeps_local_branch_when_remote_delete_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let bare = tmp.path().join("remote.git");
    git2::Repository::init_bare(&bare).unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    let head = commit_file(&repo, "a.txt", "one\n", "c1");
    repo.branch("feature", &repo.find_commit(head).unwrap(), false)
        .unwrap();
    let url = format!("file://{}", bare.display());
    repo.remote("origin", &url).unwrap();

    let worker = GitWorker::spawn(tmp.path(), || {});
    let mut runner = SyncRunner::new(tmp.path(), || {});
    assert!(runner.request(SyncCommand::DeleteRemoteThenLocalBranch {
        remote: "origin/missing".to_owned(),
        local: "feature".to_owned(),
    }));

    let replies = drain_until_reply(&mut runner, &worker, false);
    assert!(replies[0].result.is_err());
    match worker.recv() {
        Some((
            _,
            GitResult::Status {
                result: Ok(_),
                source,
            },
        )) => {
            assert_eq!(source, GitCommand::Status);
        }
        other => panic!("expected only the status refresh, got {other:?}"),
    }
    assert!(repo.find_branch("feature", git2::BranchType::Local).is_ok());
}

#[test]
fn sync_refresh_skips_graph_outside_graph_mode() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "a.txt", "one\n", "c1");

    let worker = GitWorker::spawn(tmp.path(), || {});
    let mut runner = SyncRunner::new(tmp.path(), || {});
    assert!(runner.request(SyncCommand::FetchAll));

    let replies = drain_until_reply(&mut runner, &worker, false);
    assert_eq!(replies[0].result, Err(SyncError::NoRemote));

    // Sentinel: if a Graph had been requested, it would arrive before it (the
    // worker is FIFO for poll/worktree reads — only commit-addressed reads
    // jump the queue, so the sentinel must not be one).
    worker.send(GitCommand::Diff {
        path: "a.txt".to_string(),
        staged: false,
    });
    match worker.recv() {
        Some((_, GitResult::Status { result: Ok(_), .. })) => {}
        other => panic!("expected status refresh, got {other:?}"),
    }
    match worker.recv() {
        Some((_, GitResult::Diff(_))) => {}
        other => panic!("expected the sentinel, got {other:?}"),
    }
}

#[test]
fn worker_and_poll_stay_responsive_during_a_sync_op() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    commit_file(&repo, "a.txt", "one\n", "c1");
    // Slow fake remote: the ext transport blocks 5s without speaking the
    // protocol, the op will fail afterwards — the test does not wait for it.
    repo.remote("origin", "ext::sh -c 'sleep 5'").unwrap();
    let mut cfg = repo.config().unwrap();
    cfg.set_str("protocol.ext.allow", "always").unwrap();

    let lock = MutationLock::new();
    let worker = GitWorker::spawn_with_lock(tmp.path(), lock.clone(), || {});
    let mut runner = SyncRunner::new_with_lock(tmp.path(), lock, || {});
    assert!(runner.request(SyncCommand::FetchAll));
    assert!(runner.busy());
    assert!(
        !runner.request(SyncCommand::Push),
        "request while busy ⇒ ignored"
    );

    worker.send(GitCommand::Status);
    match worker.recv() {
        Some((_, GitResult::Status { result: Ok(_), .. })) => {}
        other => panic!("expected status during sync op, got {other:?}"),
    }
    worker.send(GitCommand::Stage("a.txt".into()));
    match worker.recv() {
        Some((
            _,
            GitResult::Status {
                result: Err(err), ..
            },
        )) => assert!(
            err.message().contains("another Git operation"),
            "unexpected error: {}",
            err.message()
        ),
        other => panic!("expected mutation refusal during sync op, got {other:?}"),
    }
    assert!(runner.busy(), "the network op is still running");
    assert!(runner.try_recv().is_none());
}
