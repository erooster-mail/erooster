use std::{net::SocketAddr, sync::Arc};

use axum::{response::Html, routing::get, Router};
use tower_http::trace::TraceLayer;
use tracing::info;

use crate::config::Config;

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

    let addr = SocketAddr::from(([127, 0, 0, 1], config.webserver_port));
    info!("Webserver listening on {}", addr);

    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await?;
    Ok(())
}

#[allow(clippy::unused_async)]
async fn handler() -> Html<&'static str> {
    Html("<h1>Hello, World!</h1>")
}
