use std::path::Path;

use parking_lot::Mutex;
use rusqlite::Connection;

use crate::Result;

#[derive(Debug)]
pub struct Store {
    conn: Mutex<Connection>,
}

/// Migration array: (version, sql)
/// Version 1 = current schema (for existing DBs created with old migrate())
/// New versions get appended here
const MIGRATIONS: &[(u32, &str)] = &[(
    1,
    r#"
        CREATE TABLE IF NOT EXISTS sessions (
            id          TEXT PRIMARY KEY,
            profile     TEXT NOT NULL,
            started_at  TEXT NOT NULL,
            ended_at    TEXT,
            duration_ms INTEGER NOT NULL,
            hard_mode   INTEGER NOT NULL DEFAULT 0,
            state       TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_sessions_started_at ON sessions(started_at);
    "#,
)];

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs_err::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let store = Self { conn: Mutex::new(conn) };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<()> {
        let mut conn = self.conn.lock();
        let current_version: u32 =
            conn.pragma_query_value(None, "user_version", |row| row.get(0))?;

        for &(version, sql) in MIGRATIONS {
            if version <= current_version {
                continue;
            }

            tracing::info!(version, "applying migration");
            let tx = conn.transaction()?;
            tx.execute_batch(sql)?;
            tx.pragma_update(None, "user_version", version)?;
            tx.commit()?;
            tracing::info!(version, "migration completed");
        }

        Ok(())
    }

    pub fn execute<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let conn = self.conn.lock();
        f(&conn)
    }

    pub fn transaction<T>(&self, f: impl FnOnce(&rusqlite::Transaction) -> Result<T>) -> Result<T> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        let result = f(&tx)?;
        tx.commit()?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrate_fresh_db() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let db_path = temp_dir.path().join("test.sqlite3");
        let store = Store::open(&db_path).expect("store open");

        // Check that migrations ran and version is set to latest
        let version: u32 = store
            .execute(|conn| Ok(conn.pragma_query_value(None, "user_version", |row| row.get(0))?))
            .expect("version query");

        let latest_version = MIGRATIONS.last().map(|(v, _)| *v).unwrap_or(0);
        assert_eq!(version, latest_version);

        // Check that sessions table exists
        store
            .execute(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT name FROM sqlite_master WHERE type='table' AND name='sessions'",
                )?;
                let tables: Vec<String> = stmt
                    .query_map([], |row| row.get::<_, String>(0))?
                    .collect::<Result<Vec<_>, _>>()?;
                assert_eq!(tables.len(), 1);
                assert_eq!(tables[0], "sessions");
                Ok(())
            })
            .expect("table check");
    }

    #[test]
    fn migrate_idempotent() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let db_path = temp_dir.path().join("test.sqlite3");

        // First migration
        let store1 = Store::open(&db_path).expect("store open 1");
        let version1: u32 = store1
            .execute(|conn| Ok(conn.pragma_query_value(None, "user_version", |row| row.get(0))?))
            .expect("version query 1");

        drop(store1);

        // Second migration on same DB
        let store2 = Store::open(&db_path).expect("store open 2");
        let version2: u32 = store2
            .execute(|conn| Ok(conn.pragma_query_value(None, "user_version", |row| row.get(0))?))
            .expect("version query 2");

        assert_eq!(version1, version2);

        // Ensure we can still operate
        store2.execute(|conn| {
            conn.execute("INSERT INTO sessions (id, profile, started_at, duration_ms, state) VALUES (?, ?, ?, ?, ?)",
                rusqlite::params!["test-id", "test", "2024-01-01T00:00:00Z", 1000, "running"])?;
            Ok(())
        }).expect("insert test");

        let count: i64 = store2
            .execute(|conn| {
                Ok(conn.query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))?)
            })
            .expect("count query");
        assert_eq!(count, 1);
    }
}
