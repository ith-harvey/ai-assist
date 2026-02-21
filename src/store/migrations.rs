//! Version-tracked database migrations for the libSQL backend.
//!
//! Each migration has a version number and SQL. `run_migrations()` checks
//! the current version and applies only the new ones sequentially.
//! On first run against a legacy DB (tables exist, no `_migrations` table),
//! it detects the existing schema and seeds V1 without re-creating tables.

use libsql::Connection;

use crate::error::DatabaseError;

/// A single migration step.
struct Migration {
    version: i64,
    name: &'static str,
    sql: &'static str,
}

/// All migrations in order. Add new versions to the end.
static MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    name: "initial_schema",
    sql: r#"
            CREATE TABLE IF NOT EXISTS cards (
                id TEXT PRIMARY KEY,
                conversation_id TEXT NOT NULL,
                source_message TEXT NOT NULL,
                source_sender TEXT NOT NULL,
                suggested_reply TEXT NOT NULL,
                confidence REAL NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                channel TEXT NOT NULL,
                created_at TEXT NOT NULL,
                expires_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                message_id TEXT,
                reply_metadata TEXT,
                email_thread TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_cards_status ON cards(status);
            CREATE INDEX IF NOT EXISTS idx_cards_channel ON cards(channel);

            CREATE TABLE IF NOT EXISTS messages (
                id TEXT PRIMARY KEY,
                external_id TEXT NOT NULL UNIQUE,
                channel TEXT NOT NULL,
                sender TEXT NOT NULL,
                subject TEXT,
                content TEXT NOT NULL,
                received_at TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                replied_at TEXT,
                metadata TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_messages_status ON messages(status);
            CREATE INDEX IF NOT EXISTS idx_messages_channel ON messages(channel);
            CREATE INDEX IF NOT EXISTS idx_messages_external_id ON messages(external_id);

            CREATE TABLE IF NOT EXISTS conversations (
                id TEXT PRIMARY KEY,
                channel TEXT NOT NULL,
                user_id TEXT NOT NULL,
                thread_id TEXT,
                started_at TEXT NOT NULL DEFAULT (datetime('now')),
                last_activity TEXT NOT NULL DEFAULT (datetime('now')),
                metadata TEXT NOT NULL DEFAULT '{}'
            );
            CREATE INDEX IF NOT EXISTS idx_conversations_channel ON conversations(channel);
            CREATE INDEX IF NOT EXISTS idx_conversations_user ON conversations(user_id);
            CREATE INDEX IF NOT EXISTS idx_conversations_last_activity ON conversations(last_activity);

            CREATE TABLE IF NOT EXISTS conversation_messages (
                id TEXT PRIMARY KEY,
                conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE INDEX IF NOT EXISTS idx_conversation_messages_conversation
                ON conversation_messages(conversation_id);
        "#,
}];

/// Run all pending migrations against the given connection.
///
/// Creates the `_migrations` table if it doesn't exist.
/// Detects legacy databases (tables exist but no `_migrations` table) and
/// seeds V1 without re-running schema DDL.
pub async fn run_migrations(conn: &Connection) -> Result<(), DatabaseError> {
    // Create migrations tracking table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS _migrations (
            version INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            applied_at TEXT NOT NULL DEFAULT (datetime('now'))
        )",
        (),
    )
    .await
    .map_err(|e| DatabaseError::Migration(format!("Failed to create _migrations table: {e}")))?;

    // Get the current max version
    let current_version = get_current_version(conn).await?;

    // Detect legacy DB: cards table exists but _migrations is empty
    if current_version == 0 && legacy_tables_exist(conn).await? {
        // Legacy DB — seed V1 without running DDL (tables already exist).
        // But we still need to create conversation tables that didn't exist in legacy.
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS conversations (
                id TEXT PRIMARY KEY,
                channel TEXT NOT NULL,
                user_id TEXT NOT NULL,
                thread_id TEXT,
                started_at TEXT NOT NULL DEFAULT (datetime('now')),
                last_activity TEXT NOT NULL DEFAULT (datetime('now')),
                metadata TEXT NOT NULL DEFAULT '{}'
            );
            CREATE INDEX IF NOT EXISTS idx_conversations_channel ON conversations(channel);
            CREATE INDEX IF NOT EXISTS idx_conversations_user ON conversations(user_id);
            CREATE INDEX IF NOT EXISTS idx_conversations_last_activity ON conversations(last_activity);

            CREATE TABLE IF NOT EXISTS conversation_messages (
                id TEXT PRIMARY KEY,
                conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE INDEX IF NOT EXISTS idx_conversation_messages_conversation
                ON conversation_messages(conversation_id);",
        )
        .await
        .map_err(|e| {
            DatabaseError::Migration(format!("Failed to create conversation tables on legacy DB: {e}"))
        })?;

        // Idempotent column additions for legacy DBs that may lack newer columns
        let _ = conn
            .execute("ALTER TABLE cards ADD COLUMN reply_metadata TEXT", ())
            .await;
        let _ = conn
            .execute("ALTER TABLE cards ADD COLUMN email_thread TEXT", ())
            .await;

        seed_version(conn, 1, "initial_schema").await?;
        tracing::info!("Legacy database detected — seeded migration V1");
        return Ok(());
    }

    // Apply pending migrations
    for migration in MIGRATIONS {
        if migration.version > current_version {
            tracing::info!(
                version = migration.version,
                name = migration.name,
                "Applying migration"
            );
            conn.execute_batch(migration.sql).await.map_err(|e| {
                DatabaseError::Migration(format!(
                    "Migration V{} ({}) failed: {e}",
                    migration.version, migration.name
                ))
            })?;
            seed_version(conn, migration.version, migration.name).await?;
        }
    }

    tracing::info!("Database migrations complete (at V{})", {
        let v = get_current_version(conn).await?;
        if v == 0 {
            MIGRATIONS.last().map(|m| m.version).unwrap_or(0)
        } else {
            v
        }
    });

    Ok(())
}

