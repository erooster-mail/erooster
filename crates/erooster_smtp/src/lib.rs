// SPDX-FileCopyrightText: 2023 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

//! Erooster SMTP Mail Server
//!
//! Erooster is a rust native imap server build on modern solutions.
//! The goal being easy to setup, use and maintain for smaller mail servers
//! while being also fast and efficient.
//!
//! This crate is containing the smtp logic of the erooster mail server.
//!
#![allow(
    clippy::missing_panics_doc,
    clippy::missing_errors_doc,
    clippy::module_name_repetitions,
    // This seems to be buggy with instrument macros
    clippy::panic_in_result_fn
)]

pub(crate) mod commands;
/// The core server logic for the smtpserver.
/// This is the tls and non tls variant of the smtpserver.
pub mod servers;
pub(crate) mod utils;
