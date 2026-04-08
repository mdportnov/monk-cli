use std::path::Path;

use parking_lot::Mutex;
use rusqlite::Connection;

use crate::Result;

#[derive(Debug)]
pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs_err::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let store = Self { conn: Mutex::new(conn) };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute_batch(
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
        )?;
        Ok(())
    }
}
