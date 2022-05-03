use std::sync::Arc;

use crate::config::Config;
use sqlx::Pool;

/// Postgres specific database implementation
#[cfg(feature = "postgres")]
pub mod postgres;

/// Sqlite specific database implementation
#[cfg(feature = "sqlite")]
pub mod sqlite;

/// A uniform interface for database access
pub trait Database<S: sqlx::Database> {
    /// Creates the new database connection pool
    fn new(config: Arc<Config>) -> Self
    where
        Self: Sized;

    /// Returns the database connection pool
    fn get_pool(&self) -> &Pool<S>;
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
