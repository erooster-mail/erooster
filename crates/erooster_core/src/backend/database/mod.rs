// SPDX-FileCopyrightText: 2023 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

use crate::config::Config;
use erooster_deps::{
    async_trait,
    color_eyre::{self, Result},
    secrecy::SecretString,
    tracing::{self, instrument},
};
use sqlx::Pool;

/// Postgres specific database implementation
pub mod postgres;

/// Wrapper to simplify the Database types
pub type DB = postgres::Postgres;

/// A uniform interface for database access
#[async_trait::async_trait]
pub trait Database<S: sqlx::Database> {
    /// Creates the new database connection pool
    async fn new(config: &Config) -> Result<Self>
    where
        Self: Sized;

    /// Returns the database connection pool
    fn get_pool(&self) -> &Pool<S>;

    /// Checks if the user and password are correct
    async fn verify_user(&self, username: &str, password: SecretString) -> bool;

    /// Checks if the user exists
    async fn user_exists(&self, username: &str) -> bool;

    /// Saves a users password
    async fn change_password(
        &self,
        username: &str,
        password: SecretString,
    ) -> color_eyre::eyre::Result<()>;

    /// Adds a new user without password
    async fn add_user(&self, username: &str) -> color_eyre::eyre::Result<()>;
}

/// Get a postgres database connection pool and the higher level wrapper
#[allow(clippy::module_name_repetitions)]
#[instrument(skip(config))]
pub async fn get_database(config: &Config) -> Result<postgres::Postgres> {
    postgres::Postgres::new(config).await
}
