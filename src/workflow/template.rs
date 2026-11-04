use std::collections::BTreeMap;

/// Variables every workflow can use without declaring them (workflows.md §3).
pub const BUILTINS: [&str; 3] = ["branch", "worktree", "repo"];

/// A placeholder is `{name}` with `name` made of `[a-z0-9_]` only: any other brace
/// (code, JSON) is plain text and never flagged.
pub fn is_placeholder_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

enum Segment<'a> {
    Text(&'a str),
    Placeholder(&'a str),
}

fn segments(text: &str) -> Vec<Segment<'_>> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        let Some(close) = after.find(['{', '}']) else {
            break;
        };
        let name = &after[..close];
        if after[close..].starts_with('}') && is_placeholder_name(name) {
            out.push(Segment::Text(&rest[..open]));
            out.push(Segment::Placeholder(name));
            rest = &after[close + 1..];
        } else {
            out.push(Segment::Text(&rest[..open + 1 + close]));
            rest = &after[close..];
        }
    }
    out.push(Segment::Text(rest));
    out
}

pub fn placeholders(text: &str) -> Vec<&str> {
    segments(text)
        .into_iter()
        .filter_map(|segment| match segment {
            Segment::Placeholder(name) => Some(name),
            Segment::Text(_) => None,
        })
        .collect()
}

/// Replaces each known placeholder in one pass (a value is never expanded again);
/// an unknown one stays verbatim — validation refuses it before launch.
pub fn render(text: &str, vars: &BTreeMap<String, String>) -> String {
    let mut out = String::with_capacity(text.len());
    for segment in segments(text) {
        match segment {
            Segment::Text(text) => out.push_str(text),
            Segment::Placeholder(name) => match vars.get(name) {
                Some(value) => out.push_str(value),
                None => {
                    out.push('{');
                    out.push_str(name);
                    out.push('}');
                }
            },
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn placeholders_ignore_code_and_json_braces() {
        let text = "Fix {ticket} on {branch}. fn x() {} {\"a\": 1} {Ticket} {{nested}}";
        assert_eq!(placeholders(text), vec!["ticket", "branch", "nested"]);
    }

    #[test]
    fn render_substitutes_every_occurrence_and_keeps_the_rest_verbatim() {
        let out = render(
            "{ticket}: see {ticket} ({missing}) fn x() {}",
            &vars(&[("ticket", "BAPS-12")]),
        );
        assert_eq!(out, "BAPS-12: see BAPS-12 ({missing}) fn x() {}");
    }

    #[test]
    fn render_does_not_expand_braces_inside_a_value() {
        let out = render("{a} {b}", &vars(&[("a", "{b}"), ("b", "x")]));
        assert_eq!(out, "{b} x");
    }
}
