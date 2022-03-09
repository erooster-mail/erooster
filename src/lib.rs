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
    clippy::todo,
    clippy::fallible_impl_from,
    clippy::expl_impl_clone_on_copy,
    clippy::filetype_is_file,
    clippy::flat_map_option,
    clippy::float_cmp,
    clippy::suboptimal_flops,
    clippy::unused_self,
    clippy::unused_async,
    clippy::fn_to_numeric_cast_any,
    clippy::if_then_some_else_none,
    clippy::implicit_hasher,
    clippy::implicit_saturating_sub,
    clippy::imprecise_flops,
    clippy::lossy_float_literal,
    clippy::map_unwrap_or,
    clippy::panic_in_result_fn
)]
#![warn(missing_docs)]
#![allow(clippy::missing_panics_doc)]

mod commands;
mod line_codec;
mod state;

/// The core server logic for the imap server.
/// This is the tls and non tls variant of the server.
pub mod servers;
