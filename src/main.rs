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
    clippy::cloned_instead_of_copied,
    clippy::inefficient_to_string,
    clippy::macro_use_imports,
    clippy::mut_mut,
    clippy::todo
)]
#![warn(missing_docs)]
#![allow(clippy::missing_panics_doc)]

use anyhow::Result;
use erooster::servers::Server;
use tokio::signal;
use tracing::{error, info};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    info!("Starting ERooster Imap Server");
    tokio::spawn(async {
        if let Err(e) = erooster::servers::Unencrypted::run().await {
            panic!("Unable to start server: {:?}", e);
        }
    });
    tokio::spawn(async {
        if let Err(e) = erooster::servers::Encrypted::run().await {
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
