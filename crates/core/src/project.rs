use anyhow::Result;
use chrono::Utc;
use sea_orm::{ActiveModelTrait, EntityTrait, QueryOrder, Set};
use storage::{entities::projects, storage::Storage};
use uuid::Uuid;

pub async fn create_project(
    storage: &Storage,
    name: impl Into<String>,
    description: Option<String>,
) -> Result<projects::Model> {
    let now = Utc::now();
    let project = projects::ActiveModel {
        id: Set(Uuid::new_v4().to_string()),
        name: Set(name.into()),
        description: Set(description),
        created_at: Set(now),
        updated_at: Set(now),
    };

    Ok(project.insert(&storage.sqlite).await?)
}

pub async fn list_projects(storage: &Storage) -> Result<Vec<projects::Model>> {
    Ok(projects::Entity::find()
        .order_by_desc(projects::Column::UpdatedAt)
        .all(&storage.sqlite)
        .await?)
}

pub async fn get_project(storage: &Storage, project_id: &str) -> Result<Option<projects::Model>> {
    Ok(projects::Entity::find_by_id(project_id)
        .one(&storage.sqlite)
        .await?)
}
