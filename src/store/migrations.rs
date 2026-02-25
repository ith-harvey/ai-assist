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
static MIGRATIONS: &[Migration] = &[
    Migration {
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
    },
    Migration {
        version: 2,
        name: "routines_system",
        sql: r#"
            CREATE TABLE IF NOT EXISTS routines (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT NOT NULL DEFAULT '',
                user_id TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                trigger_type TEXT NOT NULL,
                trigger_config TEXT NOT NULL,
                action_type TEXT NOT NULL,
                action_config TEXT NOT NULL,
                cooldown_secs INTEGER NOT NULL DEFAULT 300,
                max_concurrent INTEGER NOT NULL DEFAULT 1,
                dedup_window_secs INTEGER,
                notify_channel TEXT,
                notify_user TEXT NOT NULL DEFAULT 'default',
                notify_on_success INTEGER NOT NULL DEFAULT 0,
                notify_on_failure INTEGER NOT NULL DEFAULT 1,
                notify_on_attention INTEGER NOT NULL DEFAULT 1,
                state TEXT NOT NULL DEFAULT '{}',
                last_run_at TEXT,
                next_fire_at TEXT,
                run_count INTEGER NOT NULL DEFAULT 0,
                consecutive_failures INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now')),
                UNIQUE (user_id, name)
            );
            CREATE INDEX IF NOT EXISTS idx_routines_user ON routines(user_id);
            CREATE INDEX IF NOT EXISTS idx_routines_next_fire ON routines(next_fire_at);

            CREATE TABLE IF NOT EXISTS routine_runs (
                id TEXT PRIMARY KEY,
                routine_id TEXT NOT NULL REFERENCES routines(id) ON DELETE CASCADE,
                trigger_type TEXT NOT NULL,
                trigger_detail TEXT,
                started_at TEXT NOT NULL DEFAULT (datetime('now')),
                completed_at TEXT,
                status TEXT NOT NULL DEFAULT 'running',
                result_summary TEXT,
                tokens_used INTEGER,
                job_id TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE INDEX IF NOT EXISTS idx_routine_runs_routine ON routine_runs(routine_id);
            CREATE INDEX IF NOT EXISTS idx_routine_runs_status ON routine_runs(status);

            CREATE TABLE IF NOT EXISTS settings (
                user_id TEXT NOT NULL,
                key TEXT NOT NULL,
                value TEXT NOT NULL,
                updated_at TEXT NOT NULL DEFAULT (datetime('now')),
                PRIMARY KEY (user_id, key)
            );
        "#,
    },
    Migration {
        version: 3,
        name: "llm_call_tracking",
        sql: r#"
            CREATE TABLE IF NOT EXISTS llm_calls (
                id TEXT PRIMARY KEY,
                conversation_id TEXT,
                routine_run_id TEXT,
                provider TEXT NOT NULL,
                model TEXT NOT NULL,
                input_tokens INTEGER NOT NULL,
                output_tokens INTEGER NOT NULL,
                cost TEXT NOT NULL,
                purpose TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE INDEX IF NOT EXISTS idx_llm_calls_conversation ON llm_calls(conversation_id);
            CREATE INDEX IF NOT EXISTS idx_llm_calls_provider ON llm_calls(provider);
            CREATE INDEX IF NOT EXISTS idx_llm_calls_created ON llm_calls(created_at);
        "#,
    },
    Migration {
        version: 4,
        name: "relax_llm_calls_fk",
        sql: r#"
            CREATE TABLE IF NOT EXISTS llm_calls_new (
                id TEXT PRIMARY KEY,
                conversation_id TEXT,
                routine_run_id TEXT,
                provider TEXT NOT NULL,
                model TEXT NOT NULL,
                input_tokens INTEGER NOT NULL,
                output_tokens INTEGER NOT NULL,
                cost TEXT NOT NULL,
                purpose TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            INSERT OR IGNORE INTO llm_calls_new SELECT * FROM llm_calls;
            DROP TABLE IF EXISTS llm_calls;
            ALTER TABLE llm_calls_new RENAME TO llm_calls;
            CREATE INDEX IF NOT EXISTS idx_llm_calls_conversation ON llm_calls(conversation_id);
            CREATE INDEX IF NOT EXISTS idx_llm_calls_provider ON llm_calls(provider);
            CREATE INDEX IF NOT EXISTS idx_llm_calls_created ON llm_calls(created_at);
        "#,
    },
    Migration {
        version: 5,
        name: "todos",
        sql: r#"
            CREATE TABLE IF NOT EXISTS todos (
                id TEXT PRIMARY KEY,
                user_id TEXT NOT NULL,
                title TEXT NOT NULL,
                description TEXT,
                todo_type TEXT NOT NULL,
                bucket TEXT NOT NULL DEFAULT 'human_only',
                status TEXT NOT NULL DEFAULT 'created',
                priority INTEGER NOT NULL DEFAULT 0,
                due_date TEXT,
                context TEXT,
                source_card_id TEXT,
                snoozed_until TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE INDEX IF NOT EXISTS idx_todos_status ON todos(status);
            CREATE INDEX IF NOT EXISTS idx_todos_priority ON todos(priority);
            CREATE INDEX IF NOT EXISTS idx_todos_due_date ON todos(due_date);
            CREATE INDEX IF NOT EXISTS idx_todos_todo_type ON todos(todo_type);
            CREATE INDEX IF NOT EXISTS idx_todos_user_id ON todos(user_id);
        "#,
    },
    Migration {
        version: 6,
        name: "card_type_silo_payload",
        sql: r#"
            ALTER TABLE cards ADD COLUMN card_type TEXT NOT NULL DEFAULT 'reply';
            ALTER TABLE cards ADD COLUMN silo TEXT NOT NULL DEFAULT 'messages';
            ALTER TABLE cards ADD COLUMN payload TEXT;
            CREATE INDEX IF NOT EXISTS idx_cards_silo ON cards(silo);
            CREATE INDEX IF NOT EXISTS idx_cards_card_type ON cards(card_type);

            UPDATE cards SET payload = json_object(
                'channel', channel,
                'source_sender', source_sender,
                'source_message', source_message,
                'suggested_reply', suggested_reply,
                'confidence', confidence,
                'conversation_id', conversation_id,
                'message_id', message_id,
                'reply_metadata', json(reply_metadata),
                'email_thread', json(email_thread)
            ) WHERE payload IS NULL;
        "#,
    },
];

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
        // Fall through to apply V2+ migrations
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

        // Check all tables exist (V1 + V2 + V3 + V5)
        for table in &[
            "cards",
            "messages",
            "conversations",
            "conversation_messages",
            "_migrations",
            "routines",
            "routine_runs",
            "settings",
            "llm_calls",
            "todos",
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

        // Version should be at the latest migration
        let version = get_current_version(&conn).await.unwrap();
        assert_eq!(version, 6);
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

        // Now run migrations — should detect legacy, seed V1, then apply V2+V3
        run_migrations(&conn).await.unwrap();

        // Verify all migrations applied (legacy seed V1 + V2 routines + V3 llm_calls + V5 todos)
        let version = get_current_version(&conn).await.unwrap();
        assert_eq!(version, 6);

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
        let row1 = rows.next().await.unwrap().unwrap();
        let v1: i64 = row1.get(0).unwrap();
        let n1: String = row1.get(1).unwrap();
        assert_eq!(v1, 1);
        assert_eq!(n1, "initial_schema");

        let row2 = rows.next().await.unwrap().unwrap();
        let v2: i64 = row2.get(0).unwrap();
        let n2: String = row2.get(1).unwrap();
        assert_eq!(v2, 2);
        assert_eq!(n2, "routines_system");

        let row3 = rows.next().await.unwrap().unwrap();
        let v3: i64 = row3.get(0).unwrap();
        let n3: String = row3.get(1).unwrap();
        assert_eq!(v3, 3);
        assert_eq!(n3, "llm_call_tracking");

        let row4 = rows.next().await.unwrap().unwrap();
        let v4: i64 = row4.get(0).unwrap();
        let n4: String = row4.get(1).unwrap();
        assert_eq!(v4, 4);
        assert_eq!(n4, "relax_llm_calls_fk");

        let row5 = rows.next().await.unwrap().unwrap();
        let v5: i64 = row5.get(0).unwrap();
        let n5: String = row5.get(1).unwrap();
        assert_eq!(v5, 5);
        assert_eq!(n5, "todos");

        let row6 = rows.next().await.unwrap().unwrap();
        let v6: i64 = row6.get(0).unwrap();
        let n6: String = row6.get(1).unwrap();
        assert_eq!(v6, 6);
        assert_eq!(n6, "card_type_silo_payload");
    }
}
