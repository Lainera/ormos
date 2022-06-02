use rpx::{dns, forward};
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tracing::{debug, error, info, info_span, Instrument, Span};

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    tracing_subscriber::fmt::init();
    let config = rpx::config::load_config()?;
    let resolver = dns::start::<256>(&config)?;
    let ports = config.listening_ports();

    let _ = info_span!("main");
    let mut listener_handles = Vec::new();
    for port in ports {
        let listening_addr: SocketAddr = (config.bind_address, port).into();
        let listener = TcpListener::bind(listening_addr).await?;
        info!("Started listener {listener:?}");

        let resolver = resolver.clone();
        let handle = tokio::spawn({
            let listener_span = info_span!("listener");
            async move {
                while let Ok((mut incoming, _)) = listener.accept().await {
                    debug!("Incoming connection {:?}", incoming);
                    let resolver = resolver.clone();
                    tokio::spawn({
                        let forwarder_span = info_span!("forwarder");
                        forwarder_span.follows_from(Span::current());
                        async move {
                            if let Err(err) = forward(&mut incoming, resolver).await {
                                error!("Failed to forward traffic for {incoming:?} -> {err}");
                            }
                        }
                        .instrument(forwarder_span)
                    });
                }
            }
            .instrument(listener_span)
        });

        listener_handles.push(handle);
    }

    futures::future::join_all(listener_handles).await;
    Ok(())
}
