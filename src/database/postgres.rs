use std::sync::Arc;

use crate::{config::Config, database::Database};
use sqlx::PgPool;

/// Postgres specific database implementation
/// Holds data to connect to the database
pub struct Postgres {
    pool: PgPool,
}

impl Database<sqlx::Postgres> for Postgres {
    fn new(config: Arc<Config>) -> Self {
        let pool = PgPool::connect_lazy(&config.database.postgres_url)
            .expect("Failed to connect to postgres");
        Self { pool }
    }

    fn get_pool(&self) -> &PgPool {
        &self.pool
    }
}
