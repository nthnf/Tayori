use anyhow::Result;
use lancedb::{Connection, connect};
use sea_orm::{Database, DatabaseConnection};
use std::path::Path;
use std::time::Duration;
use tokio::time::sleep;
use tracing::warn;

use crate::path::{StorageKind, default_storage_path, ensure_path_exists};

const CONNECT_RETRY_DELAY: Duration = Duration::from_secs(2);
const CONNECT_MAX_RETRIES: usize = 30;

/// Application storage handle.
///
/// `Storage` owns both durable relational storage (`sqlite`) and rebuildable
/// search/index storage (`lancedb`). SQLite is the source of truth for user data,
/// settings, transcript chunks, summaries, sessions, and documents. LanceDB stores
/// derived searchable copies such as document chunks and transcript summaries.
///
/// Creating a `Storage` value also ensures LanceDB tables and indexes exist. The
/// SQLite schema/default settings row must already exist before `new()` runs.
pub struct Storage {
    /// SeaORM connection for the SQLite source-of-truth database.
    pub sqlite: DatabaseConnection,
    /// LanceDB connection for vector and full-text search tables.
    pub lancedb: Connection,
}

impl Storage {
    /// Connect to SQLite and LanceDB, then ensure LanceDB tables/indexes exist.
    ///
    /// This does not run SQLite migrations. The caller should run migrations first
    /// so the settings row exists; LanceDB needs `settings.embedding_dimension` to
    /// build fixed-size vector schemas.
    pub async fn new() -> Result<Self> {
        let sqlite_path = default_storage_path(StorageKind::Sqlite)?;
        let lancedb_path = default_storage_path(StorageKind::LanceDb)?;

        Self::new_at(sqlite_path, lancedb_path).await
    }

    async fn new_at(sqlite_path: impl AsRef<Path>, lancedb_path: impl AsRef<Path>) -> Result<Self> {
        let sqlite = Self::connect_sqlite(sqlite_path).await?;
        let lancedb = Self::connect_lance_db(lancedb_path).await?;
        let storage = Self { sqlite, lancedb };

        storage.ensure_lance_schema().await?;

        Ok(storage)
    }

    async fn connect_sqlite(path: impl AsRef<Path>) -> Result<DatabaseConnection> {
        let mut last_error = None;
        ensure_path_exists(StorageKind::Sqlite, path.as_ref())?;
        let db_uri = format!("sqlite://{}?mode=rwc", path.as_ref().display());

        for attempt in 1..=CONNECT_MAX_RETRIES {
            match Database::connect(db_uri.as_str()).await {
                Ok(sqlite) => return Ok(sqlite),
                Err(error) => {
                    warn!(attempt, error = %error, "sqlite connection failed; retrying");
                    last_error = Some(error);
                    sleep(CONNECT_RETRY_DELAY).await;
                }
            }
        }

        Err(last_error
            .expect("database connection should have been attempted")
            .into())
    }

    async fn connect_lance_db(path: impl AsRef<Path>) -> Result<Connection> {
        let mut last_error = None;
        ensure_path_exists(StorageKind::LanceDb, path.as_ref())?;
        let db_uri = path.as_ref().display().to_string();

        for attempt in 1..=CONNECT_MAX_RETRIES {
            match connect(db_uri.as_str()).execute().await {
                Ok(conn) => return Ok(conn),
                Err(error) => {
                    warn!(attempt, error = %error, "lancedb connection failed; retrying");
                    last_error = Some(error);
                    sleep(CONNECT_RETRY_DELAY).await;
                }
            }
        }

        Err(last_error
            .expect("database connection should have been attempted")
            .into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::ConnectionTrait;

    #[tokio::test]
    async fn storage_new_at_ensures_lance_tables_and_indexes() {
        let temp = tempfile::tempdir().unwrap();
        let sqlite_path = temp.path().join("sqlite").join("tayori.db");
        let lancedb_path = temp.path().join("lancedb");

        seed_minimal_settings(&sqlite_path).await;

        let storage = Storage::new_at(&sqlite_path, &lancedb_path).await.unwrap();
        let table_names = storage.lancedb.table_names().execute().await.unwrap();

        assert!(table_names.iter().any(|name| name == "document_chunks"));
        assert!(
            table_names
                .iter()
                .any(|name| name == "transcript_summaries")
        );

        let doc_indexes = storage
            .lancedb
            .open_table("document_chunks")
            .execute()
            .await
            .unwrap()
            .list_indices()
            .await
            .unwrap();
        assert!(doc_indexes.iter().any(|index| index.columns == ["text"]));

        let summary_indexes = storage
            .lancedb
            .open_table("transcript_summaries")
            .execute()
            .await
            .unwrap()
            .list_indices()
            .await
            .unwrap();
        assert!(
            summary_indexes
                .iter()
                .any(|index| index.columns == ["summary"])
        );
    }

    async fn seed_minimal_settings(sqlite_path: &Path) {
        ensure_path_exists(StorageKind::Sqlite, sqlite_path).unwrap();
        let db = Database::connect(format!("sqlite://{}?mode=rwc", sqlite_path.display()))
            .await
            .unwrap();

        db.execute_unprepared(
            "CREATE TABLE settings (
                id TEXT PRIMARY KEY,
                llm_provider TEXT NOT NULL,
                llm_model TEXT NOT NULL,
                llm_api_key_ref TEXT NULL,
                llm_store_responses INTEGER NOT NULL,
                embedding_provider TEXT NOT NULL,
                embedding_model TEXT NOT NULL,
                embedding_dimension INTEGER NOT NULL,
                reranker_model TEXT NOT NULL,
                whisper_model TEXT NULL,
                summary_minutes INTEGER NOT NULL,
                ui_theme TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
        )
        .await
        .unwrap();

        db.execute_unprepared(
            "INSERT INTO settings (
                id,
                llm_provider,
                llm_model,
                llm_store_responses,
                embedding_provider,
                embedding_model,
                embedding_dimension,
                reranker_model,
                summary_minutes,
                ui_theme,
                created_at,
                updated_at
            ) VALUES (
                'default',
                'openai',
                'gpt-5.4-mini',
                0,
                'fastembed',
                'jinaai/jina-embeddings-v2-base-code',
                4,
                'jinaai/jina-reranker-v1-turbo-en',
                5,
                'dark',
                '2026-01-01 00:00:00',
                '2026-01-01 00:00:00'
            )",
        )
        .await
        .unwrap();
    }
}
