use crate::backend::entities::projects;
use crate::state::AppState;
use anyhow::{Result, anyhow};
use sea_orm::EntityTrait;

pub struct DashboardPageData {
    pub projects: Vec<projects::Model>,
}

pub struct DashboardPageModel {
    state: AppState,
}

impl DashboardPageModel {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    /// Fetches initial data needed to render the page using the global state.
    pub async fn load(&self) -> Result<DashboardPageData> {
        let db = &self.state.db;

        let projects = projects::Entity::find()
            .all(db)
            .await
            .map_err(|e| anyhow!("Failed to fetch projects: {}", e))?;

        Ok(DashboardPageData { projects })
    }

    /// Action to create a new project.
    pub async fn create_project(&self, name: String, desc: String) -> Result<()> {
        use sea_orm::ActiveModelTrait;
        use sea_orm::Set;

        let db = &self.state.db;

        let new_project = projects::ActiveModel {
            id: Set(uuid::Uuid::new_v4().to_string()),
            name: Set(name),
            description: Set(if desc.is_empty() { None } else { Some(desc) }),
            created_at: Set(chrono::Utc::now()),
            updated_at: Set(chrono::Utc::now()),
        };

        new_project
            .insert(db)
            .await
            .map_err(|e| anyhow!("Failed to create project: {}", e))?;
        Ok(())
    }

    /// Action to delete a project by ID.
    pub async fn delete_project(&self, id: String) -> Result<()> {
        let db = &self.state.db;

        projects::Entity::delete_by_id(id)
            .exec(db)
            .await
            .map_err(|e| anyhow!("Failed to delete project: {}", e))?;

        Ok(())
    }

    /// Action to update a project.
    pub async fn update_project(&self, id: String, name: String, desc: String) -> Result<()> {
        use sea_orm::ActiveModelTrait;
        use sea_orm::Set;

        let db = &self.state.db;

        let project = projects::Entity::find_by_id(id.clone())
            .one(db)
            .await
            .map_err(|e| anyhow!("Failed to fetch project for update: {}", e))?
            .ok_or_else(|| anyhow!("Project not found"))?;

        let mut active_project: projects::ActiveModel = project.into();
        active_project.name = Set(name);
        active_project.description = Set(if desc.is_empty() { None } else { Some(desc) });
        active_project.updated_at = Set(chrono::Utc::now());

        active_project
            .update(db)
            .await
            .map_err(|e| anyhow!("Failed to update project: {}", e))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ConnectionTrait, Database, Schema};

    async fn setup_test_state() -> AppState {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        let schema = Schema::new(db.get_database_backend());
        let stmt = schema.create_table_from_entity(projects::Entity);
        db.execute(&stmt).await.unwrap();

        AppState::new(db)
    }

    #[tokio::test]
    async fn test_create_and_load_project() {
        let state = setup_test_state().await;
        let model = DashboardPageModel::new(state);

        model
            .create_project("Test Project".to_string(), "".to_string())
            .await
            .unwrap();

        let data = model.load().await.unwrap();
        assert_eq!(data.projects.len(), 1);
        assert_eq!(data.projects[0].name, "Test Project");
    }

    #[tokio::test]
    async fn test_delete_project() {
        let state = setup_test_state().await;
        let model = DashboardPageModel::new(state);

        model
            .create_project("Test Project".to_string(), "".to_string())
            .await
            .unwrap();

        let data = model.load().await.unwrap();
        let id = data.projects[0].id.clone();

        model.delete_project(id).await.unwrap();

        let data = model.load().await.unwrap();
        assert_eq!(data.projects.len(), 0);
    }
}
