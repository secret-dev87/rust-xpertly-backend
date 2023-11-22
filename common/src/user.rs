use serde::{Serialize, Deserialize};
use uuid::Uuid;
use mongo_derive::MongoModel;
use mongo_api::MongoDbModel;
use mongodb::bson::oid::ObjectId;
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub org_id: String,
    pub user_id: Uuid,
    pub first_name: String,
    pub last_name: String,
    pub user_email: String,
    pub role: String,
    pub identity_id: String,
    pub api_keys: Option<Vec<ApiKey>>,
    pub child_tenants: Vec<Tenant>,
    pub tours: Tours,
    pub new_user: bool,
    pub notif_key: String
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ApiKey {
    pub key: String,
    pub date_created: String,
    pub last_used: String
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Tenant {
    pub child_tenant_id: String,
    pub child_tenant_name: String
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Tours {
    pub editor: bool
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AvicennaUser {
    pub tenant_id: Uuid,
    pub tenant_name: String,
    pub user_id: Uuid,
    pub first_name: String,
    pub last_name: String,
    pub user_email: String,
    pub xpertly_executions: XpertlyExecutions,
    pub role: UserRole
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct XpertlyExecutions {
    pub count: Option<i64>,
    pub quota: Option<i64>
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub enum UserRole {
    Owner,
    Admin,
    Engineer,
    User   
}

#[derive(Debug, Serialize, Deserialize)]
#[derive(MongoModel)]
pub struct TestUser {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    pub name: String,
    pub location: String,
    pub title: String,
}