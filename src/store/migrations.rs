//! Database schema initialization.
//!
//! Single `init_schema()` function creates all tables idempotently using
//! `CREATE TABLE IF NOT EXISTS`. No migration tracking, no version table.

use libsql::Connection;

use crate::error::DatabaseError;

/// Complete schema — all 9 tables with current columns and indexes.
const SCHEMA: &str = r#"
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
        email_thread TEXT,
        card_type TEXT NOT NULL DEFAULT 'reply',
        silo TEXT NOT NULL DEFAULT 'messages',
        payload TEXT
    );
    CREATE INDEX IF NOT EXISTS idx_cards_status ON cards(status);
    CREATE INDEX IF NOT EXISTS idx_cards_channel ON cards(channel);
    CREATE INDEX IF NOT EXISTS idx_cards_silo ON cards(silo);
    CREATE INDEX IF NOT EXISTS idx_cards_card_type ON cards(card_type);

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
        parent_id TEXT,
        is_agent_internal INTEGER NOT NULL DEFAULT 0,
        agent_progress TEXT,
        thread_id TEXT,
        created_at TEXT NOT NULL DEFAULT (datetime('now')),
        updated_at TEXT NOT NULL DEFAULT (datetime('now'))
    );
    CREATE INDEX IF NOT EXISTS idx_todos_status ON todos(status);
    CREATE INDEX IF NOT EXISTS idx_todos_priority ON todos(priority);
    CREATE INDEX IF NOT EXISTS idx_todos_due_date ON todos(due_date);
    CREATE INDEX IF NOT EXISTS idx_todos_todo_type ON todos(todo_type);
    CREATE INDEX IF NOT EXISTS idx_todos_user_id ON todos(user_id);
    CREATE INDEX IF NOT EXISTS idx_todos_parent_id ON todos(parent_id);
    CREATE INDEX IF NOT EXISTS idx_todos_agent_internal ON todos(is_agent_internal);
"#;

/// Create all tables and indexes idempotently.
///
/// Uses `CREATE TABLE IF NOT EXISTS` — safe to call on every startup.
/// No migration tracking needed.
pub async fn init_schema(conn: &Connection) -> Result<(), DatabaseError> {
    conn.execute_batch(SCHEMA)
        .await
        .map_err(|e| DatabaseError::Migration(format!("Schema initialization failed: {e}")))?;

    tracing::info!("Database schema initialized");
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
    async fn creates_all_tables() {
        let conn = test_conn().await;
        init_schema(&conn).await.unwrap();

        let expected_tables = [
            "cards",
            "messages",
            "conversations",
            "conversation_messages",
            "routines",
            "routine_runs",
            "settings",
            "llm_calls",
            "todos",
        ];

        for table in &expected_tables {
            let mut rows = conn
                .query(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    libsql::params![*table],
                )
                .await
                .unwrap();
            let row = rows.next().await.unwrap().unwrap();
            let count: i64 = row.get(0).unwrap();
            assert_eq!(count, 1, "Table '{table}' should exist");
        }

        // No _migrations table
        let mut rows = conn
            .query(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='_migrations'",
                (),
            )
            .await
            .unwrap();
        let row = rows.next().await.unwrap().unwrap();
        let count: i64 = row.get(0).unwrap();
        assert_eq!(count, 0, "_migrations table should NOT exist");
    }

    #[tokio::test]
    async fn is_idempotent() {
        let conn = test_conn().await;
        init_schema(&conn).await.unwrap();
        init_schema(&conn).await.unwrap();

        // Still works, tables still exist
        let mut rows = conn
            .query(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table'",
                (),
            )
            .await
            .unwrap();
        let row = rows.next().await.unwrap().unwrap();
        let count: i64 = row.get(0).unwrap();
        assert!(count >= 9, "Expected at least 9 tables, got {count}");
    }

    #[tokio::test]
    async fn all_columns_exist() {
        let conn = test_conn().await;
        init_schema(&conn).await.unwrap();

        // Verify cards table has all columns including V6 additions
        let card_cols = get_column_names(&conn, "cards").await;
        for col in &[
            "id", "conversation_id", "source_message", "source_sender",
            "suggested_reply", "confidence", "status", "channel",
            "created_at", "expires_at", "updated_at", "message_id",
            "reply_metadata", "email_thread", "card_type", "silo", "payload",
        ] {
            assert!(card_cols.contains(&col.to_string()), "cards.{col} missing");
        }

        // Verify todos table columns
        let todo_cols = get_column_names(&conn, "todos").await;
        for col in &[
            "id", "user_id", "title", "description", "todo_type",
            "bucket", "status", "priority", "due_date", "context",
            "source_card_id", "snoozed_until", "parent_id",
            "is_agent_internal", "agent_progress", "thread_id",
            "created_at", "updated_at",
        ] {
            assert!(todo_cols.contains(&col.to_string()), "todos.{col} missing");
        }

        // Verify llm_calls table columns
        let llm_cols = get_column_names(&conn, "llm_calls").await;
        for col in &[
            "id", "conversation_id", "routine_run_id", "provider",
            "model", "input_tokens", "output_tokens", "cost",
            "purpose", "created_at",
        ] {
            assert!(llm_cols.contains(&col.to_string()), "llm_calls.{col} missing");
        }
    }

    async fn get_column_names(conn: &Connection, table: &str) -> Vec<String> {
        let mut rows = conn
            .query(&format!("PRAGMA table_info({table})"), ())
            .await
            .unwrap();
        let mut cols = Vec::new();
        while let Ok(Some(row)) = rows.next().await {
            let name: String = row.get(1).unwrap();
            cols.push(name);
        }
        cols
    }
}
