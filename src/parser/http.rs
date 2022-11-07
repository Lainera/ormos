use tracing::{debug, error, instrument};

const GET: &[u8] = b"GET";
const HEAD: &[u8] = b"HEAD";
const OPTIONS: &[u8] = b"OPTIONS";
const CONNECT: &[u8] = b"CONNECT";
const POST: &[u8] = b"POST";
const PUT: &[u8] = b"PUT";
const PATCH: &[u8] = b"PATCH";
const TRACE: &[u8] = b"TRACE";
const DELETE: &[u8] = b"DELETE";
const METHODS: [&[u8]; 9] = [GET, HEAD, OPTIONS, CONNECT, POST, PUT, PATCH, TRACE, DELETE];

/// Parses the hostname from http/1 bytes
pub struct Hostname;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Supplied bytes are not valid http/1")]
    NotHttp1,
}

impl super::Parser<String, Box<dyn std::error::Error + Send + 'static>> for Hostname {
    #[instrument(skip_all, fields(input_size = input.len()))]
    fn parse(
        &mut self,
        input: &[u8],
    ) -> Result<Option<String>, Box<dyn std::error::Error + Send + 'static>> {
        if !is_http(input) {
            Err(Box::new(Error::NotHttp1))
        } else {
            Ok(try_read_hostname(input))
        }
    }
}

#[instrument(skip_all, fields(len = buf.len()))]
pub fn is_http(buf: &[u8]) -> bool {
    debug!("Got buf: {}", buf.len());
    METHODS.iter().any(|&method| {
        debug!("Method: {method:?}");
        buf.starts_with(method)
    })
}

#[instrument(skip_all, fields(len = buf.len()))]
fn try_read_hostname(buf: &[u8]) -> Option<String> {
    buf.as_ref()
        .split(|byte| *byte == b'\n')
        .filter_map(|line| std::str::from_utf8(line).ok())
        .find(|line| {
            debug!("Got a line: {line}");
            line.starts_with("Host") || line.starts_with("host")
        })
        .and_then(|header| header.split(':').nth(1))
        .map(|hostname| hostname.trim())
        .map(ToOwned::to_owned)
}
