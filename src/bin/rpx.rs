use rpx::{config::Config, forward, parser::Parser, resolver::filter};
use std::{collections::HashSet, net::SocketAddr};
use tokio::net::TcpListener;
use tower::{
    util::{BoxCloneService, Either},
    ServiceBuilder,
};
use tracing::{debug, error, info, info_span, Instrument, Span};

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    tracing_subscriber::fmt::init();
    let config = rpx::config::load_config()?;
    let resolver = resolver_stack(&config);
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
                            if let Err(err) =
                                forward(&mut incoming, resolver, get_parsers().into_iter()).await
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

fn get_parsers(
) -> Vec<Box<dyn Parser<String, Box<dyn std::error::Error + Send + 'static>> + Send + 'static>> {
    let h1 = rpx::parser::http::Hostname;
    let tls = rpx::parser::tls::ServiceName::default();

    vec![Box::new(h1), Box::new(tls)]
}

fn resolver_stack(
    config: &Config,
) -> BoxCloneService<
    (String, u16),
    Option<SocketAddr>,
    Box<dyn std::error::Error + Send + Sync + 'static>,
> {
    let filter = if config.services.is_empty() {
        None
    } else {
        let allowed: HashSet<String> = config.services.iter().map(|s| s.name.clone()).collect();

        let func = move |(record, port): (String, u16)| -> Result<(String, u16), filter::Error> {
            if allowed.contains(&record) {
                Ok((record, port))
            } else {
                Err(filter::Error::NotSupported(record))
            }
        };

        Some(tower::filter::Filter::<
            Either<rpx::resolver::dns::Service, rpx::resolver::void::Service>,
            _,
        >::layer(func))
    };

    let config_file = if config.services.is_empty() {
        None
    } else {
        Some(rpx::resolver::config_file::Layer::new(
            config.services.iter(),
        ))
    };

    let fallback = config
        .default_destination
        .map(rpx::resolver::fallback::Layer::new);

    let service = match config.dns {
        Some(address) => {
            let dns = rpx::resolver::dns::start(Some(address)).unwrap();
            Either::A(dns)
        }
        None => Either::B(rpx::resolver::void::Service),
    };

    let service = ServiceBuilder::new()
        .buffer(1024)
        .option_layer(fallback)
        .option_layer(filter)
        .option_layer(config_file)
        .service(service);

    BoxCloneService::new(service)
}
