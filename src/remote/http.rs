//! The few HTTP exchanges of the phone server: one request per connection, read up
//! to the end of its head — never past it, the WebSocket frames follow on the same
//! stream.

use std::io::{self, Read, Write};
use std::ops::Range;

use percent_encoding::percent_decode_str;

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

    /// A parameter the phone percent-encoded: a path.
    pub fn decoded_query_param(&self, name: &str) -> Option<String> {
        let raw = self.query_param(name)?;
        let decoded = percent_decode_str(raw).decode_utf8().ok()?;
        Some(decoded.into_owned())
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

/// The bytes a `Range` header asks of a body of `len` bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestedRange {
    Whole,
    Part(Range<u64>),
    Unsatisfiable,
}

impl RequestedRange {
    /// A header that is not a single `bytes` range is ignored, as HTTP allows.
    pub fn of(header: Option<&str>, len: u64) -> Self {
        let bounds = header
            .and_then(|value| value.strip_prefix("bytes="))
            .filter(|spec| !spec.contains(','))
            .and_then(|spec| spec.split_once('-'));
        let Some((first, last)) = bounds else {
            return Self::Whole;
        };
        let part = match (first.parse::<u64>(), last.parse::<u64>()) {
            (Ok(first), Ok(last)) if first <= last => first..last.saturating_add(1).min(len),
            (Ok(first), Err(_)) if last.is_empty() => first..len,
            (Err(_), Ok(suffix)) if first.is_empty() => len.saturating_sub(suffix)..len,
            _ => return Self::Whole,
        };
        if part.start >= part.end {
            return Self::Unsatisfiable;
        }
        Self::Part(part)
    }
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
        self.write_head(stream, self.body.len() as u64)?;
        stream.write_all(self.body)?;
        stream.flush()
    }

    /// The head, then `len` bytes read from `body` in place of the response's own.
    pub fn write_streaming(
        &self,
        stream: &mut impl Write,
        body: &mut impl Read,
        len: u64,
    ) -> io::Result<()> {
        self.write_head(stream, len)?;
        io::copy(&mut body.take(len), stream)?;
        stream.flush()
    }

    fn write_head(&self, stream: &mut impl Write, content_length: u64) -> io::Result<()> {
        let mut head = format!("HTTP/1.1 {}\r\n", self.status);
        for (name, value) in &self.headers {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        head.push_str(&format!(
            "Content-Length: {content_length}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n"
        ));
        stream.write_all(head.as_bytes())
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
    fn a_range_names_the_bytes_to_send_or_cannot_be_satisfied() {
        let of = |header| RequestedRange::of(Some(header), 100);

        assert_eq!(of("bytes=10-19"), RequestedRange::Part(10..20));
        assert_eq!(of("bytes=90-"), RequestedRange::Part(90..100));
        assert_eq!(of("bytes=-10"), RequestedRange::Part(90..100));
        assert_eq!(
            of("bytes=90-500"),
            RequestedRange::Part(90..100),
            "cut at the end"
        );
        assert_eq!(of("bytes=100-"), RequestedRange::Unsatisfiable);
        assert_eq!(of("bytes=-0"), RequestedRange::Unsatisfiable);
        assert_eq!(
            of("bytes=0-1,5-6"),
            RequestedRange::Whole,
            "several ranges are ignored"
        );
        assert_eq!(of("lines=1-2"), RequestedRange::Whole);
        assert_eq!(RequestedRange::of(None, 100), RequestedRange::Whole);
    }

    #[test]
    fn an_oversized_head_is_refused() {
        let flood = vec![b'a'; MAX_HEAD_BYTES + 1];

        assert_eq!(read_head(&mut flood.as_slice()).unwrap(), None);
    }
}
