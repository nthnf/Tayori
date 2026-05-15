use anyhow::Result;
use sea_orm::{ActiveModelTrait, EntityTrait, Set};
use storage::{entities::settings, storage::Storage};

pub async fn get_settings(storage: &Storage) -> Result<settings::Model> {
    settings::Entity::find_by_id("default")
        .one(&storage.sqlite)
        .await?
        .ok_or_else(|| anyhow::anyhow!("default settings row not found"))
}

pub async fn update_settings(storage: &Storage, model: settings::Model) -> Result<settings::Model> {
    let active = settings::ActiveModel {
        id: Set(model.id),
        llm_provider: Set(model.llm_provider),
        llm_model: Set(model.llm_model),
        llm_api_key_ref: Set(model.llm_api_key_ref),
        llm_store_responses: Set(model.llm_store_responses),
        embedding_provider: Set(model.embedding_provider),
        embedding_model: Set(model.embedding_model),
        sparse_model: Set(model.sparse_model),
        embedding_dimension: Set(model.embedding_dimension),
        reranker_model: Set(model.reranker_model),
        whisper_model: Set(model.whisper_model),
        summary_minutes: Set(model.summary_minutes),
        ui_theme: Set(model.ui_theme),
        created_at: Set(model.created_at),
        updated_at: Set(chrono::Utc::now()),
    };
    Ok(active.update(&storage.sqlite).await?)
}
