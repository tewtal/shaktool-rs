use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};

use crate::Error;

/// Sentinel guild id under which global (bot-wide) settings are stored.
const GLOBAL_GUILD: i64 = 0;

/// Simple persistent key-value store backed by SQLite.
///
/// Two tables are provided:
/// - `settings`: user-facing configuration, scoped by feature and either
///   per-guild or global (managed via the `config` command)
/// - `task_state`: internal persistence for background tasks (seen items,
///   cached lookups, etc.)
#[derive(Clone)]
pub struct Db {
    pool: SqlitePool,
}

impl Db {
    pub async fn connect(path: &str) -> Result<Self, Error> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal);

        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(options)
            .await?;

        migrate_settings(&pool).await?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS task_state (
                task TEXT NOT NULL,
                key TEXT NOT NULL,
                value TEXT NOT NULL,
                PRIMARY KEY (task, key)
            )",
        )
        .execute(&pool)
        .await?;

        Ok(Db { pool })
    }

    pub async fn get_global_setting(&self, scope: &str, key: &str) -> Result<Option<String>, Error> {
        self.get(GLOBAL_GUILD, scope, key).await
    }

    pub async fn set_global_setting(&self, scope: &str, key: &str, value: &str) -> Result<(), Error> {
        self.set(GLOBAL_GUILD, scope, key, value).await
    }

    pub async fn delete_global_setting(&self, scope: &str, key: &str) -> Result<bool, Error> {
        self.delete(GLOBAL_GUILD, scope, key).await
    }

    pub async fn list_global_settings(&self, scope: &str) -> Result<Vec<(String, String)>, Error> {
        self.list(GLOBAL_GUILD, scope).await
    }

    pub async fn get_guild_setting(&self, guild_id: u64, scope: &str, key: &str) -> Result<Option<String>, Error> {
        self.get(guild_id as i64, scope, key).await
    }

    pub async fn set_guild_setting(&self, guild_id: u64, scope: &str, key: &str, value: &str) -> Result<(), Error> {
        self.set(guild_id as i64, scope, key, value).await
    }

    pub async fn delete_guild_setting(&self, guild_id: u64, scope: &str, key: &str) -> Result<bool, Error> {
        self.delete(guild_id as i64, scope, key).await
    }

    pub async fn list_guild_settings(&self, guild_id: u64, scope: &str) -> Result<Vec<(String, String)>, Error> {
        self.list(guild_id as i64, scope).await
    }

    /// Returns `(guild_id, value)` for every guild that has the setting.
    pub async fn guild_setting_values(&self, scope: &str, key: &str) -> Result<Vec<(u64, String)>, Error> {
        let rows = sqlx::query(
            "SELECT guild_id, value FROM settings WHERE scope = ? AND key = ? AND guild_id != ?",
        )
        .bind(scope)
        .bind(key)
        .bind(GLOBAL_GUILD)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| (r.get::<i64, _>("guild_id") as u64, r.get("value")))
            .collect())
    }

    async fn get(&self, guild_id: i64, scope: &str, key: &str) -> Result<Option<String>, Error> {
        let row = sqlx::query("SELECT value FROM settings WHERE guild_id = ? AND scope = ? AND key = ?")
            .bind(guild_id)
            .bind(scope)
            .bind(key)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|r| r.get("value")))
    }

    async fn set(&self, guild_id: i64, scope: &str, key: &str, value: &str) -> Result<(), Error> {
        sqlx::query(
            "INSERT INTO settings (guild_id, scope, key, value) VALUES (?, ?, ?, ?)
             ON CONFLICT (guild_id, scope, key) DO UPDATE SET value = excluded.value",
        )
        .bind(guild_id)
        .bind(scope)
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn delete(&self, guild_id: i64, scope: &str, key: &str) -> Result<bool, Error> {
        let result = sqlx::query("DELETE FROM settings WHERE guild_id = ? AND scope = ? AND key = ?")
            .bind(guild_id)
            .bind(scope)
            .bind(key)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn list(&self, guild_id: i64, scope: &str) -> Result<Vec<(String, String)>, Error> {
        let rows = sqlx::query("SELECT key, value FROM settings WHERE guild_id = ? AND scope = ? ORDER BY key")
            .bind(guild_id)
            .bind(scope)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(|r| (r.get("key"), r.get("value"))).collect())
    }

    pub async fn get_task_state(&self, task: &str, key: &str) -> Result<Option<String>, Error> {
        let row = sqlx::query("SELECT value FROM task_state WHERE task = ? AND key = ?")
            .bind(task)
            .bind(key)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|r| r.get("value")))
    }

    /// Returns all `(key, value)` task state entries whose key starts with `prefix`.
    pub async fn list_task_state(&self, task: &str, prefix: &str) -> Result<Vec<(String, String)>, Error> {
        let rows = sqlx::query(
            "SELECT key, value FROM task_state WHERE task = ? AND key LIKE ? || '%' ORDER BY key",
        )
        .bind(task)
        .bind(prefix)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| (r.get("key"), r.get("value"))).collect())
    }

    /// Atomically removes and returns a task state entry. `None` means it
    /// didn't exist — e.g. a concurrent resolver claimed it first.
    pub async fn claim_task_state(&self, task: &str, key: &str) -> Result<Option<String>, Error> {
        let row = sqlx::query("DELETE FROM task_state WHERE task = ? AND key = ? RETURNING value")
            .bind(task)
            .bind(key)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|r| r.get("value")))
    }

    pub async fn delete_task_state(&self, task: &str, key: &str) -> Result<(), Error> {
        sqlx::query("DELETE FROM task_state WHERE task = ? AND key = ?")
            .bind(task)
            .bind(key)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn set_task_state(&self, task: &str, key: &str, value: &str) -> Result<(), Error> {
        sqlx::query(
            "INSERT INTO task_state (task, key, value) VALUES (?, ?, ?)
             ON CONFLICT (task, key) DO UPDATE SET value = excluded.value",
        )
        .bind(task)
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn claim_task_state_claims_exactly_once() {
        let path = std::env::temp_dir().join(format!("shaktool-test-{}.db", std::process::id()));
        let db = Db::connect(path.to_str().unwrap()).await.unwrap();

        db.set_task_state("test", "key", "value").await.unwrap();
        assert_eq!(db.claim_task_state("test", "key").await.unwrap(), Some("value".to_string()));
        assert_eq!(db.claim_task_state("test", "key").await.unwrap(), None);

        drop(db);
        let _ = std::fs::remove_file(&path);
    }
}

/// Creates the settings table, upgrading a pre-guild-id table by moving its
/// rows to the global guild.
async fn migrate_settings(pool: &SqlitePool) -> Result<(), Error> {
    let columns = sqlx::query("SELECT name FROM pragma_table_info('settings')")
        .fetch_all(pool)
        .await?;
    let table_exists = !columns.is_empty();
    let has_guild_id = columns.iter().any(|r| r.get::<String, _>("name") == "guild_id");

    if table_exists && !has_guild_id {
        sqlx::query("ALTER TABLE settings RENAME TO settings_v1").execute(pool).await?;
    }

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS settings (
            guild_id INTEGER NOT NULL,
            scope TEXT NOT NULL,
            key TEXT NOT NULL,
            value TEXT NOT NULL,
            PRIMARY KEY (guild_id, scope, key)
        )",
    )
    .execute(pool)
    .await?;

    if table_exists && !has_guild_id {
        sqlx::query(
            "INSERT INTO settings (guild_id, scope, key, value)
             SELECT ?, scope, key, value FROM settings_v1",
        )
        .bind(GLOBAL_GUILD)
        .execute(pool)
        .await?;
        sqlx::query("DROP TABLE settings_v1").execute(pool).await?;
    }

    Ok(())
}
