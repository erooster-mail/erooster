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
        Self {  pool }
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
                error!("[DB]Error verifying user: {}", e);
                false
            }
        }
    }

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
        sqlx::query("UPDATE users SET hash = ? WHERE username = ?")
            .bind(password_hash)
            .bind(username)
            .execute(self.get_pool())
            .await?;
        Ok(())
    }

    async fn add_user(&self, username: &str) -> color_eyre::eyre::Result<()> {
        sqlx::query("INSERT INTO users (username, hash) VALUES (?, NULL)")
            .bind(username)
            .execute(self.get_pool())
            .await?;
        Ok(())
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
