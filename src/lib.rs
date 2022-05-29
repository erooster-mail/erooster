//! Core logic for the erooster mail server
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

use std::{path::Path, sync::Arc};

use color_eyre::Result;
use tracing::error;

use crate::config::Config;

pub(crate) mod imap_commands;
// TODO make this only pub for benches and tests
#[allow(missing_docs)]
pub mod line_codec;
pub(crate) mod smtp_commands;

/// The backend logic of the server
pub mod backend;

/// The core server logic for the imapserver.
/// This is the tls and non tls variant of the imapserver.
pub mod imap_servers;

/// The core server logic for the smtpserver.
/// This is the tls and non tls variant of the smtpserver.
pub mod smtp_servers;

/// The configuration file for the server
pub mod config;

/// Returns the config struct from the provided location or defaults
pub async fn get_config(config_path: String) -> Result<Arc<Config>> {
    let config = if Path::new(&config_path).exists() {
        Arc::new(Config::load(config_path).await?)
    } else if Path::new("/etc/erooster/config.yml").exists() {
        Arc::new(Config::load("/etc/erooster/config.yml").await?)
    } else if Path::new("/etc/erooster/config.yaml").exists() {
        Arc::new(Config::load("/etc/erooster/config.yaml").await?)
    } else {
        error!("No config file found. Please follow the readme.");
        color_eyre::eyre::bail!("No config file found");
    };
    Ok(config)
}
