use actix_web::{post, get, put, delete, web::{Data, Json, Path}, HttpResponse};
use serde_json::json;
use xpertly_common::{TestUser, Asset};
use mongodb::bson::oid::ObjectId;
use crate::WebServerData;


#[post("/user")]
pub async fn create_user(ws_data: Data<WebServerData>, new_user: Json<TestUser>) -> HttpResponse {
    if let Some(db) = &ws_data.db {
        let data = TestUser {
            id: None,
            name: new_user.name.to_owned(),
            location: new_user.location.to_owned(),
            title: new_user.title.to_owned()
        };
    
        let user_detail = db.insert_one(&data).await;
        match user_detail {
            Ok(user) => HttpResponse::Ok().json(user),
            Err(err) => HttpResponse::InternalServerError().body(err.to_string()),
        }
    } else {
        HttpResponse::InternalServerError().body("No database connection")
    }

}

#[get("/user/{id}")]
pub async fn get_user(ws_data: Data<WebServerData>, path: Path<String>) -> HttpResponse {
    if let Some(db) = &ws_data.db {
        let id = path.into_inner();
        if id.is_empty() {
            return HttpResponse::BadRequest().body("invaild ID");
        }

        let user_detail = db.find_by_id::<TestUser>(&id).await;
        match user_detail {
            Ok(user) => HttpResponse::Ok().json(user),
            Err(err) => HttpResponse::InternalServerError().body(err.to_string()),
        }
    } else {
        HttpResponse::InternalServerError().body("No database connection")
    }
}

#[put("/user/{id}")]
pub async fn update_user(ws_data: Data<WebServerData>, path: Path<String>, new_user: Json<TestUser>) -> HttpResponse {
    if let Some(db) = &ws_data.db {
        let id = path.into_inner();
        if id.is_empty() {
            return HttpResponse::BadRequest().body("invaild ID");
        }

        let data = TestUser {
            id: Some(ObjectId::parse_str(&id).unwrap()),
            name: new_user.name.to_owned(),
            location: new_user.location.to_owned(),
            title: new_user.title.to_owned()
        };

        let update_result = db.update_item::<TestUser>(&id, data).await;
        match update_result {
            Ok(update) => {
                if update.matched_count == 1 {
                    let updated_user_info = db.find_by_id::<TestUser>(&id).await;

                    return match updated_user_info {
                        Ok(user) => HttpResponse::Ok().json(user),
                        Err(err) => HttpResponse::InternalServerError().body(err.to_string()),
                    };
                } else {
                    return HttpResponse::NotFound().body("No user found with specified ID");
                }
            }
            Err(err) => HttpResponse::InternalServerError().body(err.to_string()),
        }
    } else {
        HttpResponse::InternalServerError().body("No database connection")
    }
}

#[delete("/users")]
pub async fn delete_users(ws_data: Data<WebServerData>, ids: Json<Vec<String>>) -> HttpResponse {  
    if let Some(db) = &ws_data.db {
        let user_detail = db.delete_items::<TestUser>(ids.to_vec()).await;
        match user_detail {
            Ok(user) => HttpResponse::Ok().json(user),
            Err(err) => HttpResponse::InternalServerError().body(err.to_string()),
        }
    } else {
        HttpResponse::InternalServerError().body("No database connection")
    }
}

#[get("/mongo")]
pub async fn test_mongo(ws_data: Data<WebServerData>) -> HttpResponse {  
    // let ids = vec!["641883fdb953646d667a144f", "64188404b953646d667a1450"];
    // let user_detail = ws_data.db.delete_items::<TestUser>(ids).await;
    // match user_detail {
    //     Ok(user) => HttpResponse::Ok().json(user),
    //     Err(err) => HttpResponse::InternalServerError().body(err.to_string()),
    // }
    unimplemented!()
}


#[post("/asset")]
pub async fn create_asset(ws_data: Data<WebServerData>, new_asset: Json<Asset>) -> HttpResponse {
    println!("{:?}", new_asset);
    if let Some(db) = &ws_data.db {
        let data = Asset {
            id: None,
            ..new_asset.0
        };
    
        let asset_detail = db.insert_one(&data).await;
        match asset_detail {
            Ok(asset) => HttpResponse::Ok().json(asset),
            Err(err) => HttpResponse::InternalServerError().body(err.to_string()),
        }
    } else {
        HttpResponse::InternalServerError().body("No database connection")
    }

}
