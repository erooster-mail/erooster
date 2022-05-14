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

    async fn verify_user(&self, username: &str, password: &str) -> bool {
        let hash: std::result::Result<(String,), sqlx::Error> =
            sqlx::query_as("SELECT hash FROM users WHERE username = ?")
                .bind(username)
                .fetch_one(self.get_pool())
                .await;

        // TODO hash password and compare
        return false;
    }

    async fn user_exists(&self, username: &str) -> bool {
        let exists: std::result::Result<(bool,), sqlx::Error> =
            sqlx::query_as("SELECT EXISTS(SELECT 1 FROM users WHERE username = ?)")
                .bind(username)
                .fetch_one(self.get_pool())
                .await;
        match exists {
            Ok(exists) => exists.0,
            Err(e) => {
                error!("[DB] Error checking if user exists: {}", e);
                false
            }
        }
    }
}
