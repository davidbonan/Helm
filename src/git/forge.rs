//! Forge detection for the **Create pull request** graph action (git.md §9): the
//! `origin` remote URL is parsed into a known cloud forge, which builds the
//! prefilled create-PR web URL opened in the browser. Cloud only —
//! `github.com` / `bitbucket.org`; a self-hosted or unknown host yields `None`
//! (the menu entry stays hidden). No network, no API: `git2` keeps no transport
//! (overview §4), the browser carries the rest (title, description, reviewers).

/// A recognized cloud forge behind a repo's `origin` remote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Forge {
    GitHub { owner: String, repo: String },
    Bitbucket { workspace: String, repo: String },
}

impl Forge {
    /// Prefilled create-PR web URL for `source` → `dest` on this forge.
    pub fn pull_request_url(&self, source: &str, dest: &str) -> String {
        match self {
            // GitHub: compare/<base>...<head> (base = target, head = source);
            // `?expand=1` opens the PR form directly rather than the diff.
            Forge::GitHub { owner, repo } => format!(
                "https://github.com/{owner}/{repo}/compare/{}...{}?expand=1",
                encode(dest),
                encode(source),
            ),
            Forge::Bitbucket { workspace, repo } => format!(
                "https://bitbucket.org/{workspace}/{repo}/pull-requests/new?source={}&dest={}",
                encode(source),
                encode(dest),
            ),
        }
    }
}

/// Parse a remote URL into a known cloud forge. Handles the three forms a git
/// remote takes — scp-like `git@host:owner/repo.git`,
/// `ssh://git@host/owner/repo.git` and `https://[user@]host/owner/repo.git` —
/// and recognizes only the cloud hosts. Anything else (GitLab, self-hosted,
/// garbage) ⇒ `None`.
pub fn parse_remote(url: &str) -> Option<Forge> {
    let (host, path) = split_host_path(url.trim())?;
    let path = path.trim_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let mut segments = path.split('/');
    let owner = segments.next().filter(|s| !s.is_empty())?;
    let repo = segments.next().filter(|s| !s.is_empty())?;
    match host.as_str() {
        "github.com" => Some(Forge::GitHub {
            owner: owner.to_owned(),
            repo: repo.to_owned(),
        }),
        "bitbucket.org" => Some(Forge::Bitbucket {
            workspace: owner.to_owned(),
            repo: repo.to_owned(),
        }),
        _ => None,
    }
}

/// Split a remote URL into its host and path, across the scheme'd
/// (`scheme://[user@]host[:port]/path`) and scp-like (`[user@]host:path`) forms.
fn split_host_path(url: &str) -> Option<(String, String)> {
    if let Some(rest) = url.split("://").nth(1) {
        let (authority, path) = rest.split_once('/')?;
        let host = authority.rsplit('@').next()?.split(':').next()?;
        Some((host.to_owned(), path.to_owned()))
    } else {
        let (authority, path) = url.split_once(':')?;
        let host = authority.rsplit('@').next()?;
        Some((host.to_owned(), path.to_owned()))
    }
}

/// Minimal percent-encoding for a branch name placed in the create-PR URL. Git
/// ref names already forbid spaces and `?`/`*`; `#`, `%`, `&` and `+` are legal
/// in a ref yet break a URL path/query, so those are escaped. `/` stays literal
/// — valid in both the path and the query value, the way the forges render it.
fn encode(branch: &str) -> String {
    let mut out = String::with_capacity(branch.len());
    for ch in branch.chars() {
        match ch {
            ' ' => out.push_str("%20"),
            '#' => out.push_str("%23"),
            '%' => out.push_str("%25"),
            '&' => out.push_str("%26"),
            '+' => out.push_str("%2B"),
            '?' => out.push_str("%3F"),
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_github_across_url_forms() {
        let expected = Forge::GitHub {
            owner: "acme".to_owned(),
            repo: "webapp".to_owned(),
        };
        assert_eq!(
            parse_remote("git@github.com:acme/webapp.git"),
            Some(expected.clone())
        );
        assert_eq!(
            parse_remote("https://github.com/acme/webapp.git"),
            Some(expected.clone())
        );
        assert_eq!(
            parse_remote("ssh://git@github.com/acme/webapp.git"),
            Some(expected.clone())
        );
        // No `.git` suffix, trailing slash, https with embedded user.
        assert_eq!(
            parse_remote("https://acme@github.com/acme/webapp"),
            Some(expected.clone())
        );
        assert_eq!(
            parse_remote("https://github.com/acme/webapp/"),
            Some(expected)
        );
    }

    #[test]
    fn parses_bitbucket_across_url_forms() {
        let expected = Forge::Bitbucket {
            workspace: "team".to_owned(),
            repo: "repo".to_owned(),
        };
        assert_eq!(
            parse_remote("git@bitbucket.org:team/repo.git"),
            Some(expected.clone())
        );
        assert_eq!(
            parse_remote("https://user@bitbucket.org/team/repo.git"),
            Some(expected.clone())
        );
        assert_eq!(
            parse_remote("ssh://git@bitbucket.org/team/repo.git"),
            Some(expected)
        );
    }

    #[test]
    fn unknown_hosts_and_garbage_yield_none() {
        assert_eq!(parse_remote("git@gitlab.com:a/b.git"), None);
        assert_eq!(parse_remote("https://example.org/a/b.git"), None);
        assert_eq!(parse_remote("https://github.com/onlyowner"), None);
        assert_eq!(parse_remote("not a url"), None);
        assert_eq!(parse_remote(""), None);
    }

    #[test]
    fn github_url_puts_dest_as_base_and_source_as_head() {
        let forge = Forge::GitHub {
            owner: "acme".to_owned(),
            repo: "webapp".to_owned(),
        };
        assert_eq!(
            forge.pull_request_url("feature/login", "main"),
            "https://github.com/acme/webapp/compare/main...feature/login?expand=1",
        );
    }

    #[test]
    fn bitbucket_url_carries_source_and_dest() {
        let forge = Forge::Bitbucket {
            workspace: "team".to_owned(),
            repo: "repo".to_owned(),
        };
        assert_eq!(
            forge.pull_request_url("feature/login", "develop"),
            "https://bitbucket.org/team/repo/pull-requests/new?source=feature/login&dest=develop",
        );
    }

    #[test]
    fn url_encodes_branch_specials_keeping_slashes() {
        let forge = Forge::Bitbucket {
            workspace: "team".to_owned(),
            repo: "repo".to_owned(),
        };
        assert_eq!(
            forge.pull_request_url("a+b&c#d", "feature/x"),
            "https://bitbucket.org/team/repo/pull-requests/new?source=a%2Bb%26c%23d&dest=feature/x",
        );
    }
}
