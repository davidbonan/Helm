//! A page that could not load (browser.md §14): why, as the failure page tells it.

const URL_ERROR_DOMAIN: &str = "NSURLErrorDomain";
const WEBKIT_ERROR_DOMAIN: &str = "WebKitErrorDomain";
/// Another navigation took over.
const CANCELLED: i64 = -999;
/// The response became a download (`WebKitErrorFrameLoadInterruptedByPolicyChange`).
const INTERRUPTED_BY_POLICY: i64 = 102;
/// A plug-in took the load over.
const PLUGIN_HANDLED_LOAD: i64 = 204;

#[derive(Debug, Clone, PartialEq)]
pub struct LoadFailure {
    pub url: String,
    pub reason: FailureReason,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FailureReason {
    /// Nothing listens on the address: a dev server not running.
    Refused,
    HostNotFound,
    TimedOut,
    Offline,
    /// TLS: the connection or the certificate.
    Insecure(String),
    /// The page's web process died.
    Crashed,
    Other(String),
}

impl FailureReason {
    /// The reason of an `NSError` a navigation failed with; `None` for the
    /// errors that are no failure (a newer navigation, a download).
    pub fn of_error(domain: &str, code: i64, description: &str) -> Option<Self> {
        let ignored = match domain {
            URL_ERROR_DOMAIN => code == CANCELLED,
            WEBKIT_ERROR_DOMAIN => matches!(code, INTERRUPTED_BY_POLICY | PLUGIN_HANDLED_LOAD),
            _ => false,
        };
        if ignored {
            return None;
        }
        if domain != URL_ERROR_DOMAIN {
            return Some(Self::Other(description.to_owned()));
        }
        Some(match code {
            -1004 => Self::Refused,
            -1003 | -1006 => Self::HostNotFound,
            -1001 => Self::TimedOut,
            -1009 | -1020 => Self::Offline,
            -1206..=-1200 | -2000 => Self::Insecure(description.to_owned()),
            _ => Self::Other(description.to_owned()),
        })
    }

    pub fn title(&self, site: &str) -> String {
        match self {
            Self::Crashed => "This page stopped working".to_owned(),
            _ => format!("Can't reach {site}"),
        }
    }

    pub fn detail(&self) -> &str {
        match self {
            Self::Refused => "Nothing answers on this address. Is the server running?",
            Self::HostNotFound => "The server's address can't be found.",
            Self::TimedOut => "The server took too long to answer.",
            Self::Offline => "This Mac is offline.",
            Self::Crashed => "Its web process ended. Reload to start it again.",
            Self::Insecure(description) | Self::Other(description) => description,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refused_connection_reads_as_a_server_not_running() {
        let reason = FailureReason::of_error(URL_ERROR_DOMAIN, -1004, "Could not connect");
        assert_eq!(reason, Some(FailureReason::Refused));
    }

    #[test]
    fn a_navigation_taken_over_or_turned_download_is_no_failure() {
        assert_eq!(FailureReason::of_error(URL_ERROR_DOMAIN, -999, ""), None);
        assert_eq!(FailureReason::of_error(WEBKIT_ERROR_DOMAIN, 102, ""), None);
    }

    #[test]
    fn a_certificate_error_keeps_the_engine_s_words() {
        let reason = FailureReason::of_error(URL_ERROR_DOMAIN, -1202, "Untrusted cert");
        assert_eq!(reason.unwrap().detail(), "Untrusted cert");
    }
}
