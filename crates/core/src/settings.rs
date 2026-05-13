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
    let mut active: settings::ActiveModel = model.into();
    active.updated_at = Set(chrono::Utc::now());
    Ok(active.update(&storage.sqlite).await?)
}
