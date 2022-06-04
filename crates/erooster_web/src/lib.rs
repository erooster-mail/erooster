//! Core logic for the webserver for the erooster mail server
//!
#![feature(string_remove_matches)]
#![deny(unsafe_code)]
#![warn(
    clippy::cognitive_complexity,
    clippy::branches_sharing_code,
    clippy::imprecise_flops,
    clippy::missing_const_for_fn,
    clippy::mutex_integer,
    clippy::path_buf_push_overwrite,
    clippy::redundant_pub_crate,
    clippy::pedantic,
    clippy::dbg_macro,
    clippy::todo,
    clippy::fallible_impl_from,
    clippy::filetype_is_file,
    clippy::suboptimal_flops,
    clippy::fn_to_numeric_cast_any,
    clippy::if_then_some_else_none,
    clippy::imprecise_flops,
    clippy::lossy_float_literal,
    clippy::panic_in_result_fn,
    clippy::clone_on_ref_ptr
)]
#![warn(missing_docs)]
#![allow(
    clippy::missing_panics_doc,
    clippy::missing_errors_doc,
    clippy::module_name_repetitions
)]

use erooster_core::config::Config;
use askama::Template;
use axum::{
    http::{header, HeaderValue, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::get,
    Extension, Router,
};
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
                    "/.well-known/autoconfig/mail/config-v1.1.xml",
                    get(autoconfig),
                )
                .route(
                    "/metrics",
                    get(axum_opentelemetry_middleware::metrics_endpoint),
                )
                .layer(Extension(Arc::clone(&config)))
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

#[allow(clippy::unused_async)]
async fn autoconfig(Extension(config): Extension<Arc<Config>>) -> impl IntoResponse {
    let template = AutoconfigTemplate {
        domain: config.mail.hostname.clone(),
        displayname: config.mail.displayname.clone(),
    };
    XmlTemplate(template)
}

#[derive(Template)]
#[allow(dead_code)]
#[template(path = "autoconfig.xml")]
struct AutoconfigTemplate {
    domain: String,
    displayname: String,
}

struct XmlTemplate<T>(T);

impl<T> IntoResponse for XmlTemplate<T>
where
    T: Template,
{
    fn into_response(self) -> Response {
        match self.0.render() {
            Ok(xml) => (
                [(
                    header::CONTENT_TYPE,
                    HeaderValue::from_static(mime::TEXT_XML.as_ref()),
                )],
                xml,
            )
                .into_response(),
            Err(err) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to render template. Error: {}", err),
            )
                .into_response(),
        }
    }
}