/// Get the highest applied migration version, or 0 if none.
async fn get_current_version(conn: &Connection) -> Result<i64, DatabaseError> {
    let mut rows = conn
        .query("SELECT COALESCE(MAX(version), 0) FROM _migrations", ())
        .await
        .map_err(|e| DatabaseError::Migration(format!("Failed to query migration version: {e}")))?;

    let row = rows
        .next()
        .await
        .map_err(|e| DatabaseError::Migration(format!("Failed to read migration version: {e}")))?;

    match row {
        Some(row) => {
            let version: i64 = row.get(0).map_err(|e| {
                DatabaseError::Migration(format!("Failed to parse migration version: {e}"))
            })?;
            Ok(version)
        }
        None => Ok(0),
    }
}

/// Check if legacy tables (cards, messages) already exist.
async fn legacy_tables_exist(conn: &Connection) -> Result<bool, DatabaseError> {
    let mut rows = conn
        .query(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='cards'",
            (),
        )
        .await
        .map_err(|e| DatabaseError::Query(format!("Failed to check legacy tables: {e}")))?;

    let row = rows
        .next()
        .await
        .map_err(|e| DatabaseError::Query(format!("Failed to read legacy check: {e}")))?;

    match row {
        Some(row) => {
            let count: i64 = row.get(0).unwrap_or(0);
            Ok(count > 0)
        }
        None => Ok(false),
    }
}

/// Insert a version record into `_migrations`.
async fn seed_version(conn: &Connection, version: i64, name: &str) -> Result<(), DatabaseError> {
    conn.execute(
        "INSERT OR IGNORE INTO _migrations (version, name) VALUES (?1, ?2)",
        libsql::params![version, name],
    )
    .await
    .map_err(|e| DatabaseError::Migration(format!("Failed to record migration V{version}: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_conn() -> Connection {
        let db = libsql::Builder::new_local(":memory:")
            .build()
            .await
            .unwrap();
        db.connect().unwrap()
    }

    #[tokio::test]
    async fn migrations_create_all_tables() {
        let conn = test_conn().await;
        run_migrations(&conn).await.unwrap();

        // Check all tables exist
        for table in &[
            "cards",
            "messages",
            "conversations",
            "conversation_messages",
            "_migrations",
        ] {
            let mut rows = conn
                .query(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    libsql::params![*table],
                )
                .await
                .unwrap();
            let row = rows.next().await.unwrap().unwrap();
            let count: i64 = row.get(0).unwrap();
            assert_eq!(count, 1, "Table '{}' should exist", table);
        }
    }

    #[tokio::test]
    async fn migrations_are_idempotent() {
        let conn = test_conn().await;
        run_migrations(&conn).await.unwrap();
        // Running again should not fail
        run_migrations(&conn).await.unwrap();

        // Version should still be 1
        let version = get_current_version(&conn).await.unwrap();
        assert_eq!(version, 1);
    }

    #[tokio::test]
    async fn legacy_db_detection() {
        let conn = test_conn().await;

        // Simulate a legacy DB: create cards + messages tables manually (no _migrations)
        conn.execute_batch(
            "CREATE TABLE cards (
                id TEXT PRIMARY KEY,
                conversation_id TEXT NOT NULL,
                source_message TEXT NOT NULL,
                source_sender TEXT NOT NULL,
                suggested_reply TEXT NOT NULL,
                confidence REAL NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                channel TEXT NOT NULL,
                created_at TEXT NOT NULL,
                expires_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                message_id TEXT
            );
            CREATE TABLE messages (
                id TEXT PRIMARY KEY,
                external_id TEXT NOT NULL UNIQUE,
                channel TEXT NOT NULL,
                sender TEXT NOT NULL,
                subject TEXT,
                content TEXT NOT NULL,
                received_at TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                replied_at TEXT,
                metadata TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );",
        )
        .await
        .unwrap();

        // Now run migrations — should detect legacy and seed V1
        run_migrations(&conn).await.unwrap();

        // Verify V1 was seeded
        let version = get_current_version(&conn).await.unwrap();
        assert_eq!(version, 1);

        // Verify conversation tables were created
        let mut rows = conn
            .query(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='conversations'",
                (),
            )
            .await
            .unwrap();
        let row = rows.next().await.unwrap().unwrap();
        let count: i64 = row.get(0).unwrap();
        assert_eq!(count, 1);

        // Verify new columns were added to legacy cards table
        conn.execute(
            "INSERT INTO cards (id, conversation_id, source_message, source_sender, suggested_reply, confidence, status, channel, created_at, expires_at, updated_at, reply_metadata, email_thread) VALUES ('t1', 'c', 'm', 's', 'r', 0.9, 'pending', 'test', '2026-01-01', '2026-01-02', '2026-01-01', '{\"x\":1}', '[]')",
            (),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn version_tracking() {
        let conn = test_conn().await;
        run_migrations(&conn).await.unwrap();

        let mut rows = conn
            .query("SELECT version, name FROM _migrations ORDER BY version", ())
            .await
            .unwrap();
        let row = rows.next().await.unwrap().unwrap();
        let version: i64 = row.get(0).unwrap();
        let name: String = row.get(1).unwrap();
        assert_eq!(version, 1);
        assert_eq!(name, "initial_schema");
    }
}
