use std::sync::Arc;

use crate::{config::Config, database::Database};
use sqlx::SqlitePool;

/// Sqlite specific database implementation
/// Holds data to connect to the database
pub struct Sqlite {
    pool: SqlitePool,
}

impl Database<sqlx::Sqlite> for Sqlite {
    fn new(config: Arc<Config>) -> Self {
        let pool = SqlitePool::connect_lazy(&config.database.sqlite_path)
            .expect("Failed to connect to sqlite");
        Self { pool }
    }

    fn get_pool(&self) -> &SqlitePool {
        &self.pool
    }
}
