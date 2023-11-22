use actix::{Actor, Addr, Recipient, StreamHandler};
use actix_web::{
    get, middleware::Logger, post, web::Data, web::Json, web::Path, web::Payload, App, HttpServer,
    Responder,
};
use actix_web::{HttpRequest, HttpResponse};
use actix_web_actors::ws;
use env_logger;
use jsonwebtoken::{decode, DecodingKey, Validation};
use mongo_api::MongoDbClient;
use reqwest::header::{HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::str::FromStr;
use uuid::Uuid;
use xpertly_common::{AvicennaUser, TestUser, WorkerConfig};

mod auth;
use auth::extractor::Authenticated;
use auth::middleware::{AuthenticateMiddlewareFactory, XpertlyJwk};

// use websockets::live_updates::LiveUpdateWsActor;
use actix_ws::{handle, Message};
use xpertly_worker::WorkerLog;

mod websockets;
use dotenv::dotenv;
use std::env;
use websockets::server::LiveUpdateServer;
use websockets::session::WsActor;

mod test_api;
use test_api::{create_user, get_user, update_user};

mod assets;
use assets::*;

mod integrations;
use integrations::*;

type ClientSocket = Recipient<WorkerLog>;
#[derive(Clone)]
pub struct WebServerData {
    pub ws_server: Addr<websockets::server::LiveUpdateServer>,
    pub db: Option<MongoDbClient>,
}

// #[get("/test/{name}")]
// async fn test(name: Path<String>) -> impl Responder {
//     // println!("{}", auth.claims.username);
//     format!("Hello {}!", name)
// }
#[derive(Deserialize)]
struct TriggerRequest {
    tags: Vec<String>,
    worker: WorkerConfig,
    exe_id: Option<Uuid>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ResumeWorker {
    pub token: String,
    pub custom_output: Option<Value>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CancelWorker {
    pub token: String,
    pub message: Option<Value>
}

#[get("/ws/{execution_id}")]
async fn ws_index(
    id: Path<Uuid>,
    req: HttpRequest,
    body: Payload,
    ws_data: Data<WebServerData>,
) -> Result<HttpResponse, actix_web::Error> {
    let srv_addr = ws_data.ws_server.clone();
    let resp = ws::start(
        WsActor {
            id: id.into_inner(),
            srv_addr,
        },
        &req,
        body,
    );
    resp
}

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    id: String,
    auth: String,
    exp: usize,
}

#[post("/api/resume")]
async fn resume(resume_req: Json<ResumeWorker>, srv_data: Data<WebServerData>) -> impl Responder {
    let ws_addr = srv_data.ws_server.clone();
    // this is a temporary solution to validate the design, secret should not be hardcoded
    let decode_key = DecodingKey::from_secret("wow much secret".as_ref());
    let validation = Validation::default();
    let token = decode::<Claims>(&resume_req.token, &decode_key, &validation).unwrap();
    dbg!(&token);
    let client = reqwest::Client::new();
    let suspended_worker: serde_json::Value = client
        .get(format!(
            "https://api.dev.xpertly.io/v1/client/get_handler_payload/{}",
            token.claims.id
        ))
        .header(
            HeaderName::from_str("Authorization").unwrap(),
            HeaderValue::from_str(token.claims.auth.as_str()).unwrap(),
        )
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    dbg!(&suspended_worker);

    let suspended_worker_inv =
        xpertly_worker::WorkerInvocation::from_suspended(suspended_worker).unwrap();
    dbg!("Resuming Worker");

    // spawn a thread to complete worker execution and return from this endpoint immediately,
    // leaving the worker execution going in a detached thread.
    
    suspended_worker_inv.resume(
            &resume_req.custom_output.as_ref().unwrap(),
            Some(ws_addr.recipient()),
        )
        .await;

    HttpResponse::Ok().json(json!({"message": "successfully resumed worker"}))
}

#[post("/api/cancel")]
async fn cancel(cancel_req: Json<CancelWorker>, srv_data: Data<WebServerData>) -> impl Responder {
    let ws_addr = srv_data.ws_server.clone();
    // this is a temporary solution to validate the design, secret should not be hardcoded
    let decode_key = DecodingKey::from_secret("wow much secret".as_ref());
    let validation = Validation::default();
    let token = decode::<Claims>(&cancel_req.token, &decode_key, &validation).unwrap();
    dbg!(&token);
    let client = reqwest::Client::new();
    let suspended_worker: serde_json::Value = client
        .get(format!(
            "https://api.dev.xpertly.io/v1/client/get_handler_payload/{}",
            token.claims.id
        ))
        .header(
            HeaderName::from_str("Authorization").unwrap(),
            HeaderValue::from_str(token.claims.auth.as_str()).unwrap(),
        )
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    dbg!(&suspended_worker);

    let suspended_worker_inv =
        xpertly_worker::WorkerInvocation::from_suspended(suspended_worker).unwrap();
    dbg!("Cancelling Worker");

    // spawn a thread to complete worker execution and return from this endpoint immediately,
    // leaving the worker execution going in a detached thread.
    suspended_worker_inv
        .cancel(
            &cancel_req.message.as_ref().unwrap(),
            Some(ws_addr.recipient()),
        ).await;

    HttpResponse::Ok().json(json!({"message": "successfully cancelled worker"}))
}
#[post("/api/tenants/{tenant_id}/workers/{worker_id}/trigger")]
async fn trigger(
    req: HttpRequest,
    ids: Path<(Uuid, Uuid)>,
    trigger: Json<TriggerRequest>,
    auth: Authenticated,
    ws_srv: Data<WebServerData>,
) -> impl Responder {
    let ws_addr = ws_srv.ws_server.clone();
    let worker = xpertly_worker::Worker::from_config(&trigger.worker).unwrap();

    // TODO: don't take exe id from client
    let exe_id = trigger.exe_id.unwrap_or(Uuid::new_v4());
    let client = reqwest::Client::new();
    let (tenant_id, worker_id) = ids.into_inner();
    let user_response = client
        .get(format!(
            "https://api.dev.avicenna.io/v1/tenants/{tenant_id}/users/{user_id}",
            tenant_id = tenant_id,
            user_id = auth.claims.username
        ))
        .header("Authorization", format!("Bearer {}", auth.token))
        .send()
        .await
        .unwrap();

    let resp_json = user_response.json::<Value>().await.unwrap();
    let user = serde_json::from_value::<AvicennaUser>(resp_json).unwrap();

    // response
    std::thread::spawn(move || {
        xpertly_worker::execute(
            &trigger.tags,
            worker,
            user,
            &auth.token,
            exe_id,
            Some(ws_addr.recipient()),
        );
    });

    HttpResponse::Ok().json(json!({ "executionId": exe_id }))
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));
    dotenv().ok();

    let client = reqwest::Client::new();
    let jwks: serde_json::Value = client.get("https://cognito-idp.ap-southeast-2.amazonaws.com/ap-southeast-2_rf7hpngbY/.well-known/jwks.json").send().await.unwrap().json().await.unwrap();
    dbg!(&jwks);
    let jwks = serde_json::from_value::<Vec<XpertlyJwk>>(jwks["keys"].clone()).unwrap();

    let ws_server = LiveUpdateServer::new().start();

    let uri = match env::var("MONGOURI") {
        Ok(v) => Some(v.to_string()),
        Err(_) => {
            println!("No MONGO URI set, not initializing db");
            None
        }
    };

    let server_data = if let Some(uri) = &uri {
        let db = MongoDbClient::init(&uri, "rustDB").await;
        WebServerData {
            ws_server,
            db: Some(db),
        }
    } else {
        WebServerData {
            ws_server,
            db: None,
        }
    };

    HttpServer::new(move || {
        App::new()
            .app_data(Data::new(server_data.clone()))
            .wrap(Logger::default())
            .wrap(AuthenticateMiddlewareFactory::new(jwks.clone()))
            .service(trigger)
            .service(ws_index)
            .service(resume)
            .service(cancel)
            // .service(test)
            // .service(get_user)
            // .service(update_user)
            // .service(create_user)
            .service(create_asset)
            .service(create_device)
            .service(get_assets)
            .service(create_asset_tag)
            .service(create_device_tag)
            .service(get_all_tags)
            .service(get_assets_by_tags)
            .service(create_integration)
            .service(get_integrations)
            .service(get_integration)
    })
    .bind(("0.0.0.0", 8000))?
    .run()
    .await?;

    println!("server started on port 8000");
    Ok(())
}
