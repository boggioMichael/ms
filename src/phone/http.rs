//! Just enough HTTP/1.1 for one phone page: requests with a
//! `Content-Length` body, responses with one, and connections kept open
//! between them. It works over anything that reads and writes, so the same
//! code serves the TLS connection from the phone, the plain one from a
//! tunnel, and the tests.

use std::io::{self, Read, Write};

/// Headers larger than this are refused.
const MAX_HEAD: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub method: String,
    /// The path without the query, e.g. `/api/state`.
    pub path: String,
    /// Decoded query parameters, in order.
    pub query: Vec<(String, String)>,
    /// Header names in lower case.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    pub fn param(&self, name: &str) -> Option<&str> {
        self.query
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    /// Whether the client asked to close the connection after this request.
    pub fn wants_close(&self) -> bool {
        self.header("connection")
            .is_some_and(|v| v.eq_ignore_ascii_case("close"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
    pub headers: Vec<(&'static str, String)>,
}

impl Response {
    pub fn new(status: u16, content_type: &'static str, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            content_type,
            body: body.into(),
            headers: Vec::new(),
        }
    }

    pub fn json(status: u16, body: &serde_json::Value) -> Self {
        Self::new(status, "application/json; charset=utf-8", body.to_string())
    }

    pub fn text(status: u16, body: impl Into<String>) -> Self {
        Self::new(status, "text/plain; charset=utf-8", body.into())
    }

    pub fn empty(status: u16) -> Self {
        Self::new(status, "text/plain; charset=utf-8", Vec::new())
    }

    pub fn with_header(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.headers.push((name, value.into()));
        self
    }
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        204 => "No Content",
        301 => "Moved Permanently",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        413 => "Payload Too Large",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "Unknown",
    }
}

#[derive(Debug)]
pub enum HttpError {
    Io(io::Error),
    /// The request could not be parsed; answer 400 and close.
    Malformed(&'static str),
    /// Too large; answer 413 and close.
    TooLarge,
}

impl From<io::Error> for HttpError {
    fn from(e: io::Error) -> Self {
        HttpError::Io(e)
    }
}

/// A connection: reads requests, writes responses. Bytes read past the end
/// of one request (a pipelined next one) are kept for the next call.
pub struct Conn<S> {
    stream: S,
    buffer: Vec<u8>,
}

impl<S: Read + Write> Conn<S> {
    pub fn new(stream: S) -> Self {
        Self {
            stream,
            buffer: Vec::with_capacity(4096),
        }
    }

    pub fn get_mut(&mut self) -> &mut S {
        &mut self.stream
    }

    fn fill(&mut self) -> io::Result<usize> {
        let mut chunk = [0u8; 8192];
        let n = self.stream.read(&mut chunk)?;
        self.buffer.extend_from_slice(&chunk[..n]);
        Ok(n)
    }

    /// The next request, or `None` when the client closed the connection
    /// cleanly between requests.
    pub fn read_request(&mut self, max_body: usize) -> Result<Option<Request>, HttpError> {
        let head_end = loop {
            if let Some(end) = find(&self.buffer, b"\r\n\r\n") {
                break end;
            }
            if self.buffer.len() > MAX_HEAD {
                return Err(HttpError::TooLarge);
            }
            if self.fill()? == 0 {
                return if self.buffer.is_empty() {
                    Ok(None)
                } else {
                    Err(HttpError::Malformed("connection closed mid-request"))
                };
            }
        };
        let head = std::str::from_utf8(&self.buffer[..head_end])
            .map_err(|_| HttpError::Malformed("headers are not UTF-8"))?
            .to_string();
        let mut lines = head.split("\r\n");
        let request_line = lines.next().ok_or(HttpError::Malformed("empty request"))?;
        let mut parts = request_line.split(' ');
        let (method, target, version) = (parts.next(), parts.next(), parts.next());
        let (Some(method), Some(target), Some(version)) = (method, target, version) else {
            return Err(HttpError::Malformed("bad request line"));
        };
        if !version.starts_with("HTTP/1.") {
            return Err(HttpError::Malformed("not HTTP/1.x"));
        }
        let mut headers = Vec::new();
        for line in lines {
            let Some((name, value)) = line.split_once(':') else {
                return Err(HttpError::Malformed("bad header line"));
            };
            headers.push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
        }
        let length = match headers.iter().find(|(n, _)| n == "content-length") {
            Some((_, v)) => v
                .parse::<usize>()
                .map_err(|_| HttpError::Malformed("bad content-length"))?,
            None => 0,
        };
        if headers
            .iter()
            .any(|(n, v)| n == "transfer-encoding" && !v.eq_ignore_ascii_case("identity"))
        {
            return Err(HttpError::Malformed("chunked bodies are not supported"));
        }
        if length > max_body {
            return Err(HttpError::TooLarge);
        }
        let body_start = head_end + 4;
        while self.buffer.len() < body_start + length {
            if self.fill()? == 0 {
                return Err(HttpError::Malformed("connection closed mid-body"));
            }
        }
        let body = self.buffer[body_start..body_start + length].to_vec();
        self.buffer.drain(..body_start + length);

        let (path, query) = match target.split_once('?') {
            Some((path, query)) => (path, parse_query(query)),
            None => (target, Vec::new()),
        };
        Ok(Some(Request {
            method: method.to_string(),
            path: percent_decode(path),
            query,
            headers,
            body,
        }))
    }

    pub fn write_response(&mut self, response: &Response, keep_alive: bool) -> io::Result<()> {
        let mut head = format!(
            "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: {}\r\n",
            response.status,
            reason(response.status),
            response.content_type,
            response.body.len(),
            if keep_alive { "keep-alive" } else { "close" },
        );
        for (name, value) in &response.headers {
            head.push_str(name);
            head.push_str(": ");
            head.push_str(value);
            head.push_str("\r\n");
        }
        head.push_str("\r\n");
        let mut out = head.into_bytes();
        out.extend_from_slice(&response.body);
        self.stream.write_all(&out)?;
        self.stream.flush()
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

pub fn parse_query(query: &str) -> Vec<(String, String)> {
    query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| match pair.split_once('=') {
            Some((k, v)) => (percent_decode(k), percent_decode(v)),
            None => (percent_decode(pair), String::new()),
        })
        .collect()
}

/// `%41` and `+` decoded; invalid escapes kept as they are.
pub fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
                match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(b) => {
                        out.push(b);
                        i += 2;
                    }
                    None => out.push(b'%'),
                }
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// A stream that hands out its input in small pieces and records writes.
    struct Trickle {
        input: Cursor<Vec<u8>>,
        piece: usize,
        written: Vec<u8>,
    }

    impl Read for Trickle {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let n = buf.len().min(self.piece);
            self.input.read(&mut buf[..n])
        }
    }

    impl Write for Trickle {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.written.extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn conn(input: &str, piece: usize) -> Conn<Trickle> {
        Conn::new(Trickle {
            input: Cursor::new(input.as_bytes().to_vec()),
            piece,
            written: Vec::new(),
        })
    }

    #[test]
    fn parses_requests_arriving_in_pieces_and_pipelined() {
        let mut c = conn(
            "POST /api/heard?k=abc%20d&x HTTP/1.1\r\nHost: pc\r\nContent-Length: 5\r\n\r\nhelloGET / HTTP/1.1\r\nConnection: close\r\n\r\n",
            3,
        );
        let first = c.read_request(1024).unwrap().unwrap();
        assert_eq!(first.method, "POST");
        assert_eq!(first.path, "/api/heard");
        assert_eq!(first.param("k"), Some("abc d"));
        assert_eq!(first.param("x"), Some(""));
        assert_eq!(first.header("HOST"), Some("pc"));
        assert_eq!(first.body, b"hello");
        assert!(!first.wants_close());
        let second = c.read_request(1024).unwrap().unwrap();
        assert_eq!(second.path, "/");
        assert!(second.wants_close());
        assert!(c.read_request(1024).unwrap().is_none());
    }

    #[test]
    fn refuses_oversized_bodies_and_garbage() {
        let mut c = conn("POST / HTTP/1.1\r\nContent-Length: 99\r\n\r\n", 64);
        assert!(matches!(c.read_request(10), Err(HttpError::TooLarge)));
        let mut c = conn("\x16\x03\x01\x02\x00\r\n\r\n", 64);
        assert!(matches!(c.read_request(10), Err(HttpError::Malformed(_))));
        let mut c = conn("GET / HTTP/1.1\r\nContent-Le", 64);
        assert!(matches!(c.read_request(10), Err(HttpError::Malformed(_))));
    }

    #[test]
    fn writes_a_response_with_its_length() {
        let mut c = conn("", 64);
        c.write_response(&Response::text(200, "hi").with_header("X-Test", "1"), true)
            .unwrap();
        let written = String::from_utf8(c.get_mut().written.clone()).unwrap();
        assert!(written.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(written.contains("Content-Length: 2\r\n"));
        assert!(written.contains("Connection: keep-alive\r\n"));
        assert!(written.contains("X-Test: 1\r\n"));
        assert!(written.ends_with("\r\n\r\nhi"));
    }

    #[test]
    fn decodes_percent_escapes() {
        assert_eq!(percent_decode("a%2Fb+c"), "a/b c");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
        assert_eq!(percent_decode("%D7%A9"), "ש");
    }
}
