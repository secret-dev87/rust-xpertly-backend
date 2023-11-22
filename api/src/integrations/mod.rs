use actix_web::web::Json;
use actix_web::web::{Data, Path};
use actix_web::{delete, get, post, put, HttpResponse};
use mongodb::bson::{doc, oid::ObjectId};
use serde_json::{json, Value, Map};
use uuid::Uuid;
use xpertly_common::{asset::Asset, asset::AssetTag, integration::Integration, Display};

use crate::WebServerData;

#[post("/api/tenants/{tenant_id}/integrations/{integration_type}/create")]
pub async fn create_integration(
    ws_data: Data<WebServerData>,
    path: Path<(String, String)>,
    data: Json<Value>
) -> HttpResponse {
    let (tenant_id, integration_type) = path.into_inner();
    if let Some(db) = &ws_data.db {
        let integration_id = Uuid::new_v4().to_string();
        let integration_type = integration_type.to_string();
        let mut integration_base = json!(
            {
                "PK": tenant_id,
                "SK": format!("integration#{}#{}", integration_type, integration_id),
            }
        );

        for (key, value) in data.as_object().unwrap().iter() {
            integration_base[key] = value.clone();
        }

        let integration = serde_json::from_value::<Integration>(integration_base).unwrap();
        let integration_detail = db.insert_one(&integration).await;
        match integration_detail {
            Ok(_) => HttpResponse::Ok().json(integration.display()),
            Err(err) => HttpResponse::InternalServerError().body(err.to_string()),
        }
    } else {
        HttpResponse::InternalServerError().body("No database connection")
    }
}

#[get("/api/tenants/{tenant_id}/integrations/{integration_type}")]
pub async fn get_integrations(
    ws_data: Data<WebServerData>,
    path: Path<(String, String)>,
) -> HttpResponse {
    let (tenant_id, integration_type) = path.into_inner();
    if let Some(db) = &ws_data.db {
        let filter = doc! {"PK": tenant_id, "SK": {"$regex": format!("^integration#{}#", integration_type)}};
        let integrations = db.filter_items::<Integration>(Some(filter)).await;
        match integrations {
            Ok(integrations) => {
                HttpResponse::Ok().json(integrations.display())
            }
            Err(err) => HttpResponse::InternalServerError().body(err.to_string()),
        }
    } else {
        HttpResponse::InternalServerError().body("No database connection")
    }
}

#[get("/api/tenants/{tenant_id}/integrations/{integration_type}/{integration_id}")]
pub async fn get_integration(
    ws_data: Data<WebServerData>,
    path: Path<(String, String, String)>,
) -> HttpResponse {
    let (tenant_id, integration_type, integration_id) = path.into_inner();
    if let Some(db) = &ws_data.db {
        let filter = doc! {"PK": tenant_id, "SK": format!("integration#{}#{}", integration_type, integration_id)};
        let integration = db.filter_item::<Integration>(Some(filter)).await;
        match integration {
            Ok(Some(integration)) => {
                HttpResponse::Ok().json(integration.display())
            }
            Ok(None) => HttpResponse::NotFound().body("Integration not found"),
            Err(err) => HttpResponse::InternalServerError().body(err.to_string()),
        }
    } else {
        HttpResponse::InternalServerError().body("No database connection")
    }
}