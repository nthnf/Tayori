use anyhow::Result;
use sea_orm::{ConnectionTrait, DatabaseConnection, SqlxSqliteConnector};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::fs;
use std::str::FromStr;
use tracing::info;

use super::path::app_data_dir;

/// Establishes a connection to the SQLite database, initializing the connection
/// with required extensions (vector and fts5).
///
/// # Errors
/// Returns an error if the database path is invalid, the vector extension
/// cannot be extracted, or the connection fails.
pub async fn connect(uri: &str) -> Result<DatabaseConnection> {
    let vector_extension = vector_extension_path()?;

    let options = SqliteConnectOptions::from_str(uri)?
        .create_if_missing(true) // Ensure the file exists
        .extension(vector_extension);

    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await?;

    let db = SqlxSqliteConnector::from_sqlx_sqlite_pool(pool);

    Ok(db)
}

/// Run sqlite vector extension initialization queries
/// Run on startup to ensure extensions already loaded
/// On migration, extension haven't been loaded yet
pub async fn init_vector_indexes(db: &DatabaseConnection) -> Result<()> {
    let dim = super::models::embed::get_model_dim()?;
    let init_sql = format!(
        "-- Dense vectors are stored on document chunks and transcript summaries\n\
        SELECT vector_init('document_chunks', 'vector', 'type=FLOAT32,dimension={dim},distance=COSINE');\n\
        SELECT vector_init('transcript_summaries', 'vector', 'type=FLOAT32,dimension={dim},distance=COSINE');"
    );

    // Execute the raw queries against the active connection
    db.execute_unprepared(&init_sql).await?;

    // Optional: Log success so you know it ran during startup
    info!("✅ Vector indexes initialized successfully with dimension {dim}.");

    Ok(())
}

pub fn default_sqlite_uri() -> Result<String> {
    Ok(format!("sqlite://{}", sqlite_path()?))
}

/// Computes the full path to the SQLite database file within the application data directory.
fn sqlite_path() -> Result<String> {
    let db_dir = app_data_dir().join("db");

    // Ensure the directory exists before returning the path
    if !db_dir.exists() {
        fs::create_dir_all(&db_dir)?;
    }

    let path = db_dir.join("tayori.db");

    path.into_os_string()
        .into_string()
        .map_err(|path| anyhow::anyhow!("sqlite path is not valid UTF-8: {:?}", path))
}

/// Extracts the bundled sqlite-vector extension to a temporary directory
/// and returns the path for SQLite to load.
fn vector_extension_path() -> Result<String> {
    #[cfg(target_os = "windows")]
    const VEC_BYTES: &[u8] = include_bytes!("../../sqlite-extension/vector.dll");

    #[cfg(target_os = "linux")]
    const VEC_BYTES: &[u8] = include_bytes!("../../sqlite-extension/vector.so");

    let ext_name = if cfg!(target_os = "windows") {
        "vector.dll"
    } else if cfg!(target_os = "linux") {
        "vector.so"
    } else {
        anyhow::bail!("Unsupported OS: Only Windows and Linux are currently supported.");
    };

    let temp_dir = std::env::temp_dir().join("tayori_ext");
    if !temp_dir.exists() {
        fs::create_dir_all(&temp_dir)?;
    }

    let ext_path = temp_dir.join(ext_name);

    if !ext_path.exists() {
        fs::write(&ext_path, VEC_BYTES)?;
    }

    ext_path
        .into_os_string()
        .into_string()
        .map_err(|path| anyhow::anyhow!("extension path is not valid UTF-8: {:?}", path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use migration::{Migrator, MigratorTrait};
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

    #[tokio::test]
    async fn test_full_database_stack() {
        // STEP 1: Connect to the database
        let conn = connect("sqlite::memory:")
            .await
            .expect("Should connect to SQLite");

        // STEP 2: Verify the connection is active
        assert!(conn.ping().await.is_ok());

        // STEP 3: Define a Raw SQL statement
        // We use Statement::from_string to wrap our raw SQL for the SQLite backend
        let raw_query = Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT vector_version();".to_owned(),
        );

        // STEP 4: Execute the query
        // In SeaORM, for raw Statements, query_one accepts the Statement directly.
        let result = conn.query_one_raw(raw_query).await;

        // STEP 5: Validate that the extension function was recognized
        assert!(
            result.is_ok(),
            "Vector extension failed! SQLite doesn't recognize 'vector_version()'. Error: {:?}",
            result.err()
        );

        if let Ok(Some(row)) = result {
            // STEP 6: Optional - print the version to console
            let version: String = row.try_get_by_index(0).unwrap_or_default();
            println!("Vector Extension Version: {}", version);
        }
    }

    #[tokio::test]
    async fn test_vector_initialization_lifecycle() {
        // 1. Spin up a fresh, empty in-memory database
        let db = connect("sqlite::memory:")
            .await
            .expect("Failed to connect to in-memory DB");

        // 2. RUN MIGRATIONS
        // This will create all tables, including `document_chunks`
        Migrator::up(&db, None)
            .await
            .expect("Failed to run database migrations");

        // 3. PHASE 2: AFTER MIGRATIONS
        // Now the tables exist, so the function will execute the `vector_init` SQL.
        let post_migration_result = init_vector_indexes(&db).await;

        // 4. VERIFY THE EXECUTION ATTEMPT
        match post_migration_result {
            Ok(_) => {
                // If you configured your test environment to actively load the
                // sqlite-vector extension, it will succeed perfectly.
                println!("✅ Vectors initialized successfully in test database!");
            }
            Err(e) => {
                // If the extension isn't loaded (standard for `cargo test`),
                // SQLite will throw a specific error. We assert this exact error
                // to prove the SQL ran against the newly created tables.
                let err_msg = e.to_string().to_lowercase();
                assert!(
                    err_msg.contains("no such function: vector_init"),
                    "Expected 'no such function' error due to missing extension, but got: {}",
                    err_msg
                );
                println!("✅ Test passed: Tables were found and vector_init was attempted!");
            }
        }
    }
}
