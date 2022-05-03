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
#![allow(clippy::missing_panics_doc)]

pub(crate) mod imap_commands;
// TODO make this only pub for benches and tests
#[allow(missing_docs)]
pub mod line_codec;
pub(crate) mod smtp_commands;

/// The core server logic for the imapserver.
/// This is the tls and non tls variant of the imapserver.
pub mod imap_servers;

/// The core server logic for the smtpserver.
/// This is the tls and non tls variant of the smtpserver.
pub mod smtp_servers;

/// The configuration file for the server
pub mod config;
