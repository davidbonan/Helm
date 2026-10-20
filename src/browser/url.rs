//! Which URL a browser pane opens (browser.md §4, §5): pure string rules.

const LOCAL_HOSTS: [&str; 4] = ["localhost", "127.0.0.1", "0.0.0.0", "[::1]"];
const SEARCH_URL: &str = "https://www.google.com/search?q=";

/// The last local URL a server printed (Vite's `Local: http://localhost:5173/`…),
/// `0.0.0.0` rewritten to `localhost`; network addresses are not the preview's.
pub fn server_url_in(lines: &[String]) -> Option<String> {
    lines.iter().rev().find_map(|line| {
        line.match_indices("http")
            .filter_map(|(at, _)| local_url_at(&line[at..]))
            .last()
    })
}

/// The worktree's server URL: what it printed, else its `$PORT`.
pub fn server_url(printed: Option<String>, port: Option<u16>) -> Option<String> {
    printed.or_else(|| port.map(|port| format!("http://localhost:{port}/")))
}

/// What the URL field loads: a scheme is kept, a local host or an IP gets
/// `http://`, a host with a dot or a port `https://`, anything else is searched.
/// Blank ⇒ nothing.
pub fn typed_url(text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    if text.contains("://") || text.starts_with("about:") || text.starts_with("data:") {
        return Some(text.to_owned());
    }
    let host = host_of(text);
    if is_local_host(host) || is_ip(host) {
        return Some(format!("http://{text}"));
    }
    let authority = text.split(['/', '?', '#']).next().unwrap_or_default();
    if text.contains(char::is_whitespace) || !(host.contains('.') || authority.contains(':')) {
        let query = percent_encoding::utf8_percent_encode(text, percent_encoding::NON_ALPHANUMERIC);
        return Some(format!("{SEARCH_URL}{query}"));
    }
    Some(format!("https://{text}"))
}

/// A page the history and the favorites keep: `http(s)` only.
pub fn is_web(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

/// How the address bar shows a URL: its host, then the rest; the scheme and a
/// bare `/` path are dropped.
pub fn display_parts(url: &str) -> (&str, &str) {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let (host, path) = rest
        .find(['/', '?', '#'])
        .map_or((rest, ""), |at| rest.split_at(at));
    (host, if path == "/" { "" } else { path })
}

/// A page served from this machine (the hosts of browser.md §5).
pub fn is_local(url: &str) -> bool {
    is_web(url) && is_local_host(host_of(display_parts(url).0))
}

/// Where a tile says a page is: its host, or `:port/path` for a local page —
/// the host says nothing there (browser.md §7).
pub fn tile_location(url: &str) -> std::borrow::Cow<'_, str> {
    let (authority, path) = display_parts(url);
    if !is_local(url) {
        return authority.into();
    }
    match authority
        .rsplit_once(':')
        .filter(|(_, port)| !port.ends_with(']'))
    {
        Some((_, port)) => format!(":{port}{path}").into(),
        None => format!("{authority}{path}").into(),
    }
}

fn local_url_at(text: &str) -> Option<String> {
    let scheme = ["http://", "https://"]
        .into_iter()
        .find(|scheme| text.starts_with(scheme))?;
    let rest = &text[scheme.len()..];
    let end = rest
        .find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | '`'))
        .unwrap_or(rest.len());
    let rest = rest[..end].trim_end_matches(['.', ',', ';', ')', ']']);
    if !is_local_host(host_of(rest)) {
        return None;
    }
    let host = rest
        .strip_prefix("0.0.0.0")
        .map_or(rest.to_owned(), |tail| format!("localhost{tail}"));
    Some(format!("{scheme}{host}"))
}

fn host_of(authority_and_path: &str) -> &str {
    let authority = authority_and_path
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default();
    if authority.starts_with('[') {
        return authority.split_inclusive(']').next().unwrap_or(authority);
    }
    authority.split(':').next().unwrap_or(authority)
}

