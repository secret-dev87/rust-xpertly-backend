use serde::{Serialize, Deserialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize)]
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

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKey {
    pub key: String,
    pub date_created: String,
    pub last_used: String
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tenant {
    pub child_tenant_id: String,
    pub child_tenant_name: String
}

#[derive(Serialize, Deserialize)]
pub struct Tours {
    pub editor: bool
}