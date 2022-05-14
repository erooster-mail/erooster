use crate::config::Config;
use sqlx::Pool;
use std::sync::Arc;

/// Postgres specific database implementation
#[cfg(feature = "postgres")]
pub mod postgres;

/// Sqlite specific database implementation
#[cfg(feature = "sqlite")]
pub mod sqlite;

/// Wrapper to simplify the Database types
#[cfg(feature = "postgres")]
pub type DB = Arc<postgres::Postgres>;

/// Wrapper to simplify the Database types
#[cfg(feature = "sqlite")]
pub type DB = Arc<sqlite::Sqlite>;

/// A uniform interface for database access
#[async_trait::async_trait]
pub trait Database<S: sqlx::Database> {
    /// Creates the new database connection pool
    fn new(config: Arc<Config>) -> Self
    where
        Self: Sized;

    /// Returns the database connection pool
    fn get_pool(&self) -> &Pool<S>;

    /// Checks if the user and password are correct
    async fn verify_user(&self, username: &str, password: &str) -> bool;

    /// Checks if the user exists
    async fn user_exists(&self, username: &str) -> bool;

    /// Saves a users password
    async fn change_password(&self, username: &str, password: &str)
        -> color_eyre::eyre::Result<()>;

    /// Adds a new user without password
    async fn add_user(&self, username: &str) -> color_eyre::eyre::Result<()>;
}

#[cfg(feature = "postgres")]
#[allow(clippy::module_name_repetitions)]
#[must_use]
/// Get a postgres database connection pool and the higher level wrapper
pub fn get_database(config: Arc<Config>) -> postgres::Postgres {
    postgres::Postgres::new(config)
}

#[cfg(feature = "sqlite")]
#[allow(clippy::module_name_repetitions)]
#[must_use]
/// Get a sqlite database connection pool and the higher level wrapper
pub fn get_database(config: Arc<Config>) -> sqlite::Sqlite {
    sqlite::Sqlite::new(config)
}
