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

pub fn build_review_prompt(comments: &BTreeMap<String, Vec<LineComment>>) -> String {
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
