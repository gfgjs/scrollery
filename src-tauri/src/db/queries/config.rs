//! App config KV DAO(T 线拆分自 queries.rs,SQL 与行为不变)。

use rusqlite::{params, Connection, OptionalExtension};

use crate::error::{AppError, Result};

pub fn get_config(conn: &Connection, key: &str) -> Result<Option<String>> {
    conn.query_row(
        "SELECT value FROM app_config WHERE key=?1",
        params![key],
        |row| row.get(0),
    )
    .optional()
    .map_err(AppError::from)
}

pub fn set_config(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO app_config (key, value) VALUES (?1, ?2)",
        params![key, value],
    )?;
    Ok(())
}
