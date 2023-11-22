use std::collections::HashMap;

use actix_web::web::{Json, Query, UrlEncoded};
use actix_web::web::{Data, Path};
use actix_web::{delete, get, post, put, HttpResponse, HttpRequest};
use mongodb::bson::{doc, oid::ObjectId};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;
use xpertly_common::Device;
use xpertly_common::{asset::Asset, asset::AssetTag, Display};

use crate::WebServerData;

#[post("/api/tenants/{tenant_id}/integrations/{integration_type}/{integration_id}/assets/create")]
pub async fn create_asset(
    ws_data: Data<WebServerData>,
    path: Path<(String, String, String)>,
    data: Json<Value>,
) -> HttpResponse {
    let (tenant_id, integration_type, integration_id) = path.into_inner();
    if let Some(db) = &ws_data.db {
        // let asset_id = format!("asset#{}#{}#{}", integration_type, integration_id, Uuid::new_v4());
        let asset_id = Uuid::new_v4().to_string();
        let asset_type = data["type"].as_str().unwrap().to_string();
        let vendor_identifier = data["vendorIdentifier"].as_str().unwrap().to_string();
        let attributes = data.get("attributes").unwrap().clone();
        let data = Asset {
            id: None,
            tenant_id,
            asset_id,
            integration_id,
            integration_type,
            vendor_identifier,
            asset_type,
            attributes,
        };

        let asset_detail = db.insert_one(&data).await;
        match asset_detail {
            Ok(_) => HttpResponse::Ok().json(data),
            Err(err) => HttpResponse::InternalServerError().body(err.to_string()),
        }
    } else {
        HttpResponse::InternalServerError().body("No database connection")
    }
}

#[post("/api/tenants/{tenant_id}/integrations/{integration_type}/{integration_id}/devices/create")]
pub async fn create_device(
    ws_data: Data<WebServerData>,
    path: Path<(String, String, String)>,
    data: Json<Value>,
) -> HttpResponse {
    let (tenant_id, integration_type, integration_id) = path.into_inner();
    if let Some(db) = &ws_data.db {
        let device_id = Uuid::new_v4().to_string();
        let device_model = data["deviceModel"].as_str().unwrap().to_string();
        let device_serial = data["deviceSerial"].as_str().unwrap().to_string();
        let attributes = data.get("attributes").unwrap().clone();
        let data = Device {
            id: None,
            tenant_id,
            device_id,
            integration_id,
            integration_type,
            device_serial,
            device_model,
            attributes,
        };

        match db.insert_one(&data).await {
            Ok(_) => HttpResponse::Ok().json(data.display()),
            Err(err) => HttpResponse::InternalServerError().body(err.to_string()),
        }
    } else {
        HttpResponse::InternalServerError().body("No database connection")
    }
}

#[post("/api/tenants/{tenant_id}/integrations/{integration_type}/{integration_id}/assets/{asset_id}/tags/create")]
pub async fn create_asset_tag(
    ws_data: Data<WebServerData>,
    path: Path<(String, String, String, String)>,
    data: Json<Value>,
) -> HttpResponse {
    let (tenant_id, integration_type, integration_id, asset_id) = path.into_inner();
    if let Some(db) = &ws_data.db {
        let asset_tags = data["tags"].as_array().unwrap().clone();

        let sk = format!(
            "asset#{}#{}#{}",
            integration_type.clone(),
            integration_id.clone(),
            asset_id.clone()
        );
        let filter = doc! {"SK": sk, "PK":tenant_id.clone()};

        let asset: Option<Asset> = db.filter_item(Some(filter)).await.unwrap();

        if let Some(mut val) = asset {
            let mut tags = val.attributes["assetTags"].as_array().unwrap().clone();
            for tag in asset_tags.clone() {
                if !tags.contains(&tag) {
                    tags.push(tag.clone());
                }
            }
            if let Value::Object(obj) = &mut val.attributes {
                obj.extend([("assetTags".to_string(), tags.into())]);
            }

            db.update_item::<Asset>(&val.id.unwrap().to_string(), val.clone())
                .await
                .unwrap();
        } else {
            return HttpResponse::NotFound().body("Asset not found")
        }

        let mut ret: Vec<AssetTag> = vec![];
        for tag in asset_tags {
            let asset_tag = AssetTag {
                id: None,
                tag: tag.as_str().unwrap().to_string(),
                tenant_id: tenant_id.clone(),
                asset_id: asset_id.clone(),
                integration_id: integration_id.clone(),
                integration_type: integration_type.clone(),
            };
            db.insert_one(&asset_tag).await.unwrap();
            ret.push(asset_tag);
        }
        HttpResponse::Ok().json(ret)
    } else {
        HttpResponse::InternalServerError().body("No database connection")
    }
}

