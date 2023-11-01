// SPDX-FileCopyrightText: 2023 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

pub use futures;
pub use tokio;
pub use tokio_rustls;
pub use tokio_stream;
pub use tokio_util;

pub use rustls;
pub use rustls_pemfile;
pub use webpki_roots;

pub use cfg_if;

pub use clap;

pub use color_eyre;

pub use tracing;
pub use tracing_error;
pub use tracing_subscriber;

pub use opentelemetry;

pub use base64;

pub use clearscreen;
pub use indicatif;
pub use owo_colors;
pub use rpassword;

pub use sys_info;

pub use url;

pub use serde;
pub use serde_json;
pub use serde_yaml;

pub use bytes;

pub use simdutf8;

#[cfg(feature = "maildir")]
pub use maildir;
pub use mailparse;

pub use async_trait;

pub use secrecy;

pub use argon2;

pub use once_cell;

pub use rand_core;

pub use axum;
pub use axum_opentelemetry_middleware;
pub use axum_server;
pub use mime;
pub use tower_http;

pub use yaque;

pub use mail_auth;

pub use trust_dns_resolver;

pub use uuid;

pub use nom;

pub use reqwest;

pub use const_format;
