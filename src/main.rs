//! Erooster Mail Server
//!
//! Erooster is a rust native imap server build on modern solutions.
//! The goal being easy to setup, use and maintain for smaller mail servers
//! while being also fast and efficient.
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
#![allow(clippy::missing_panics_doc)]

use std::sync::Arc;

use anyhow::Result;
use erooster::{config::Config, servers::Server};
use tokio::signal;
use tracing::{error, info};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    info!("Starting ERooster Imap Server");
    let config = Arc::new(Config::load("./config.yml")?);
    let config_clone = Arc::clone(&config);
    tokio::spawn(async move {
        if let Err(e) = erooster::servers::Unencrypted::run_imap(Arc::clone(&config_clone)).await {
            panic!("Unable to start server: {:?}", e);
        }
    });
    tokio::spawn(async move {
        if let Err(e) = erooster::servers::Encrypted::run_imap(Arc::clone(&config)).await {
            panic!("Unable to start TLS server: {:?}", e);
        }
    });

    match signal::ctrl_c().await {
        Ok(()) => {}
        Err(err) => {
            error!("Unable to listen for shutdown signal: {}", err);
            // we also shut down in case of error
        }
    }
    Ok(())
}
