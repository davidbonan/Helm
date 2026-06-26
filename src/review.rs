use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineComment {
    pub old_lineno: Option<u32>,
    pub new_lineno: Option<u32>,
    pub code: String,
    pub note: String,
}

impl LineComment {
    pub fn line_ref(&self) -> Option<u32> {
        self.new_lineno.or(self.old_lineno)
    }
}

/// Review comments of a single repo, grouped by file path (sorted) so the prompt
/// and the badge count are deterministic.
pub type FileComments = BTreeMap<String, Vec<LineComment>>;

/// One read-only comment already posted on a PR thread (pull-requests.md §11):
/// author + body, anchored elsewhere by `ForgeThreads`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadComment {
    pub author: String,
    pub body: String,
    /// Forge id of this comment, when known — the reply target (its thread root's
    /// id) the diff overlay posts a reply against (pull-requests.md §11).
    pub id: Option<u64>,
}

/// Existing PR threads of one repo, keyed by file path then the `(old, new)`
/// anchor of the commented row, each a chronologically-ordered thread. The full
/// pair — not a bare line number — keeps a deleted row (old N) and an added row
/// (new N) sharing a number from rendering the same thread twice. Read-only: the
/// diff view renders these but never edits them (the editable draft store stays
/// `FileComments`).
pub type ForgeAnchor = (Option<u32>, Option<u32>);
pub type ForgeThreads = BTreeMap<String, BTreeMap<ForgeAnchor, Vec<ThreadComment>>>;

/// Adds `comment` under `file`, replacing in place a comment already anchored at
/// the same line (one note per line).
pub fn add_comment(store: &mut FileComments, file: &str, comment: LineComment) {
    let line = comment.line_ref();
    let file_comments = store.entry(file.to_owned()).or_default();
    match file_comments.iter_mut().find(|c| c.line_ref() == line) {
        Some(existing) => *existing = comment,
        None => file_comments.push(comment),
    }
}

/// Removes the comment anchored at `line` of `file`, purging the file entry when it
/// becomes empty so the store never keeps blank groups.
pub fn delete_comment(store: &mut FileComments, file: &str, line: Option<u32>) {
    if let Some(file_comments) = store.get_mut(file) {
        file_comments.retain(|c| c.line_ref() != line);
        if file_comments.is_empty() {
            store.remove(file);
        }
    }
}

/// Total number of comments across every file — the `Send (N)` badge count.
pub fn count(store: &FileComments) -> usize {
    store.values().map(Vec::len).sum()
}

/// Which store a line note belongs to (pull-requests.md §11). The working-tree and
/// commit surfaces only have `Agent`; the PR review surface keeps the two pools
/// apart so forge comments are never forced through the agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewPool {
    /// Notes batched to the local agent (the Sparkles gutter button + "Send to …").
    Agent,
    /// PR review comments posted to GitHub / Bitbucket on submit (the
    /// MessageSquarePlus gutter button + "Submit review").
    Forge,
}

/// Review action raised by the diff view, applied by the app (RC5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewIntent {
    SaveComment {
        pool: ReviewPool,
        file: String,
        comment: LineComment,
    },
    DeleteComment {
        pool: ReviewPool,
        file: String,
        line: Option<u32>,
    },
    SendToAgent,
    /// Launch the agent to address an existing PR comment thread anchored at the
    /// `(old, new)` row of `file` (pull-requests.md §11). Only raised on the PR
    /// review surface.
    AskAgentOnThread {
        file: String,
        old: Option<u32>,
        new: Option<u32>,
    },
    /// Post a reply to an existing PR comment thread (pull-requests.md §11):
    /// `comment_id` is the thread root's forge id, `body` the reply text. Raised
    /// from both the diff overlay and the center inline-comment card.
    ReplyToThread {
        comment_id: u64,
        body: String,
    },
}

