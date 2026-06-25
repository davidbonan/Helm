//! Bitbucket token storage in the macOS Keychain via the `security` CLI
//! (pull-requests.md §3, service `helm.bitbucket`). The non-secret email lives
//! in `Prefs`; the token is read/written here and never persisted to TOML.

use std::process::Command;

const SERVICE: &str = "helm.bitbucket";

/// `security find-generic-password -s helm.bitbucket -a <email> -w` — prints the
/// token to stdout, exits non-zero when the item is absent.
pub fn read_args(email: &str) -> Vec<String> {
    vec![
        "find-generic-password".into(),
        "-s".into(),
        SERVICE.into(),
        "-a".into(),
        email.into(),
        "-w".into(),
    ]
}

/// `security add-generic-password -U -s helm.bitbucket -a <email> -w <token>` —
/// `-U` replaces an existing item instead of failing.
pub fn store_args(email: &str, token: &str) -> Vec<String> {
    vec![
        "add-generic-password".into(),
        "-U".into(),
        "-s".into(),
        SERVICE.into(),
        "-a".into(),
        email.into(),
        "-w".into(),
        token.into(),
    ]
}

/// Read the stored token for `email`; `None` when absent or `security` fails.
pub fn read_token(email: &str) -> Option<String> {
    let out = Command::new("security")
        .args(read_args(email))
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let token = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (!token.is_empty()).then_some(token)
}

/// Store (or replace) the token for `email`; returns whether `security` succeeded.
pub fn store_token(email: &str, token: &str) -> bool {
    Command::new("security")
        .args(store_args(email, token))
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_args_query_the_helm_service_and_account() {
        assert_eq!(
            read_args("me@corp.com"),
            vec![
                "find-generic-password",
                "-s",
                "helm.bitbucket",
                "-a",
                "me@corp.com",
                "-w",
            ]
        );
    }

    #[test]
    fn store_args_update_in_place_with_the_token() {
        assert_eq!(
            store_args("me@corp.com", "secret"),
            vec![
                "add-generic-password",
                "-U",
                "-s",
                "helm.bitbucket",
                "-a",
                "me@corp.com",
                "-w",
                "secret",
            ]
        );
    }
}
