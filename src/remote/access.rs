//! Who may talk to the phone server (specs/remote.md §3.2): the single-use
//! pairing code of the QR, the session cookie a paired device carries, the
//! `Origin` a WebSocket upgrade must come from.

use std::net::SocketAddr;

use crate::remote::devices::COOKIE_MAX_AGE_SECS;

pub const SESSION_COOKIE: &str = "helm_session";

const TOKEN_BYTES: usize = 16;

/// A pairing code is spent by its first use, and dies unused after this long.
pub const PAIRING_CODE_LIFETIME_MS: u64 = 5 * 60 * 1000;

/// Random bits, hex.
#[derive(Clone, PartialEq, Eq)]
pub struct Token(String);

impl Token {
    /// 128 bits.
    pub fn mint() -> Self {
        Self::random(TOKEN_BYTES)
    }

    pub fn random(bytes: usize) -> Self {
        let mut buffer = vec![0u8; bytes];
        unsafe { libc::arc4random_buf(buffer.as_mut_ptr().cast(), buffer.len()) };
        Self(buffer.iter().map(|b| format!("{b:02x}")).collect())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn matches(&self, candidate: &str) -> bool {
        constant_time_eq(&self.0, candidate)
    }
}

/// The comparison time says nothing about the prefix matched.
pub fn constant_time_eq(ours: &str, theirs: &str) -> bool {
    let (ours, theirs) = (ours.as_bytes(), theirs.as_bytes());
    ours.len() == theirs.len()
        && ours
            .iter()
            .zip(theirs)
            .fold(0u8, |diff, (a, b)| diff | (a ^ b))
            == 0
}

/// What the QR code carries: single use — the server forgets it once redeemed.
pub struct PairingCode {
    token: Token,
    expires_ms: u64,
}

impl PairingCode {
    pub fn mint(now_ms: u64) -> Self {
        Self {
            token: Token::mint(),
            expires_ms: now_ms + PAIRING_CODE_LIFETIME_MS,
        }
    }

    pub fn as_str(&self) -> &str {
        self.token.as_str()
    }

    pub fn is_live(&self, now_ms: u64) -> bool {
        now_ms < self.expires_ms
    }

    pub fn redeems(&self, candidate: &str, now_ms: u64) -> bool {
        self.is_live(now_ms) && self.token.matches(candidate)
    }
}

/// What a request is checked against, for one bound address.
pub struct Access {
    origin: String,
}

impl Access {
    pub fn new(server: SocketAddr) -> Self {
        Self {
            origin: format!("http://{server}"),
        }
    }

    pub fn origin(&self) -> &str {
        &self.origin
    }

    /// What the QR code encodes.
    pub fn pairing_url(&self, code: &PairingCode) -> String {
        format!("{}/pair?t={}", self.origin, code.as_str())
    }

    /// A WebSocket upgrade needs our own `Origin`: another page open on the phone
    /// cannot drive the agents with the cookie it would inherit.
    pub fn accepts_origin(&self, origin: Option<&str>) -> bool {
        origin == Some(self.origin.as_str())
    }
}

/// `Set-Cookie` value carrying a device's session token.
pub fn session_cookie(token: &Token) -> String {
    format!(
        "{SESSION_COOKIE}={}; HttpOnly; SameSite=Strict; Path=/; Max-Age={COOKIE_MAX_AGE_SECS}",
        token.as_str()
    )
}

/// The session token of a request's `Cookie` header.
pub fn session_token(cookie_header: Option<&str>) -> Option<&str> {
    cookie_header?.split(';').find_map(|pair| {
        let (name, value) = pair.trim().split_once('=')?;
        (name == SESSION_COOKIE).then_some(value)
    })
}

/// What the Mac is told about who reaches it, as a native notification: fired
/// from the server thread, so with helm hidden too.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessAlert {
    Paired {
        device: String,
    },
    TwoAddresses {
        device: String,
    },
    /// The server left the address the paired phones bookmarked.
    Moved,
}

impl AccessAlert {
    pub fn title(&self) -> String {
        match self {
            Self::Paired { .. } => "New phone paired".to_owned(),
            Self::TwoAddresses { device } => format!("{device} is connected from two addresses"),
            Self::Moved => "Phone access has a new address".to_owned(),
        }
    }

    pub fn body(&self) -> String {
        match self {
            Self::Paired { device } => format!(
                "{device} can now reach your agents. Not yours? Revoke it in Preferences › Phone."
            ),
            Self::TwoAddresses { .. } => {
                "If one isn't yours, revoke it in Preferences › Phone.".to_owned()
            }
            Self::Moved => {
                "Your phone's bookmark no longer works: run Open on phone and scan the code again."
                    .to_owned()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn access() -> Access {
        Access::new("192.168.1.20:5123".parse().unwrap())
    }

    #[test]
    fn minted_tokens_are_128_bits_of_hex_and_differ() {
        let (a, b) = (Token::mint(), Token::mint());

        assert_eq!(a.as_str().len(), 32);
        assert!(a.as_str().bytes().all(|c| c.is_ascii_hexdigit()));
        assert!(a != b);
    }

    #[test]
    fn the_pairing_url_points_at_the_server_with_the_code() {
        let code = PairingCode::mint(0);

        assert_eq!(
            access().pairing_url(&code),
            format!("http://192.168.1.20:5123/pair?t={}", code.as_str())
        );
    }

    #[test]
    fn a_pairing_code_redeems_only_itself_and_only_for_five_minutes() {
        let code = PairingCode::mint(1_000);
        let value = code.as_str().to_owned();

        assert!(code.redeems(&value, 1_000 + PAIRING_CODE_LIFETIME_MS - 1));
        assert!(!code.redeems(&value[1..], 1_000));
        assert!(
            !code.redeems(&value, 1_000 + PAIRING_CODE_LIFETIME_MS),
            "an unused code dies"
        );
    }

    #[test]
    fn the_session_token_is_found_among_other_cookies() {
        let header = format!("theme=dark; {SESSION_COOKIE}=abc");

        assert_eq!(session_token(Some(&header)), Some("abc"));
        assert_eq!(session_token(Some("theme=dark")), None);
        assert_eq!(session_token(None), None);
    }

    #[test]
    fn the_session_cookie_outlives_the_browser_session() {
        let cookie = session_cookie(&Token::mint());

        assert!(cookie.contains("HttpOnly; SameSite=Strict; Path=/"));
        assert!(cookie.ends_with(&format!("Max-Age={COOKIE_MAX_AGE_SECS}")));
    }

    #[test]
    fn an_upgrade_needs_our_origin() {
        let access = access();

        assert!(access.accepts_origin(Some("http://192.168.1.20:5123")));
        assert!(!access.accepts_origin(Some("http://evil.example")));
        assert!(!access.accepts_origin(None));
    }
}
