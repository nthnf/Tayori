use crate::backend::entities::settings;
use crate::state::AppState;
use anyhow::{Result, anyhow};
use chrono::Utc;
use sea_orm::{ActiveModelTrait, EntityTrait, Set};

pub struct SettingsPageData {
    pub settings: settings::Model,
}

pub struct SettingsPageModel {
    state: AppState,
}

impl SettingsPageModel {
    const DEFAULT_ID: &'static str = "default";

    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    /// Fetches initial data needed to render the settings page.
    /// If no settings exist, it creates a default configuration.
    pub async fn load(&self) -> Result<SettingsPageData> {
        let db = &self.state.db;

        if let Some(existing) = settings::Entity::find_by_id(Self::DEFAULT_ID.to_string())
            .one(db)
            .await
            .map_err(|e| anyhow!("Failed to fetch settings: {}", e))?
        {
            return Ok(SettingsPageData { settings: existing });
        }

        // Create defaults if not exists
        let default_settings = settings::ActiveModel {
            id: Set(Self::DEFAULT_ID.to_string()),
            llm_provider: Set("openai".to_string()),
            llm_model: Set("gpt-5.4-mini".to_string()),
            transcript_model: Set("medium".to_string()),
            summary_minutes: Set(5),
            ui_theme: Set("light".to_string()),
            created_at: Set(Utc::now()),
            updated_at: Set(Utc::now()),
        };

        let settings = default_settings
            .insert(db)
            .await
            .map_err(|e| anyhow!("Failed to create default settings: {}", e))?;

        Ok(SettingsPageData { settings })
    }

    /// Action to update the settings.
    pub async fn update(&self, updated_settings: settings::Model) -> Result<()> {
        let db = &self.state.db;

        let active_model = settings::ActiveModel {
            id: Set(updated_settings.id),
            llm_provider: Set(updated_settings.llm_provider),
            llm_model: Set(updated_settings.llm_model),
            transcript_model: Set(updated_settings.transcript_model),
            summary_minutes: Set(updated_settings.summary_minutes),
            ui_theme: Set(updated_settings.ui_theme),
            created_at: Set(updated_settings.created_at),
            updated_at: Set(Utc::now()),
        };

        active_model
            .update(db)
            .await
            .map_err(|e| anyhow!("Failed to update settings: {}", e))?;

        Ok(())
    }

    /// Action to reset settings to default values.
    pub async fn reset(&self) -> Result<()> {
        let db = &self.state.db;

        let default_settings = settings::ActiveModel {
            id: Set(Self::DEFAULT_ID.to_string()),
            llm_provider: Set("openai".to_string()),
            llm_model: Set("gpt-5.4-mini".to_string()),
            transcript_model: Set("medium".to_string()),
            summary_minutes: Set(5),
            ui_theme: Set("light".to_string()),
            updated_at: Set(Utc::now()),
            ..Default::default()
        };

        default_settings
            .update(db)
            .await
            .map_err(|e| anyhow!("Failed to reset settings: {}", e))?;

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
        let stmt = schema.create_table_from_entity(settings::Entity);
        db.execute(&stmt).await.unwrap();

        AppState::new(db)
    }

    #[tokio::test]
    async fn test_load_creates_defaults() {
        let state = setup_test_state().await;
        let model = SettingsPageModel::new(state);

        let data = model.load().await.unwrap();
        assert_eq!(data.settings.llm_provider, "openai");
        assert_eq!(data.settings.llm_model, "gpt-5.4-mini");
        assert_eq!(data.settings.ui_theme, "light");
    }

    #[tokio::test]
    async fn test_update_settings() {
        let state = setup_test_state().await;
        let model = SettingsPageModel::new(state);

        let mut data = model.load().await.unwrap();
        data.settings.ui_theme = "dark".to_string();

        model.update(data.settings).await.unwrap();

        let updated_data = model.load().await.unwrap();
        assert_eq!(updated_data.settings.ui_theme, "dark");
    }

    #[tokio::test]
    async fn test_reset_settings() {
        let state = setup_test_state().await;
        let model = SettingsPageModel::new(state);

        let mut data = model.load().await.unwrap();
        data.settings.ui_theme = "dark".to_string();
        model.update(data.settings).await.unwrap();

        model.reset().await.unwrap();

        let final_data = model.load().await.unwrap();
        assert_eq!(final_data.settings.ui_theme, "light");
    }
}
