use crate::{backend::database::Database, config::Config};
use argon2::{password_hash::SaltString, Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use color_eyre::Result;
use rand_core::OsRng;
use sqlx::PgPool;
use std::sync::Arc;
use tracing::{debug, error, instrument};

/// Postgres specific database implementation
/// Holds data to connect to the database
pub struct Postgres {
    pool: PgPool,
}

#[async_trait::async_trait]
impl Database<sqlx::Postgres> for Postgres {
    #[instrument(skip(config))]
    async fn new(config: Arc<Config>) -> Result<Self> {
        let pool = PgPool::connect_lazy(&config.database.postgres_url)
            .expect("Failed to connect to postgres");

        sqlx::migrate!().run(&pool).await?;
        Ok(Self { pool })
    }

    #[instrument(skip(self))]
    fn get_pool(&self) -> &PgPool {
        &self.pool
    }

    #[instrument(skip(self, username, password))]
    async fn verify_user(&self, username: &str, password: &str) -> bool {
        let hash: std::result::Result<(String,), sqlx::Error> =
            sqlx::query_as("SELECT hash FROM users WHERE username = $1")
                .bind(username)
                .fetch_one(self.get_pool())
                .await;

        match hash {
            Ok(hash) => {
                let hash = hash.0;
                match PasswordHash::new(&hash) {
                    Ok(parsed_hash) => Argon2::default()
                        .verify_password(password.as_bytes(), &parsed_hash)
                        .is_ok(),
                    Err(e) => {
                        error!("[DB] Error verifying user: {}", e);
                        false
                    }
                }
            }
            Err(e) => {
                error!("[DB] Error verifying user: {}", e);
                false
            }
        }
    }

    #[instrument(skip(self, username, password))]
    async fn change_password(
        &self,
        username: &str,
        password: &str,
    ) -> color_eyre::eyre::Result<()> {
        let salt = SaltString::generate(&mut OsRng);
        let argon2 = Argon2::default();
        let password_hash = argon2
            .hash_password(password.as_bytes(), &salt)?
            .to_string();
        sqlx::query("UPDATE users SET hash = $1 WHERE username = $2")
            .bind(password_hash)
            .bind(username)
            .execute(self.get_pool())
            .await?;
        Ok(())
    }

    #[instrument(skip(self, username))]
    async fn add_user(&self, username: &str) -> color_eyre::eyre::Result<()> {
        sqlx::query("INSERT INTO users (username) VALUES ($1)")
            .bind(username)
            .execute(self.get_pool())
            .await?;
        Ok(())
    }

    #[instrument(skip(self, username))]
    async fn user_exists(&self, username: &str) -> bool {
        let exists: std::result::Result<bool, sqlx::Error> =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE username = $1)")
                .bind(username)
                .fetch_one(self.get_pool())
                .await;
        debug!("[POSTGRES] [user_exists] {:?}", exists);
        match exists {
            Ok(exists) => exists,
            Err(e) => {
                if !matches!(e, sqlx::Error::RowNotFound) {
                    error!("[DB] Error checking if user exists: {}", e);
                }
                false
            }
        }
    }
}