fn is_local_host(host: &str) -> bool {
    LOCAL_HOSTS.contains(&host) || host.ends_with(".localhost")
}

fn is_ip(host: &str) -> bool {
    host.parse::<std::net::Ipv4Addr>().is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &[&str]) -> Vec<String> {
        text.iter().map(|line| (*line).to_owned()).collect()
    }

    #[test]
    fn the_address_bar_shows_host_then_path_without_scheme() {
        assert_eq!(
            display_parts("http://localhost:3000/"),
            ("localhost:3000", "")
        );
        assert_eq!(
            display_parts("https://example.com/docs?q=1"),
            ("example.com", "/docs?q=1")
        );
    }

    #[test]
    fn vite_banner_gives_its_local_url_not_the_network_one() {
        let output = lines(&[
            "  VITE v5.4.2  ready in 312 ms",
            "  ➜  Local:   http://localhost:5173/",
            "  ➜  Network: http://192.168.1.20:5173/",
        ]);
        assert_eq!(
            server_url_in(&output).as_deref(),
            Some("http://localhost:5173/")
        );
    }

    #[test]
    fn the_last_printed_local_url_wins() {
        let output = lines(&[
            "started on http://localhost:3000",
            "restarted on http://127.0.0.1:3001/app",
        ]);
        assert_eq!(
            server_url_in(&output).as_deref(),
            Some("http://127.0.0.1:3001/app")
        );
    }

    #[test]
    fn any_interface_reads_as_localhost_and_trailing_punctuation_drops() {
        let output = lines(&["Listening at http://0.0.0.0:8000."]);
        assert_eq!(
            server_url_in(&output).as_deref(),
            Some("http://localhost:8000")
        );
    }

    #[test]
    fn local_subdomains_and_https_count() {
        let output = lines(&["- Local: https://app.localhost:8443 (ready)"]);
        assert_eq!(
            server_url_in(&output).as_deref(),
            Some("https://app.localhost:8443")
        );
    }

    #[test]
    fn no_local_url_means_none() {
        let output = lines(&["see https://vitejs.dev/config", "compiled in 2s"]);
        assert_eq!(server_url_in(&output), None);
    }

    #[test]
    fn printed_url_beats_the_port() {
        assert_eq!(
            server_url(Some("http://localhost:5173/".into()), Some(3000)).as_deref(),
            Some("http://localhost:5173/")
        );
        assert_eq!(
            server_url(None, Some(3001)).as_deref(),
            Some("http://localhost:3001/")
        );
        assert_eq!(server_url(None, None), None);
    }

    #[test]
    fn typed_urls_get_a_scheme_by_host() {
        assert_eq!(
            typed_url("localhost:3000/a").as_deref(),
            Some("http://localhost:3000/a")
        );
        assert_eq!(
            typed_url("192.168.1.4:8080").as_deref(),
            Some("http://192.168.1.4:8080")
        );
        assert_eq!(typed_url("docs.rs").as_deref(), Some("https://docs.rs"));
        assert_eq!(
            typed_url(" http://x.test ").as_deref(),
            Some("http://x.test")
        );
        assert_eq!(typed_url("  "), None);
    }

    #[test]
    fn a_local_tile_shows_its_port_and_path_instead_of_the_host() {
        assert_eq!(tile_location("http://localhost:3000/admin"), ":3000/admin");
        assert_eq!(tile_location("http://127.0.0.1:6006/"), ":6006");
        assert_eq!(tile_location("http://localhost/"), "localhost");
        assert_eq!(tile_location("https://docs.rs/std"), "docs.rs");
    }

    #[test]
    fn words_or_a_bare_name_are_searched() {
        assert_eq!(
            typed_url("rust egui").as_deref(),
            Some("https://www.google.com/search?q=rust%20egui")
        );
        assert_eq!(
            typed_url("egui").as_deref(),
            Some("https://www.google.com/search?q=egui")
        );
        assert_eq!(
            typed_url("devbox:8080/x").as_deref(),
            Some("https://devbox:8080/x")
        );
    }
}
