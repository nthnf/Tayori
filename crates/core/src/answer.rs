use anyhow::Result;
use futures_core::Stream;
use futures_util::StreamExt;
use llm::{ChatStream, LlmClient};
use sea_orm::{ActiveModelTrait, Set};
use std::pin::Pin;
use storage::{entities::session_answers, storage::Storage};
use uuid::Uuid;

pub type AnswerStream = Pin<Box<dyn Stream<Item = Result<String>> + Send>>;

pub async fn stream_answer(
    llm: &LlmClient,
    model: &str,
    question: &str,
    context: &str,
) -> Result<ChatStream> {
    llm.stream_answer_from_context(model, question, context)
        .await
}

pub async fn persist_streamed_answer(
    storage: &Storage,
    project_id: &str,
    session_id: &str,
    question: String,
    context: String,
    mut stream: ChatStream,
) -> Result<String> {
    let mut answer = String::new();
    while let Some(delta) = stream.next().await {
        answer.push_str(&delta?);
    }

    persist_answer(
        storage,
        project_id,
        session_id,
        question,
        Some(context),
        answer.clone(),
    )
    .await?;
    Ok(answer)
}

pub async fn persist_answer(
    storage: &Storage,
    project_id: &str,
    session_id: &str,
    query: String,
    context: Option<String>,
    answer: String,
) -> Result<session_answers::Model> {
    Ok(session_answers::ActiveModel {
        id: Set(Uuid::new_v4().to_string()),
        project_id: Set(project_id.to_string()),
        session_id: Set(session_id.to_string()),
        context: Set(context),
        query: Set(query),
        answer: Set(answer),
        created_at: Set(chrono::Utc::now()),
    }
    .insert(&storage.sqlite)
    .await?)
}
