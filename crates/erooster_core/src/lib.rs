// SPDX-FileCopyrightText: 2023 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

//! Core logic for the erooster mail server
//!
#![feature(string_remove_matches)]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]
#![deny(unsafe_code, clippy::unwrap_used)]
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
    clippy::module_name_repetitions,
    // This seems to be buggy with instrument macros
    clippy::panic_in_result_fn
)]

use std::path::Path;

use erooster_deps::{
    base64::{
        alphabet,
        engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig},
    },
    color_eyre::{self, Result},
    tracing::{self, error, instrument},
};

// TODO make this only pub for benches and tests
#[allow(missing_docs)]
pub mod line_codec;
/// An custom panic handler for erooster
pub mod panic_handler;

/// The backend logic of the server
pub mod backend;

/// The configuration file for the server
pub mod config;

/// Returns the config struct from the provided location or defaults
#[instrument(skip(config_path))]
pub async fn get_config(config_path: String) -> Result<config::Config> {
    let config = if Path::new(&config_path).exists() {
        config::Config::load(config_path).await?
    } else if Path::new("/etc/erooster/config.yml").exists() {
        config::Config::load("/etc/erooster/config.yml").await?
    } else if Path::new("/etc/erooster/config.yaml").exists() {
        config::Config::load("/etc/erooster/config.yaml").await?
    } else {
        error!("No config file found. Please follow the readme.");
        color_eyre::eyre::bail!("No config file found");
    };
    Ok(config)
}

/// The maximum size of a line in bytes
pub const LINE_LIMIT: usize = 8192;

/// A Base64 Decoder config that doesn't care about padding
pub const BASE64_DECODER_CONFIG: GeneralPurposeConfig = GeneralPurposeConfig::new()
    .with_encode_padding(true)
    .with_decode_padding_mode(DecodePaddingMode::Indifferent);

/// A Base64 Decoder config that doesn't care about padding
pub const BASE64_DECODER: GeneralPurpose =
    GeneralPurpose::new(&alphabet::STANDARD, BASE64_DECODER_CONFIG);