pub fn build_review_prompt(comments: &FileComments) -> String {
    let mut out = String::from("Please address the following code review comments.\n");
    for (file, file_comments) in comments {
        if file_comments.is_empty() {
            continue;
        }
        out.push_str(&format!("\n## {file}\n"));
        for c in file_comments {
            let loc = match c.line_ref() {
                Some(n) => format!("L{n}"),
                None => "?".to_string(),
            };
            out.push_str(&format!(
                "\n- {loc} `{}`\n  {}\n",
                c.code.trim_end(),
                c.note
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn comment(old: Option<u32>, new: Option<u32>, code: &str, note: &str) -> LineComment {
        LineComment {
            old_lineno: old,
            new_lineno: new,
            code: code.to_string(),
            note: note.to_string(),
        }
    }

    #[test]
    fn add_comment_appends_then_replaces_same_line_in_place() {
        let mut store = FileComments::new();
        add_comment(&mut store, "a.rs", comment(None, Some(1), "x", "first"));
        add_comment(&mut store, "a.rs", comment(None, Some(2), "y", "second"));
        assert_eq!(store["a.rs"].len(), 2);

        // Same line ref ⇒ replace in place, keeping the count and position.
        add_comment(&mut store, "a.rs", comment(None, Some(1), "x", "edited"));
        assert_eq!(store["a.rs"].len(), 2);
        assert_eq!(store["a.rs"][0].note, "edited");
        assert_eq!(store["a.rs"][1].note, "second");
    }

    #[test]
    fn delete_comment_removes_the_line_and_purges_empty_files() {
        let mut store = FileComments::new();
        add_comment(&mut store, "a.rs", comment(None, Some(1), "x", "n1"));
        add_comment(&mut store, "a.rs", comment(None, Some(2), "y", "n2"));

        delete_comment(&mut store, "a.rs", Some(1));
        assert_eq!(store["a.rs"].len(), 1);
        assert_eq!(store["a.rs"][0].line_ref(), Some(2));

        delete_comment(&mut store, "a.rs", Some(2));
        assert!(
            !store.contains_key("a.rs"),
            "the file entry is purged once its last comment is gone"
        );
    }

    #[test]
    fn count_sums_comments_across_files() {
        let mut store = FileComments::new();
        assert_eq!(count(&store), 0);
        add_comment(&mut store, "a.rs", comment(None, Some(1), "x", "n"));
        add_comment(&mut store, "a.rs", comment(None, Some(2), "y", "n"));
        add_comment(&mut store, "b.rs", comment(None, Some(3), "z", "n"));
        assert_eq!(count(&store), 3);
    }

    #[test]
    fn line_ref_prefers_new_then_old() {
        assert_eq!(comment(Some(5), Some(10), "x", "n").line_ref(), Some(10));
        assert_eq!(comment(Some(5), None, "x", "n").line_ref(), Some(5));
        assert_eq!(comment(None, None, "x", "n").line_ref(), None);
    }

    #[test]
    fn prompt_groups_files_in_btree_order() {
        let mut comments = BTreeMap::new();
        comments.insert(
            "src/zeta.rs".to_string(),
            vec![comment(None, Some(1), "z", "fix z")],
        );
        comments.insert(
            "src/alpha.rs".to_string(),
            vec![comment(None, Some(2), "a", "fix a")],
        );

        let prompt = build_review_prompt(&comments);
        let alpha = prompt.find("src/alpha.rs").unwrap();
        let zeta = prompt.find("src/zeta.rs").unwrap();
        assert!(alpha < zeta, "files must appear in sorted order:\n{prompt}");
    }

    #[test]
    fn prompt_uses_new_lineno_then_old_lineno() {
        let mut comments = BTreeMap::new();
        comments.insert(
            "f.rs".to_string(),
            vec![
                comment(Some(3), Some(8), "added", "on new"),
                comment(Some(4), None, "removed", "on old"),
            ],
        );

        let prompt = build_review_prompt(&comments);
        assert!(prompt.contains("- L8 `added`"), "{prompt}");
        assert!(prompt.contains("- L4 `removed`"), "{prompt}");
    }

    #[test]
    fn prompt_aggregates_multiple_files_with_code_and_note() {
        let mut comments = BTreeMap::new();
        comments.insert(
            "a.rs".to_string(),
            vec![comment(None, Some(1), "let a = 1;", "rename a")],
        );
        comments.insert(
            "b.rs".to_string(),
            vec![comment(None, Some(2), "let b = 2;", "rename b")],
        );

        let prompt = build_review_prompt(&comments);
        assert!(prompt.contains("## a.rs"), "{prompt}");
        assert!(prompt.contains("## b.rs"), "{prompt}");
        assert!(prompt.contains("`let a = 1;`"), "{prompt}");
        assert!(prompt.contains("rename a"), "{prompt}");
        assert!(prompt.contains("`let b = 2;`"), "{prompt}");
        assert!(prompt.contains("rename b"), "{prompt}");
    }

    #[test]
    fn prompt_trims_trailing_newline_from_code() {
        let mut comments = BTreeMap::new();
        comments.insert(
            "a.rs".to_string(),
            vec![comment(None, Some(1), "    let a = 1;\n", "note")],
        );

        let prompt = build_review_prompt(&comments);
        assert!(prompt.contains("`    let a = 1;`"), "{prompt}");
    }
}
