#![doc = include_str!("../Readme.md")]

pub mod config;
pub mod parser;
pub mod resolver;

use parser::Parser;
use std::{future::poll_fn, net::SocketAddr, ops::Deref, time::Duration};

use bytes::{BufMut, BytesMut};
use tokio::{
    io::{self, AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};
use tracing::{debug, instrument, trace, warn};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("Unexpected error occurred: `{0}`")]
    Other(Box<dyn std::error::Error + Sync + Send + 'static>),
}

#[instrument(skip_all, fields(incoming = ?incoming.peer_addr(), port = ?incoming.local_addr().map(|a| a.port())))]
/// Forwards traffic from incoming connection to preconfigured destination.
///
/// Forwarding traffic involves following steps:
/// - Parse the desired service name
/// - Resolve remote address by service name
/// - Forward traffic to the remote address
///
/// ### Parse
///
/// Parsing is delegated to a number of [parsers][Parser]. Incoming traffic is read
/// until one of the parsers succeeds, all of the parsers fail or timeout occurs.
/// Until either of the outcomes happen, async task pulls bytes out of remote connection in a loop
/// offering full buffer for parsing on every tick.
///
/// ### Resolve
///
/// Resolvers are implementors of [service][tower::Service], which accept `(String, u16)` and
/// respond with optional socket address. There are couple of resolvers available in [corresponding
/// module][resolver]
///
/// ### Forward
///
/// Once connection to remote destination had been established all incoming data collected so far
/// is forwarded to dst. Task resolves when connection is closed.
pub async fn forward<'a, R, I>(
    incoming: &mut TcpStream,
    mut resolver: R,
    parsers: I,
) -> Result<(), Error>
where
    R: tower::Service<
        (String, u16),
        Response = Option<SocketAddr>,
        Error = Box<dyn std::error::Error + Send + Sync + 'static>,
    >,
    I: Iterator<Item = Box<dyn Parser<String, Box<dyn std::error::Error + Send + 'static>> + Send + 'static>>
{
    debug!("enter");
    let port = incoming.local_addr()?.port();

    let mut buf = BytesMut::with_capacity(256);
    
    let mut parsers: Vec<_> = parsers.collect();
    let mut parsers: Vec<&mut _> = parsers.iter_mut()
        .map(|boxed| boxed.as_mut())
        .collect();

    let with_deadline = {
        let duration = Duration::from_secs(30);
        tokio::time::timeout(duration, parse_service_name(incoming, &mut buf, parsers.as_mut_slice()))
    };

    // Read the service name from incoming stream
    let service_name = match with_deadline.await {
        Err(_) => {
            debug!("Timeout");
            // Failed to read the service name in time -> abort
            None
        }
        Ok(Err(err)) => {
            // Error can only occur in the event of IO issue, abort;
            debug!("Failed to resolve service name: {err}");
            None
        }
        Ok(Ok(None)) => {
            debug!("None of the parsers were able to parse the name");
            // Use default name (empty string) and feed to resolver -> if it has default destination,
            // it would resolve regardless, if it doesn't - then it would resolve None with noop
            Some(String::new())
        }
        Ok(Ok(Some(name))) => {
            debug!(host = name.as_str(), "resolved service name");
            Some(name)
        }
    };

    // Resolve service name to some address
    let outgoing = match service_name {
        None => None,
        Some(name) => {
            // Ensure resolver is ready
            poll_fn(|cx| resolver.poll_ready(cx))
                .await
                .map_err(Error::Other)?;

            resolver.call((name, port)).await.map_err(Error::Other)?
        }
    };

    if let Some(outgoing) = outgoing {
        debug!(destination = ?outgoing, "resolved destination");
        let mut outgoing = TcpStream::connect(outgoing).await?;

        // Copy everything read so far
        outgoing.write_all(&buf).await?;

        let (incoming, outgoing) = io::copy_bidirectional(incoming, &mut outgoing).await?;
        debug!(incoming, outgoing, "After copy_bidirectional");
    } else {
        warn!("Failed to resolve destination for {incoming:?}, dropping request");
        incoming.shutdown().await?;
    }

    Ok(())
}

#[instrument(skip_all, fields(parsers = parsers.len()))]
async fn parse_service_name<'b, 'p, B, R>(
    reader: &mut R,
    buf: &'b mut B,
    parsers: &'p mut [&'p mut (dyn Parser<String, Box<dyn std::error::Error + Send + 'static>>
                          + Send
                          + 'static)],
) -> Result<Option<String>, Error>
where
    B: BufMut + Deref<Target = [u8]>,
    R: AsyncReadExt + Unpin + core::fmt::Debug,
{
    debug!("enter");
    let mut active: Vec<usize> = (0..parsers.len()).collect();

    loop {
        if active.is_empty() {
            return Ok(None);
        }

        reader.read_buf(buf).await?;

        trace!("read");

        let mut valid = Vec::new();

        for &ix in active.iter() {
            let parser = &mut parsers[ix];

            match parser.parse(buf) {
                // Parser successfully parsed the name
                Ok(Some(name)) => return Ok(Some(name)),
                // Parser still requires more data
                Ok(None) => valid.push(ix),
                // Parser failed to parse - no need to ask it anymore
                Err(err) => {
                    debug!("Failed to parse: {err}");
                }
            }
        }

        active = valid;
    }
}
