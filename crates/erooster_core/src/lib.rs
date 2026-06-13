// SPDX-FileCopyrightText: 2023 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

//! Core logic for the erooster mail server
//!
#![allow(
    clippy::missing_panics_doc,
    clippy::missing_errors_doc,
    clippy::module_name_repetitions,
    // This seems to be buggy with instrument macros
    clippy::panic_in_result_fn
)]

use std::path::{Path, PathBuf};

use {
    base64::{
        alphabet,
        engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig},
    },
    color_eyre::{self, Result},
    tracing::{error, instrument},
};

// TODO make this only pub for benches and tests
#[allow(missing_docs)]
pub mod line_codec;
/// An custom panic handler for erooster
pub mod panic_handler;

/// The backend logic of the server
pub mod backend;

/// Helpers for writing isolated unit tests with per-test SQLite storage.
#[cfg(feature = "test-helpers")]
pub mod test_helpers;

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
    } else if let Some(path) = find_config_in_ancestors() {
        config::Config::load(path).await?
    } else {
        error!("No config file found. Please follow the readme.");
        color_eyre::eyre::bail!("No config file found");
    };
    Ok(config)
}

/// Walks ancestor directories from `CARGO_MANIFEST_DIR` (set by Cargo during tests)
/// looking for a `config.yml` or `config.yaml` file. Returns the first match.
fn find_config_in_ancestors() -> Option<PathBuf> {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").ok()?;
    let mut dir = PathBuf::from(manifest_dir);
    loop {
        for name in ["config.yml", "config.yaml"] {
            let candidate = dir.join(name);
            if candidate.exists() {
                return Some(candidate);
            }
        }
        if !dir.pop() {
            return None;
        }
    }
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
