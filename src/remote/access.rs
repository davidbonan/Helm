//! Who may talk to the phone server (specs/remote.md §3): a token minted per
//! start, exchanged once for a session cookie; an idle clock that stops access
//! when no phone stays connected; the single-use pairing code of the QR.

use std::net::SocketAddr;

pub const SESSION_COOKIE: &str = "helm_session";

/// Access stops after this long with no connected client.
pub const IDLE_STOP_MS: u64 = 2 * 60 * 60 * 1000;

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

/// The checks every request goes through, for one start of the server.
pub struct Access {
    token: Token,
    origin: String,
}

impl Access {
    pub fn new(token: Token, server: SocketAddr) -> Self {
        Self {
            token,
            origin: format!("http://{server}"),
        }
    }

    /// What the QR code encodes.
    pub fn pairing_url(&self) -> String {
        format!("{}/pair?t={}", self.origin, self.token.as_str())
    }

    /// The `t` query value of `/pair`.
    pub fn pairs(&self, token: &str) -> bool {
        self.token.matches(token)
    }

    /// `Set-Cookie` value handed back by a successful pairing.
    pub fn session_cookie(&self) -> String {
        format!(
            "{SESSION_COOKIE}={}; HttpOnly; SameSite=Strict; Path=/",
            self.token.as_str()
        )
    }

    /// The request's `Cookie` header carries this start's session.
    pub fn is_paired(&self, cookie_header: Option<&str>) -> bool {
        cookie_header
            .and_then(session_value)
            .is_some_and(|value| self.token.matches(value))
    }

    /// A WebSocket upgrade also needs our own `Origin`: another page open on the
    /// phone cannot drive the agents with the cookie it would inherit.
    pub fn accepts_upgrade(&self, cookie_header: Option<&str>, origin: Option<&str>) -> bool {
        self.is_paired(cookie_header) && origin == Some(self.origin.as_str())
    }
}

fn session_value(cookie_header: &str) -> Option<&str> {
    cookie_header.split(';').find_map(|pair| {
        let (name, value) = pair.trim().split_once('=')?;
        (name == SESSION_COOKIE).then_some(value)
    })
}

/// Counts connected clients and says when access has been idle for too long.
/// Starts idle: a start nobody pairs with expires too.
#[derive(Debug)]
pub struct IdleClock {
    clients: usize,
    idle_since_ms: u64,
}

impl IdleClock {
    pub fn started(now_ms: u64) -> Self {
        Self {
            clients: 0,
            idle_since_ms: now_ms,
        }
    }

    pub fn clients(&self) -> usize {
        self.clients
    }

    pub fn connect(&mut self) {
        self.clients += 1;
    }

    pub fn disconnect(&mut self, now_ms: u64) {
        self.clients = self.clients.saturating_sub(1);
        if self.clients == 0 {
            self.idle_since_ms = now_ms;
        }
    }

    pub fn is_expired(&self, now_ms: u64) -> bool {
        self.clients == 0 && now_ms.saturating_sub(self.idle_since_ms) >= IDLE_STOP_MS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn access() -> Access {
        Access::new(Token("ab".repeat(16)), "192.168.1.20:5123".parse().unwrap())
    }

    #[test]
    fn minted_tokens_are_128_bits_of_hex_and_differ() {
        let (a, b) = (Token::mint(), Token::mint());

        assert_eq!(a.as_str().len(), 32);
        assert!(a.as_str().bytes().all(|c| c.is_ascii_hexdigit()));
        assert!(a != b, "a new start must invalidate the previous pairing");
    }

    #[test]
    fn the_pairing_url_points_at_the_server_with_the_token() {
        assert_eq!(
            access().pairing_url(),
            format!("http://192.168.1.20:5123/pair?t={}", "ab".repeat(16))
        );
    }

    #[test]
    fn only_this_start_token_pairs() {
        let access = access();

        assert!(access.pairs(&"ab".repeat(16)));
        assert!(!access.pairs(&"ab".repeat(15)), "a prefix is not the token");
        assert!(!access.pairs(&"cd".repeat(16)));
    }

    #[test]
    fn the_session_cookie_is_found_among_others() {
        let access = access();
        let header = format!("theme=dark; {SESSION_COOKIE}={}", "ab".repeat(16));

        assert!(access.is_paired(Some(&header)));
        assert!(!access.is_paired(Some("theme=dark")));
        assert!(!access.is_paired(None));
    }

    #[test]
    fn an_upgrade_needs_the_session_and_our_origin() {
        let access = access();
        let cookie = access.session_cookie();
        let cookie = cookie.split(';').next();

        assert!(access.accepts_upgrade(cookie, Some("http://192.168.1.20:5123")));
        assert!(!access.accepts_upgrade(cookie, Some("http://evil.example")));
        assert!(!access.accepts_upgrade(cookie, None));
        assert!(!access.accepts_upgrade(None, Some("http://192.168.1.20:5123")));
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
    fn a_start_nobody_pairs_with_expires_after_the_idle_delay() {
        let clock = IdleClock::started(1_000);

        assert!(!clock.is_expired(1_000 + IDLE_STOP_MS - 1));
        assert!(clock.is_expired(1_000 + IDLE_STOP_MS));
    }

    #[test]
    fn a_connected_phone_holds_access_and_its_departure_restarts_the_delay() {
        let mut clock = IdleClock::started(0);
        clock.connect();
        assert!(
            !clock.is_expired(10 * IDLE_STOP_MS),
            "a client never expires"
        );

        clock.disconnect(10 * IDLE_STOP_MS);
        assert!(!clock.is_expired(10 * IDLE_STOP_MS + 1));
        assert!(clock.is_expired(11 * IDLE_STOP_MS));
    }
}
