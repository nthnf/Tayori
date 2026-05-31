use crate::backend::entities::{
    document_chunks, documents, session_answers, sessions, transcript_chunks, transcript_summaries,
};
use crate::backend::upload;
use crate::state::AppState;
use anyhow::{Result, anyhow};
use chrono::Utc;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
pub struct ProjectPageData {
    pub documents: Vec<documents::Model>,
    pub sessions: Vec<sessions::Model>,
}

pub struct ProjectPageModel {
    state: AppState,
}

impl ProjectPageModel {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    /// Fetches initial data needed to render the project details page.
    pub async fn load(&self, project_id: String) -> Result<ProjectPageData> {
        let db = &self.state.db;

        let documents = documents::Entity::find()
            .filter(documents::Column::ProjectId.eq(project_id.clone()))
            .all(db)
            .await
            .map_err(|e| anyhow!("Failed to fetch documents: {}", e))?;

        let sessions = sessions::Entity::find()
            .filter(sessions::Column::ProjectId.eq(project_id))
            .all(db)
            .await
            .map_err(|e| anyhow!("Failed to fetch sessions: {}", e))?;

        Ok(ProjectPageData {
            documents,
            sessions,
        })
    }

    /// Creates a document entry with 'ingesting' status.
    pub async fn create_ingesting_doc(
        &self,
        project_id: String,
        source_name: String,
    ) -> Result<String> {
        let db = &self.state.db;
        let doc_id = uuid::Uuid::new_v4().to_string();

        let new_doc = documents::ActiveModel {
            id: Set(doc_id.clone()),
            project_id: Set(project_id),
            source_name: Set(source_name),
            original_path: Set(None),
            content_hash: Set(None),
            status: Set("ingesting".to_string()),
            error_message: Set(None),
            created_at: Set(Utc::now()),
            updated_at: Set(Utc::now()),
        };

        new_doc
            .insert(db)
            .await
            .map_err(|e| anyhow!("Failed to insert document: {}", e))?;

        Ok(doc_id)
    }

    /// Uploads, chunks, embeds, and marks a document ready.
    pub async fn process_and_ready_doc(
        &self,
        doc_id: String,
        bytes: &[u8],
        extension: &str,
    ) -> Result<()> {
        let db = &self.state.db;

        // Helper to mark failed
        let mark_failed = |err_msg: String, id: String| async move {
            let _ = documents::Entity::update(documents::ActiveModel {
                id: Set(id),
                status: Set("failed".to_string()),
                error_message: Set(Some(err_msg.clone())),
                updated_at: Set(Utc::now()),
                ..Default::default()
            })
            .exec(db)
            .await;
            anyhow!("{}", err_msg)
        };

        // 1. Process document into chunks
        let chunks = match upload::process_document(bytes, extension) {
            Ok(c) => c,
            Err(e) => {
                return Err(mark_failed(format!("Failed to parse document: {}", e), doc_id).await);
            }
        };
        if chunks.is_empty() {
            return Err(mark_failed(
                "No text chunks could be extracted from the document".into(),
                doc_id,
            )
            .await);
        }

        // 2. Initialize embedder if not exists
        if let Err(e) = self
            .state
            .embedder
            .get_or_try_init(|| async {
                crate::backend::models::embed::Embedder::new()
                    .map_err(|e| anyhow!("Failed to init embedder: {}", e))
            })
            .await
        {
            return Err(mark_failed(format!("Failed to init embedder: {}", e), doc_id).await);
        }

        // 3. Generate embeddings (CPU bound)
        let chunks_clone = chunks.clone();
        let embedder_cell = self.state.embedder.clone();
        let embeddings = match tokio::task::spawn_blocking(move || {
            let embedder = embedder_cell
                .get()
                .ok_or_else(|| anyhow!("Embedder cell empty"))?;
            embedder.embed(chunks_clone)
        })
        .await
        {
            Ok(Ok(emb)) => emb,
            Ok(Err(e)) => {
                return Err(mark_failed(format!("Failed to embed chunks: {}", e), doc_id).await);
            }
            Err(e) => return Err(mark_failed(format!("Task failed: {}", e), doc_id).await),
        };

        // 4. Save chunks
        let mut chunk_models = Vec::new();
        for (i, (text, vector)) in chunks.into_iter().zip(embeddings.into_iter()).enumerate() {
            let vector_u8: Vec<u8> = vector.into_iter().flat_map(|f| f.to_le_bytes()).collect();
            chunk_models.push(document_chunks::ActiveModel {
                id: Set(uuid::Uuid::new_v4().to_string()),
                document_id: Set(doc_id.clone()),
                chunk_index: Set(i as i64),
                content: Set(text),
                vector: Set(vector_u8),
                created_at: Set(Utc::now()),
            });
        }

        if !chunk_models.is_empty()
            && let Err(e) = document_chunks::Entity::insert_many(chunk_models)
                .exec(db)
                .await
        {
            return Err(mark_failed(format!("Failed to insert chunks: {}", e), doc_id).await);
        }

        // 5. Mark document as ready
        documents::Entity::update(documents::ActiveModel {
            id: Set(doc_id),
            status: Set("ready".to_string()),
            updated_at: Set(Utc::now()),
            ..Default::default()
        })
        .exec(db)
        .await
        .map_err(|e| anyhow!("Failed to mark document ready: {}", e))?;

        Ok(())
    }

