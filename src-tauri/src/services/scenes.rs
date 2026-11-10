use super::{
    devices::{ServiceRequest, ServiceResult, validate_entity_id},
    session::HaService,
};
use crate::{database::Database, errors::AppError};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct SceneFavorite {
    pub scene_entity_id: String,
    pub is_favorite: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneSummary {
    pub entity_id: String,
    pub name: String,
    pub state: String,
    pub last_updated: String,
    pub favorite: bool,
}
pub fn validate_scene(id: &str) -> Result<(), AppError> {
    validate_entity_id(id)?;
    if !id.starts_with("scene.") {
        return Err(AppError::invalid_input("必须选择 scene 实体"));
    }
    Ok(())
}
impl Database {
    pub async fn scene_favorites(&self) -> Result<Vec<SceneFavorite>, AppError> {
        sqlx::query_as("SELECT scene_entity_id,is_favorite FROM scene_favorites")
            .fetch_all(&self.pool)
            .await
            .map_err(|_| AppError::database("load_scene_favorites"))
    }
    pub async fn set_scene_favorite(
        &self,
        id: &str,
        favorite: bool,
    ) -> Result<SceneFavorite, AppError> {
        validate_scene(id)?;
        sqlx::query("INSERT INTO scene_favorites(scene_entity_id,is_favorite) VALUES(?,?) ON CONFLICT(scene_entity_id) DO UPDATE SET is_favorite=excluded.is_favorite").bind(id).bind(favorite).execute(&self.pool).await.map_err(|_|AppError::database("save_scene_favorite"))?;
        Ok(SceneFavorite {
            scene_entity_id: id.into(),
            is_favorite: favorite,
        })
    }
}
impl HaService {
    pub async fn scenes(&self, database: &Database) -> Result<Vec<SceneSummary>, AppError> {
        let favorites = database.scene_favorites().await?;
        let names = database.entity_preferences().await?;
        Ok(self
            .entities()
            .await
            .into_iter()
            .filter(|entity| entity.domain == "scene")
            .map(|entity| SceneSummary {
                favorite: favorites.iter().any(|favorite| {
                    favorite.scene_entity_id == entity.entity_id && favorite.is_favorite
                }),
                name: names
                    .iter()
                    .find(|preference| preference.entity_id == entity.entity_id)
                    .and_then(|preference| preference.display_name.clone())
                    .unwrap_or(entity.name),
                entity_id: entity.entity_id,
                state: entity.state,
                last_updated: entity.last_updated,
            })
            .collect())
    }
    pub async fn execute_scene(
        &self,
        database: &Database,
        id: &str,
    ) -> Result<ServiceResult, AppError> {
        validate_scene(id)?;
        self.call_service(
            database,
            ServiceRequest {
                entity_id: id.into(),
                domain: "scene".into(),
                service: "turn_on".into(),
                service_data: serde_json::json!({}),
            },
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::{
        catalog::DeviceCatalog, devices::validate_service, session::EntitySummary,
    };
    #[tokio::test]
    async fn scene_favorites_persist_and_other_domains_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scene.sqlite");
        let db = Database::open(&path).await.unwrap();
        db.set_scene_favorite("scene.evening", true).await.unwrap();
        assert!(db.set_scene_favorite("light.evening", true).await.is_err());
        db.close().await;
        let reopened = Database::open(&path).await.unwrap();
        assert!(reopened.scene_favorites().await.unwrap()[0].is_favorite);
        reopened
            .set_scene_favorite("scene.evening", false)
            .await
            .unwrap();
        assert!(!reopened.scene_favorites().await.unwrap()[0].is_favorite);
        reopened.close().await;
    }
    #[test]
    fn an_unactivated_scene_can_run_but_extra_parameters_and_other_services_cannot() {
        let mut entity = EntitySummary {
            entity_id: "scene.evening".into(),
            name: "Evening".into(),
            domain: "scene".into(),
            state: "unknown".into(),
            unit: None,
            last_updated: "2026-11-10T01:00:00Z".into(),
            attributes: serde_json::json!({}),
        };
        let catalog = DeviceCatalog {
            services: std::collections::BTreeMap::from([("scene".into(), vec!["turn_on".into()])]),
            services_loaded: true,
            ..Default::default()
        };
        let mut request = ServiceRequest {
            entity_id: "scene.evening".into(),
            domain: "scene".into(),
            service: "turn_on".into(),
            service_data: serde_json::json!({}),
        };
        validate_service(&entity, &catalog, &request).unwrap();
        request.service_data = serde_json::json!({"entity_id":"scene.other"});
        assert!(validate_service(&entity, &catalog, &request).is_err());
        request.service_data = serde_json::json!({});
        request.service = "reload".into();
        assert!(validate_service(&entity, &catalog, &request).is_err());
        request.service = "turn_on".into();
        entity.state = "unavailable".into();
        assert!(validate_service(&entity, &catalog, &request).is_err());
    }
}
