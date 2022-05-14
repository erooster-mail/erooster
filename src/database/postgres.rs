use std::sync::Arc;

use crate::{config::Config, database::Database};
use sqlx::PgPool;
use tracing::error;

/// Postgres specific database implementation
/// Holds data to connect to the database
pub struct Postgres {
    pool: PgPool,
}

#[async_trait::async_trait]
impl Database<sqlx::Postgres> for Postgres {
    fn new(config: Arc<Config>) -> Self {
        let pool = PgPool::connect_lazy(&config.database.postgres_url)
            .expect("Failed to connect to postgres");
        Self { pool }
    }

    fn get_pool(&self) -> &PgPool {
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
