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
    clippy::cloned_instead_of_copied,
    clippy::inefficient_to_string,
    clippy::macro_use_imports,
    clippy::mut_mut,
    clippy::todo
)]
#![warn(missing_docs)]
#![allow(clippy::missing_panics_doc)]

mod commands;
mod line_codec;
mod state;

/// The core server logic for the imap server.
/// This is the tls and non tls variant of the server.
pub mod servers;
