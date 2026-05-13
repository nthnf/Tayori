use anyhow::{Result, ensure};
use chrono::Utc;
use detection::{Detection, detect_question_or_command};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, QueryOrder, Set};
use storage::{entities::transcript_chunks, storage::Storage};
use stt::chunk::TranscriptionChunk;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq)]
pub struct StoredTranscriptChunk {
    pub chunk: transcript_chunks::Model,
    pub detection: Detection,
}

pub async fn persist_transcription_chunk(
    storage: &Storage,
    project_id: &str,
    session_id: &str,
    chunk: TranscriptionChunk,
) -> Result<StoredTranscriptChunk> {
    ensure!(chunk.start_ms <= i64::MAX as u64, "start_ms exceeds i64");
    ensure!(chunk.end_ms <= i64::MAX as u64, "end_ms exceeds i64");
    ensure!(
        chunk.duration_ms <= i64::MAX as u64,
        "duration_ms exceeds i64"
    );

    let detection = detect_question_or_command(&chunk.body);
    let model = transcript_chunks::ActiveModel {
        id: Set(Uuid::new_v4().to_string()),
        session_id: Set(session_id.to_string()),
        project_id: Set(project_id.to_string()),
        chunk_index: Set(chunk.index as i64),
        text: Set(chunk.body),
        start_ms: Set(chunk.start_ms as i64),
        end_ms: Set(chunk.end_ms as i64),
        duration_ms: Set(chunk.duration_ms as i64),
        created_at: Set(Utc::now()),
    }
    .insert(&storage.sqlite)
    .await?;

    Ok(StoredTranscriptChunk {
        chunk: model,
        detection,
    })
}

pub async fn list_transcript_chunks(
    storage: &Storage,
    session_id: &str,
) -> Result<Vec<transcript_chunks::Model>> {
    Ok(transcript_chunks::Entity::find()
        .filter(transcript_chunks::Column::SessionId.eq(session_id))
        .order_by_asc(transcript_chunks::Column::ChunkIndex)
        .all(&storage.sqlite)
        .await?)
}
