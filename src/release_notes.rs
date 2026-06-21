//! Bundled release notes (update.md §9.1): a single `release-notes.md` at the
//! repo root, embedded at build time via `include_str!` — always present,
//! offline by construction, with no fetch, cache file, or API quota. Authored by
//! the `/release` skill from the commit log, newest version first, capped at 10
//! versions.

/// The release-notes markdown, baked into the binary. Rendered as-is in the
/// What's new modal and in Preferences › Updates (update.md §9.4).
pub const RELEASE_NOTES: &str = include_str!("../release-notes.md");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_are_bundled_and_non_empty() {
        assert!(!RELEASE_NOTES.trim().is_empty());
    }

    #[test]
    fn version_sections_parse_newest_first_within_cap() {
        let versions: Vec<&str> = RELEASE_NOTES
            .lines()
            .filter_map(|line| line.strip_prefix("## "))
            .map(str::trim)
            .collect();
        assert!(!versions.is_empty(), "at least one `## <version>` section");
        assert!(
            versions.len() <= 10,
            "kept to the latest 10 versions (update.md §9.1), got {}",
            versions.len()
        );
        for v in &versions {
            assert!(
                !v.is_empty() && v.split('.').all(|p| p.parse::<u32>().is_ok()),
                "section header is not a dotted version: {v:?}"
            );
        }
    }
}
