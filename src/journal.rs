use anyhow::{Result, bail};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecutionEvent {
    pub seq: i64,
    pub execution_id: String,
    pub from_state: Option<String>,
    pub to_state: String,
    pub worker_id: Option<String>,
    pub result_hash: Option<String>,
    pub ts: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecutionSnapshot {
    pub id: String,
    pub payload: String,
    pub state: String,
    pub worker_id: Option<String>,
    pub result_hash: Option<String>,
    pub updated_at: i64,
}

pub struct Journal {
    conn: Connection,
}

impl Journal {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS executions (
                id TEXT PRIMARY KEY,
                payload TEXT NOT NULL,
                state TEXT NOT NULL,
                worker_id TEXT,
                result_hash TEXT,
                updated_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS events (
                seq INTEGER PRIMARY KEY AUTOINCREMENT,
                execution_id TEXT NOT NULL,
                from_state TEXT,
                to_state TEXT NOT NULL,
                worker_id TEXT,
                result_hash TEXT,
                ts INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_events_execution_seq
                ON events(execution_id, seq);
            ",
        )?;

        // v0.1 databases did not persist result_hash on each event.
        let has_result_hash = {
            let mut stmt = conn.prepare("PRAGMA table_info(events)")?;
            let columns = stmt.query_map([], |row| row.get::<_, String>(1))?;
            let mut found = false;
            for column in columns {
                if column? == "result_hash" {
                    found = true;
                    break;
                }
            }
            found
        };
        if !has_result_hash {
            conn.execute("ALTER TABLE events ADD COLUMN result_hash TEXT", [])?;
        }

        Ok(Self { conn })
    }

    pub fn create(&self, execution_id: &str, payload: &str) -> Result<()> {
        let now = now_unix();
        self.conn.execute(
            "INSERT INTO executions (id, payload, state, updated_at) VALUES (?1, ?2, 'Created', ?3)",
            params![execution_id, payload, now],
        )?;
        self.conn.execute(
            "INSERT INTO events (execution_id, from_state, to_state, ts) VALUES (?1, NULL, 'Created', ?2)",
            params![execution_id, now],
        )?;
        Ok(())
    }

    pub fn transition(
        &self,
        execution_id: &str,
        from: &str,
        to: &str,
        worker_id: Option<&str>,
        result_hash: Option<&str>,
    ) -> Result<()> {
        let now = now_unix();
        self.conn.execute(
            "UPDATE executions
             SET state=?2, worker_id=COALESCE(?3, worker_id), result_hash=COALESCE(?4, result_hash), updated_at=?5
             WHERE id=?1 AND state=?6",
            params![execution_id, to, worker_id, result_hash, now, from],
        )?;
        if self.conn.changes() != 1 {
            bail!("invalid transition for {execution_id}: expected current state {from}");
        }
        self.conn.execute(
            "INSERT INTO events (execution_id, from_state, to_state, worker_id, result_hash, ts)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![execution_id, from, to, worker_id, result_hash, now],
        )?;
        Ok(())
    }

    pub fn completed_count(&self) -> Result<u64> {
        let value: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM executions WHERE state='Completed'",
            [],
            |row| row.get(0),
        )?;
        Ok(value as u64)
    }

    pub fn execution_ids(&self) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM executions ORDER BY rowid")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn snapshot(&self, execution_id: &str) -> Result<Option<ExecutionSnapshot>> {
        self.conn
            .query_row(
                "SELECT id, payload, state, worker_id, result_hash, updated_at FROM executions WHERE id=?1",
                params![execution_id],
                |row| {
                    Ok(ExecutionSnapshot {
                        id: row.get(0)?,
                        payload: row.get(1)?,
                        state: row.get(2)?,
                        worker_id: row.get(3)?,
                        result_hash: row.get(4)?,
                        updated_at: row.get(5)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn events_for_execution(&self, execution_id: &str) -> Result<Vec<ExecutionEvent>> {
        let mut stmt = self.conn.prepare(
            "SELECT seq, execution_id, from_state, to_state, worker_id, result_hash, ts
             FROM events WHERE execution_id=?1 ORDER BY seq ASC",
        )?;
        let rows = stmt.query_map(params![execution_id], |row| {
            Ok(ExecutionEvent {
                seq: row.get(0)?,
                execution_id: row.get(1)?,
                from_state: row.get(2)?,
                to_state: row.get(3)?,
                worker_id: row.get(4)?,
                result_hash: row.get(5)?,
                ts: row.get(6)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
