use anyhow::{anyhow, Result};
use mongo_api::MongoDbModel;
use mongo_derive::MongoModel;
use mongodb::bson::oid::ObjectId;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use crate::Display;

pub mod ansible;
pub mod dnac;
pub mod meraki;
pub mod splunk;
pub mod viptela;

#[derive(Serialize, Deserialize, Debug, Clone, MongoModel)]
#[serde(untagged)]
pub enum Integration {
    Meraki(MerakiIntegration),
    Ansible(AnsibleIntegration),
    Splunk(SplunkIntegration),
    Dnac(DnacIntegration),
    Viptela(ViptelaIntegration),
}

impl Integration {
    pub fn new(integration: serde_json::Value) -> Result<Self> {
        let integration_type = integration["integrationType"].as_str();
        if let Some(vendor) = integration_type {
            match vendor {
                "meraki" => Ok(Integration::Meraki(
                    serde_json::from_value(integration).unwrap(),
                )),
                "ansible" => Ok(Integration::Ansible(
                    serde_json::from_value(integration).unwrap(),
                )),
                "splunk" => Ok(Integration::Splunk(
                    serde_json::from_value(integration).unwrap(),
                )),
                "dnac" => Ok(Integration::Dnac(
                    serde_json::from_value(integration).unwrap(),
                )),
                "viptela" => Ok(Integration::Viptela(
                    serde_json::from_value(integration).unwrap(),
                )),
                other => Err(anyhow!("expected a valid vendor, got {}", other)),
            }
        } else {
            Err(anyhow!("no vendor found"))
        }
    }
}

impl Display for Integration {
    fn display(&self) -> Value {
        match self {
            Integration::Meraki(integration) => integration.display(),
            Integration::Ansible(integration) => integration.display(),
            Integration::Splunk(integration) => integration.display(),
            Integration::Dnac(integration) => integration.display(),
            Integration::Viptela(integration) => integration.display(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct MerakiIntegration {
    pub id: Option<ObjectId>,
    pub tenant_id: String,
    pub integration_type: String,
    pub integration_id: String,
    pub api_key: String,
    pub organization: String,
}

#[derive(Debug, Clone)]
pub struct AnsibleIntegration {
    pub id: Option<ObjectId>,
    pub tenant_id: String,
    pub integration_type: String,
    pub integration_id: String,
    pub ansible_hostname: String,
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone)]
pub struct SplunkIntegration {
    pub id: Option<ObjectId>,
    pub tenant_id: String,
    pub integration_type: String,
    pub integration_id: String,
    pub hostname: String,
    pub port: String,
    pub hec_token: String,
}

#[derive(Debug, Clone)]
pub struct DnacIntegration {
    pub id: Option<ObjectId>,
    pub tenant_id: String,
    pub integration_type: String,
    pub integration_id: String,
    pub dnac_hostname: String,
    pub port: String,
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone)]
pub struct ViptelaIntegration {
    pub id: Option<ObjectId>,
    pub tenant_id: String,
    pub integration_type: String,
    pub integration_id: String,
    pub v_manage_hostname: String,
    pub username: String,
    pub password: String
}
