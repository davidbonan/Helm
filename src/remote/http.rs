//! The few HTTP exchanges of the phone server: one request per connection, read up
//! to the end of its head — never past it, the WebSocket frames follow on the same
//! stream.

use std::io::{self, Read, Write};

const MAX_HEAD_BYTES: usize = 8 * 1024;
const MAX_HEADERS: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestHead {
    pub method: String,
    pub path: String,
    query: Option<String>,
    headers: Vec<(String, String)>,
}

impl RequestHead {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    pub fn query_param(&self, name: &str) -> Option<&str> {
        self.query.as_deref()?.split('&').find_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            (key == name).then_some(value)
        })
    }

    pub fn is_websocket_upgrade(&self) -> bool {
        self.header("upgrade")
            .is_some_and(|value| value.eq_ignore_ascii_case("websocket"))
    }
}

/// `None` for a closed, oversized or malformed request.
pub fn read_head(stream: &mut impl Read) -> io::Result<Option<RequestHead>> {
    let mut raw = Vec::new();
    let mut byte = [0u8; 1];
    while !raw.ends_with(b"\r\n\r\n") {
        if raw.len() >= MAX_HEAD_BYTES || stream.read(&mut byte)? == 0 {
            return Ok(None);
        }
        raw.push(byte[0]);
    }
    Ok(parse_head(&raw))
}

fn parse_head(raw: &[u8]) -> Option<RequestHead> {
    let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
    let mut request = httparse::Request::new(&mut headers);
    if !request.parse(raw).ok()?.is_complete() {
        return None;
    }
    let target = request.path?;
    let (path, query) = match target.split_once('?') {
        Some((path, query)) => (path, Some(query.to_owned())),
        None => (target, None),
    };
    Some(RequestHead {
        method: request.method?.to_owned(),
        path: path.to_owned(),
        query,
        headers: request
            .headers
            .iter()
            .map(|h| {
                (
                    h.name.to_owned(),
                    String::from_utf8_lossy(h.value).into_owned(),
                )
            })
            .collect(),
    })
}

pub struct Response<'a> {
    pub status: &'static str,
    pub headers: Vec<(&'static str, String)>,
    pub body: &'a [u8],
}

impl<'a> Response<'a> {
    pub fn new(status: &'static str, content_type: &'static str, body: &'a [u8]) -> Self {
        Self {
            status,
            headers: vec![("Content-Type", content_type.to_owned())],
            body,
        }
    }

    pub fn with(mut self, name: &'static str, value: String) -> Self {
        self.headers.push((name, value));
        self
    }

    pub fn write_to(&self, stream: &mut impl Write) -> io::Result<()> {
        let mut head = format!("HTTP/1.1 {}\r\n", self.status);
        for (name, value) in &self.headers {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        head.push_str(&format!(
            "Content-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
            self.body.len()
        ));
        stream.write_all(head.as_bytes())?;
        stream.write_all(self.body)?;
        stream.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_head_is_read_up_to_its_end_and_no_further() {
        let mut stream: &[u8] =
            b"GET /pair?t=abc&x=1 HTTP/1.1\r\nHost: h\r\nCookie: a=b\r\n\r\nFRAME";

        let head = read_head(&mut stream).unwrap().unwrap();

        assert_eq!((head.method.as_str(), head.path.as_str()), ("GET", "/pair"));
        assert_eq!(head.query_param("t"), Some("abc"));
        assert_eq!(
            head.header("cookie"),
            Some("a=b"),
            "names are case-insensitive"
        );
        assert_eq!(
            stream, b"FRAME",
            "the bytes after the head stay on the stream"
        );
    }

    #[test]
    fn an_oversized_head_is_refused() {
        let flood = vec![b'a'; MAX_HEAD_BYTES + 1];

        assert_eq!(read_head(&mut flood.as_slice()).unwrap(), None);
    }
}
