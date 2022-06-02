use crate::config::Config;
use axum::{response::Html, routing::get, Router};
use axum_server::tls_rustls::RustlsConfig;
use std::sync::Arc;
use tower_http::trace::TraceLayer;
use tracing::info;

/// Starts the webserver used for the admin page and metrics
#[tracing::instrument(skip(config))]
pub async fn start(config: Arc<Config>) -> color_eyre::eyre::Result<()> {
    let metrics_middleware =
        axum_opentelemetry_middleware::RecorderMiddlewareBuilder::new(env!("CARGO_PKG_NAME"));
    let metrics_middleware = metrics_middleware.build();

    let app = Router::new()
        .route("/", get(handler))
        .route(
            "/metrics",
            get(axum_opentelemetry_middleware::metrics_endpoint),
        )
        .layer(metrics_middleware)
        .layer(TraceLayer::new_for_http());

    let addr = format!("{}:{}", config.webserver.ip, config.webserver.port).parse()?;

    if config.webserver.tls {
        let config =
            RustlsConfig::from_pem_file(config.tls.cert_path.clone(), config.tls.key_path.clone())
                .await?;
        info!("[Webserver] Listening on {}", addr);
        axum_server::bind_rustls(addr, config)
            .serve(app.into_make_service())
            .await?;
    } else {
        axum::Server::bind(&addr)
            .serve(app.into_make_service())
            .await?;
    }

    Ok(())
}

#[allow(clippy::unused_async)]
async fn handler() -> Html<&'static str> {
    Html("<h1>Hello, World!</h1>")
}
