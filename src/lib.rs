#![doc = include_str!("../Readme.md")]

pub mod config;
pub mod dns;
pub mod http;
pub mod tls;

use std::time::Duration;

use anyhow::Error;
use bytes::{BufMut, BytesMut};
use dns::{Resolver, Running};
use tokio::{
    io::{self, AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};
use tracing::{debug, instrument, warn};

#[instrument(skip(incoming), fields(incoming = ?incoming.local_addr(), resolver = %resolver))]
pub async fn forward(
    incoming: &mut TcpStream,
    resolver: Resolver<Running>,
) -> Result<(), anyhow::Error> {
    debug!("Started forwarder for {incoming:?}");
    let port = incoming.local_addr()?.port();
    debug!("Got port from incoming connection: {port}");

    let mut buf = BytesMut::with_capacity(256);

    let with_deadline = {
        let duration = Duration::from_secs(30);
        tokio::time::timeout(duration, read_name(&mut buf, incoming))
    };

    let outgoing = match with_deadline.await {
        Err(_) => {
            debug!("Failed to resolve service name in time");
            incoming.shutdown().await?;
            return Ok(());
        }
        Ok(Err(err)) => {
            warn!("Failed to resolve service name: {err}, falling back to default destination");
            resolver.default_destination().await
        }
        Ok(Ok(name)) => {
            debug!(host = name.as_str(), "resolved  sni");
            resolver.resolve(name, port).await
        }
    }?;

    if outgoing.is_none() {
        warn!("Requested service for {incoming:?} is not configured, dropping request");
        incoming.shutdown().await?;
        return Ok(());
    }

    let outgoing = outgoing.unwrap();
    debug!("Forwarding request to {outgoing}");
    let mut outgoing = TcpStream::connect(outgoing).await?;

    // copy everything that was already read;
    outgoing.write_all(buf.as_ref()).await?;
    let (incoming, outgoing) = io::copy_bidirectional(incoming, &mut outgoing).await?;

    debug!(incoming, outgoing, "After copy_bidirectional");
    Ok(())
}

#[instrument]
pub async fn read_name<B, R>(buf: &mut B, reader: &mut R) -> Result<String, Error>
where
    R: AsyncReadExt + Unpin + core::fmt::Debug,
    B: BufMut + AsRef<[u8]> + core::fmt::Debug,
{
    let mut read = 0;
    loop {
        read += reader.read_buf(buf).await?;
        if read >= 10 {
            break;
        }
    }

    if http::is_http(&buf.as_ref()[..10]) {
        debug!("reading hostname");
        http::read_hostname(buf, reader).await
    } else {
        debug!("reading sni");
        tls::read_sni(buf, reader).await
    }
}
