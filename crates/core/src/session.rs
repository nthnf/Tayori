use anyhow::Result;
use chrono::Utc;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, QueryOrder, Set};
use storage::{entities::sessions, storage::Storage};
use uuid::Uuid;

pub async fn create_session(
    storage: &Storage,
    project_id: impl Into<String>,
    title: Option<String>,
) -> Result<sessions::Model> {
    let now = Utc::now();
    let session = sessions::ActiveModel {
        id: Set(Uuid::new_v4().to_string()),
        project_id: Set(project_id.into()),
        title: Set(title),
        status: Set("running".to_string()),
        started_at: Set(now),
        ended_at: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
    };

    Ok(session.insert(&storage.sqlite).await?)
}

pub async fn list_sessions(storage: &Storage, project_id: &str) -> Result<Vec<sessions::Model>> {
    Ok(sessions::Entity::find()
        .filter(sessions::Column::ProjectId.eq(project_id))
        .order_by_desc(sessions::Column::StartedAt)
        .all(&storage.sqlite)
        .await?)
}

pub async fn end_session(storage: &Storage, session_id: &str) -> Result<()> {
    let Some(session) = sessions::Entity::find_by_id(session_id)
        .one(&storage.sqlite)
        .await?
    else {
        return Ok(());
    };

    let now = Utc::now();
    let mut active: sessions::ActiveModel = session.into();
    active.status = Set("completed".to_string());
    active.ended_at = Set(Some(now));
    active.updated_at = Set(now);
    active.update(&storage.sqlite).await?;
    Ok(())
}

pub async fn delete_session(storage: &Storage, session_id: &str) -> Result<()> {
    sessions::Entity::delete_by_id(session_id)
        .exec(&storage.sqlite)
        .await?;
    Ok(())
}
