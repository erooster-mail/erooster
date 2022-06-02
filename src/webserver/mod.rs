use crate::config::Config;
use axum::{response::Html, routing::get, Router};
use axum_server::tls_rustls::RustlsConfig;
use std::{net::SocketAddr, sync::Arc};
use tower_http::trace::TraceLayer;
use tracing::info;

/// Starts the webserver used for the admin page and metrics
#[tracing::instrument(skip(config))]
pub async fn start(config: Arc<Config>) -> color_eyre::eyre::Result<()> {
    let metrics_middleware =
        axum_opentelemetry_middleware::RecorderMiddlewareBuilder::new(env!("CARGO_PKG_NAME"));
    let metrics_middleware = metrics_middleware.build();

    let addrs: Vec<SocketAddr> = if let Some(listen_ips) = &config.listen_ips {
        listen_ips
            .iter()
            .map(|ip| format!("{}:{}", ip, config.webserver.port).parse().unwrap())
            .collect()
    } else {
        vec![format!("0.0.0.0:{}", config.webserver.port).parse()?]
    };
    for addr in addrs {
        let config = Arc::clone(&config);
        let metrics_middleware = metrics_middleware.clone();
        tokio::spawn(async move {
            let config = Arc::clone(&config);
            let app = Router::new()
                .route("/", get(handler))
                .route(
                    "/metrics",
                    get(axum_opentelemetry_middleware::metrics_endpoint),
                )
                .layer(metrics_middleware.clone())
                .layer(TraceLayer::new_for_http());

            if config.webserver.tls {
                let tls_config = RustlsConfig::from_pem_file(
                    config.tls.cert_path.clone(),
                    config.tls.key_path.clone(),
                )
                .await
                .unwrap();

                info!("[Webserver] Listening on {}", addr);
                axum_server::bind_rustls(addr, tls_config)
                    .serve(app.into_make_service())
                    .await
                    .unwrap();
            } else {
                info!("[Webserver] Listening on {}", addr);
                axum_server::bind(addr)
                    .serve(app.into_make_service())
                    .await
                    .unwrap();
            }
        });
    }

    Ok(())
}

#[allow(clippy::unused_async)]
async fn handler() -> Html<&'static str> {
    Html("<h1>Hello, World!</h1>")
}