#[post("/api/tenants/{tenant_id}/integrations/{integration_type}/{integration_id}/devices/{asset_id}/tags/create")]
pub async fn create_device_tag(
    ws_data: Data<WebServerData>,
    path: Path<(String, String, String, String)>,
    data: Json<Value>,
) -> HttpResponse {
    let (tenant_id, integration_type, integration_id, device_id) = path.into_inner();
    if let Some(db) = &ws_data.db {
        let device_tags = data["tags"].as_array().unwrap().clone();

        let sk = format!(
            "device#{}#{}#{}",
            integration_type.clone(),
            integration_id.clone(),
            device_id.clone()
        );

        let filter = doc! {"SK": sk, "PK":tenant_id.clone()};

        let device: Option<Device> = db.filter_item(Some(filter)).await.unwrap();

        if let Some(mut val) = device {
            let mut tags = val.attributes["deviceTags"].as_array().unwrap().clone();
            for tag in device_tags.clone() {
                if !tags.contains(&tag) {
                    tags.push(tag.clone());
                }
            }
            if let Value::Object(obj) = &mut val.attributes {
                obj.extend([("deviceTags".to_string(), tags.into())]);
            }

            db.update_item::<Device>(&val.id.unwrap().to_string(), val.clone())
                .await
                .unwrap();
        } else {
            return HttpResponse::NotFound().body("Device not found")
        }

        let mut ret: Vec<AssetTag> = vec![];
        for tag in device_tags {
            let asset_tag = AssetTag {
                id: None,
                tag: tag.as_str().unwrap().to_string(),
                tenant_id: tenant_id.clone(),
                asset_id: device_id.clone(),
                integration_id: integration_id.clone(),
                integration_type: integration_type.clone(),
            };
            db.insert_one(&asset_tag).await.unwrap();
            ret.push(asset_tag);
        }
        HttpResponse::Ok().json(ret)
    } else {
        HttpResponse::InternalServerError().body("No database connection")
    }
}

#[get("/api/tenants/{tenant_id}/integrations/{integration_type}/{integration_id}/assets")]
pub async fn get_assets(
    ws_data: Data<WebServerData>,
    path: Path<(String, String, String)>,
) -> HttpResponse {
    let (tenant_id, integration_type, integration_id) = path.into_inner();
    if let Some(db) = &ws_data.db {
        let asset_filter = doc! {"PK": tenant_id.clone(), "SK": {"$regex": format!("^asset#{}#{}#", integration_type, integration_id)}};
        let assets: Vec<Asset> = db.filter_items(Some(asset_filter)).await.unwrap();
        let device_filter = doc! {"PK": tenant_id.clone(), "SK": {"$regex": format!("^device#{}#{}#", integration_type, integration_id)}};
        let devices: Vec<Device> = db.filter_items(Some(device_filter)).await.unwrap();
        HttpResponse::Ok().json(json!({"assets": assets.display(), "devices": devices.display()}))
    } else {
        HttpResponse::InternalServerError().body("No database connection")
    }
}

#[get("/api/tenants/{tenant_id}/get-all-tags")]
pub async fn get_all_tags(ws_data: Data<WebServerData>, path: Path<String>) -> HttpResponse {
    let tenant_id = path.into_inner();
    if let Some(db) = &ws_data.db {
        let filter = doc! {"SK1": tenant_id};
        let asset_tags: Vec<AssetTag> = db.filter_items(Some(filter)).await.unwrap();
        let mut only_tag_words: Vec<String> = vec![];
        asset_tags.into_iter().for_each(|tag| {
            if !only_tag_words.contains(&tag.tag) {
                only_tag_words.push(tag.tag);
            }
        });
        HttpResponse::Ok().json(json!({"tags": only_tag_words}))
    } else {
        HttpResponse::InternalServerError().body("No database connection")
    }
}

#[derive(Deserialize, Serialize, Debug, Clone)]
struct AssetByTagParams {
    tags: Vec<String>
}

#[get("/api/tenants/{tenant_id}/assets-by-tags")]
pub async fn get_assets_by_tags(
    ws_data: Data<WebServerData>,
    path: Path<String>,
    req: HttpRequest,
) -> HttpResponse {
    let tenant_id = path.into_inner();
    let mut query_params: HashMap<String, Vec<String>> = HashMap::new();
    req.query_string().split("&").into_iter().for_each(|param| {
        let split: Vec<&str> = param.split("=").collect();
        query_params
            .entry(split[0].to_string())
            .or_insert(vec![])
            .push(urlencoding::decode(split[1]).unwrap().into_owned());
    });

    let tags = query_params.get("tags").unwrap();

    let mut ret = HashMap::new();
    if let Some(db) = &ws_data.db {
        for tag in tags {
            let asset_filter = doc!{"attributes.assetTags": tag.clone(), "PK": tenant_id.clone()};
            let assets: Vec<Asset> = db.filter_items(Some(asset_filter)).await.unwrap();
            let device_filter = doc!{"attributes.deviceTags": tag.clone(), "PK": tenant_id.clone()};
            let devices: Vec<Device> = db.filter_items(Some(device_filter)).await.unwrap();
            ret.entry("assets")
                .or_insert(HashMap::new())
                .insert(tag.clone(), assets.display());
            ret.entry("devices")
                .or_insert(HashMap::new())
                .insert(tag.clone(), devices.display());
        }
        HttpResponse::Ok().json(json!(ret))
    } else {
        HttpResponse::InternalServerError().body("No database connection")
    }
}