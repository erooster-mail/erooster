// SPDX-FileCopyrightText: 2023 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

use crate::{backend::database::Database, config::Config};
use erooster_deps::{
    argon2::{password_hash::SaltString, Argon2, PasswordHash, PasswordHasher, PasswordVerifier},
    async_trait,
    color_eyre::{self, Result},
    rand_core::OsRng,
    secrecy::{ExposeSecret, SecretString},
    tracing::{self, debug, debug_span, error, instrument},
};
use sqlx::{pool::PoolOptions, SqlitePool};

/// Postgres specific database implementation
/// Holds data to connect to the database
#[derive(Debug, Clone)]
pub struct Sqlite {
    pool: SqlitePool,
}

#[async_trait::async_trait]
impl Database<sqlx::Sqlite> for Sqlite {
    #[instrument(skip(config))]
    async fn new(config: &Config) -> Result<Self> {
        let pool = PoolOptions::new()
            .min_connections(2)
            .max_connections(10)
            .connect(&config.database.url)
            .await?;
        sqlx::migrate!("./sqlite_migrations").run(&pool).await?;
        Ok(Self { pool })
    }

    #[instrument(skip(self))]
    fn get_pool(&self) -> &SqlitePool {
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
                debug!("[SQLITE] [user_exists] {:?}", exists.len());
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
