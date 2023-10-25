use crate::{backend::database::Database, config::Config};
use argon2::{password_hash::SaltString, Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use color_eyre::Result;
use once_cell::sync::OnceCell;
use rand_core::OsRng;
use secrecy::{ExposeSecret, SecretString};
use sqlx::{pool::PoolOptions, PgPool};
use tracing::{debug, debug_span, error, instrument};

/// Postgres specific database implementation
/// Holds data to connect to the database
#[derive(Debug, Clone)]
pub struct Postgres {
    pool: PgPool,
}

#[async_trait::async_trait]
impl Database<sqlx::Postgres> for Postgres {
    #[instrument(skip(config))]
    async fn new(config: &Config) -> Result<Self> {
        static INSTANCE: OnceCell<Postgres> = OnceCell::new();

        let pool = INSTANCE.get();

        if let Some(pool) = pool {
            Ok(pool.clone())
        } else {
            let pool = PoolOptions::new()
                .min_connections(2)
                .max_connections(10)
                .connect(&config.database.postgres_url)
                .await?;
            sqlx::migrate!().run(&pool).await?;
            let new_self = Self { pool };
            INSTANCE.get_or_init(|| new_self.clone());
            Ok(new_self)
        }
    }

    #[instrument(skip(self))]
    fn get_pool(&self) -> &PgPool {
        &self.pool
    }

    #[instrument(skip(self, username, password))]
    async fn verify_user(&self, username: &str, password: SecretString) -> bool {
        let hash: std::result::Result<(String,), sqlx::Error> =
            sqlx::query_as("SELECT hash FROM users WHERE username = $1")
                .bind(username)
                .fetch_one(self.get_pool())
                .await;

        match hash {
            Ok(hash) => {
                let hash = hash.0;
                let span = debug_span!("verify_user::aron2::PasswordHash");
                span.in_scope(|| match PasswordHash::new(&hash) {
                    Ok(parsed_hash) => {
                        let span = debug_span!("verify_user::aron2::verify_password");
                        span.in_scope(|| {
                            Argon2::default()
                                .verify_password(password.expose_secret().as_bytes(), &parsed_hash)
                                .is_ok()
                        })
                    }
                    Err(e) => {
                        error!("[DB] Error verifying user: {}", e);
                        false
                    }
                })
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
        password: SecretString,
    ) -> color_eyre::eyre::Result<()> {
        let salt = SaltString::generate(&mut OsRng);
        let argon2 = Argon2::default();
        let password_hash = argon2
            .hash_password(password.expose_secret().as_bytes(), &salt)?
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
        let exists = sqlx::query("SELECT 1 FROM users WHERE username = $1")
            .bind(username)
            .fetch_all(self.get_pool())
            .await;
        match exists {
            Ok(exists) => {
                debug!("[POSTGRES] [user_exists] {:?}", exists.len());
                exists.len() == 1
            }
            Err(e) => {
                if !matches!(e, sqlx::Error::RowNotFound) {
                    error!("[DB] Error checking if user exists: {}", e);
                }
                false
            }
        }
    }
}
