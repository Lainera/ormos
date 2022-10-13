use anyhow::Error;
use bytes::BufMut;
use rustls::internal::msgs::message::OpaqueMessage;
use tokio::io::AsyncReadExt;
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

#[instrument]
pub fn is_http(buf: &[u8]) -> bool {
    debug!("Got buf: {}", buf.len());
    METHODS.iter().any(|&method| {
        debug!("Method: {method:?}");
        buf.starts_with(method)
    })
}

#[instrument]
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

#[instrument]
pub async fn read_hostname<B, R>(buf: &mut B, reader: &mut R) -> Result<String, Error>
where
    R: AsyncReadExt + Unpin + core::fmt::Debug,
    B: BufMut + AsRef<[u8]> + core::fmt::Debug,
{
    debug!("Entered");
    if let Some(hostname) = try_read_hostname(buf.as_ref()) {
        return Ok(hostname);
    }
    debug!("Reading more");
    let mut total = buf.as_ref().len();
    loop {
        let read = reader.read_buf(buf).await?;
        debug!("Read: {read}/{total}");
        if read == 0 {
            return Err(anyhow::anyhow!("EOF, but failed to read hostname"));
        } else {
            total += read;
        }

        match try_read_hostname(buf.as_ref()) {
            Some(hostname) => {
                debug!("Got hostname after reading {total} bytes -> {hostname}");
                return Ok(hostname);
            }
            None if total > OpaqueMessage::MAX_WIRE_SIZE => {
                error!("Buf exceeded max size");
                return Err(anyhow::anyhow!("Buf exceeded max size"));
            }
            None => {
                debug!("Read {total} bytes, no hostname yet");
            }
        }
    }
}