    /// Removes a document and all associated chunks.
    pub async fn remove_docs(&self, document_id: String) -> Result<()> {
        let db = &self.state.db;

        // Manual cascade to ensure chunks are deleted if DB constraints aren't strict
        document_chunks::Entity::delete_many()
            .filter(document_chunks::Column::DocumentId.eq(document_id.clone()))
            .exec(db)
            .await
            .map_err(|e| anyhow!("Failed to delete chunks: {}", e))?;

        documents::Entity::delete_by_id(document_id)
            .exec(db)
            .await
            .map_err(|e| anyhow!("Failed to delete document: {}", e))?;

        Ok(())
    }

    /// Creates a new session in the project.
    pub async fn create_session(
        &self,
        project_id: String,
        title: Option<String>,
    ) -> Result<String> {
        let db = &self.state.db;

        let session_id = uuid::Uuid::new_v4().to_string();
        let new_session = sessions::ActiveModel {
            id: Set(session_id.clone()),
            project_id: Set(project_id),
            title: Set(title),
            status: Set("running".to_string()),
            started_at: Set(Utc::now()),
            ended_at: Set(None),
            created_at: Set(Utc::now()),
            updated_at: Set(Utc::now()),
        };

        new_session
            .insert(db)
            .await
            .map_err(|e| anyhow!("Failed to create session: {}", e))?;

        Ok(session_id)
    }

    /// Removes a session by ID.
    pub async fn remove_session(&self, session_id: String) -> Result<()> {
        let db = &self.state.db;

        transcript_chunks::Entity::delete_many()
            .filter(transcript_chunks::Column::SessionId.eq(session_id.clone()))
            .exec(db)
            .await
            .map_err(|e| anyhow!("Failed to delete transcript chunks: {}", e))?;

        transcript_summaries::Entity::delete_many()
            .filter(transcript_summaries::Column::SessionId.eq(session_id.clone()))
            .exec(db)
            .await
            .map_err(|e| anyhow!("Failed to delete transcript summaries: {}", e))?;

        session_answers::Entity::delete_many()
            .filter(session_answers::Column::SessionId.eq(session_id.clone()))
            .exec(db)
            .await
            .map_err(|e| anyhow!("Failed to delete session answers: {}", e))?;

        sessions::Entity::delete_by_id(session_id)
            .exec(db)
            .await
            .map_err(|e| anyhow!("Failed to delete session: {}", e))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::entities::projects;
    use migration::MigratorTrait;
    use sea_orm::Database;

    async fn setup_test_state() -> (AppState, String) {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        migration::Migrator::up(&db, None).await.unwrap();

        // Create a dummy project first
        let project_id = "test-project-id".to_string();
        projects::ActiveModel {
            id: Set(project_id.clone()),
            name: Set("Test Project".to_string()),
            created_at: Set(Utc::now()),
            updated_at: Set(Utc::now()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        (AppState::new(db), project_id)
    }

    #[tokio::test]
    async fn test_load_project_page() {
        let (state, project_id) = setup_test_state().await;
        let model = ProjectPageModel::new(state);

        let result = model.load(project_id).await.unwrap();
        assert_eq!(result.documents.len(), 0);
        assert_eq!(result.sessions.len(), 0);
    }

    // Testing upload would be slow because it downloads the embedding model,
    // so we'll just test the session logic in unit tests for speed.
    #[tokio::test]
    async fn test_create_and_remove_session() {
        let (state, project_id) = setup_test_state().await;
        let model = ProjectPageModel::new(state);

        model
            .create_session(project_id.clone(), Some("Test Session".to_string()))
            .await
            .unwrap();

        let result = model.load(project_id.clone()).await.unwrap();
        assert_eq!(result.sessions.len(), 1);
        let session_id = result.sessions[0].id.clone();

        model.remove_session(session_id).await.unwrap();

        let result = model.load(project_id).await.unwrap();
        assert_eq!(result.sessions.len(), 0);
    }
}
