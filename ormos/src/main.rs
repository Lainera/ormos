use config::Config;
use rpx::forward;
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tower::{util::BoxCloneService, ServiceBuilder};
use tracing::{debug, error, info, info_span, Instrument, Span};

mod config;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    tracing_subscriber::fmt::init();
    let config = config::load_config()?;
    let resolver = resolver_stack(&config);

    let _ = info_span!("main");
    let mut listener_handles = Vec::new();
    for listener in config.listen {
        let acceptor = TcpListener::bind(listener.address).await?;
        info!("Started listener {listener:?}");

        let resolver = resolver.clone();
        let handle = tokio::spawn({
            let listener_span = info_span!("listener");
            async move {
                while let Ok((mut incoming, _)) = acceptor.accept().await {
                    debug!("Incoming connection {:?}", incoming);
                    let resolver = resolver.clone();
                    let parsers = listener
                        .parsers()
                        .iter()
                        .map(Into::into)
                        .collect::<Vec<_>>();
                    tokio::spawn({
                        let forwarder_span = info_span!("forwarder");
                        forwarder_span.follows_from(Span::current());
                        async move {
                            if let Err(err) =
                                forward(&mut incoming, resolver, parsers.into_iter()).await
                            {
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

fn resolver_stack(
    config: &Config,
) -> BoxCloneService<
    (String, u16),
    Option<SocketAddr>,
    Box<dyn std::error::Error + Send + Sync + 'static>,
> {
    let service = ServiceBuilder::new()
        .buffer(1024)
        .option_layer(config.fallback.clone())
        .option_layer(config.filter.clone())
        .option_layer(config.override_rules.clone())
        .option_layer(config.rewrite.clone())
        .option_layer(config.dns.clone())
        .service(rpx::resolver::void::Service);

    BoxCloneService::new(service)
}
