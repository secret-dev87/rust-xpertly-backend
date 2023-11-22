use crate::Display;

use super::AnsibleIntegration;
use mongodb::bson::oid::ObjectId;
use serde::{de::MapAccess, de::Visitor, ser::SerializeMap, Deserialize, Serialize};
use serde_json::{json, Value};

struct AnsibleIntegrationVisitor;

impl Serialize for AnsibleIntegration {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut seq = serializer.serialize_map(Some(6))?;
        seq.serialize_entry("PK", &self.tenant_id)?;
        let sk = format!("integration#{}#{}", &self.integration_type.to_string(), &self.integration_id.to_string());
        seq.serialize_entry("SK", &sk)?;

        seq.serialize_entry("ansibleHostname", &self.ansible_hostname)?;
        seq.serialize_entry("username", &self.username)?;
        seq.serialize_entry("password", &self.password)?;
        if let Some(id) = &self.id {
            seq.serialize_entry("_id", id)?;
        }
        seq.end()
    }
}

impl<'de> Deserialize<'de> for AnsibleIntegration {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_map(AnsibleIntegrationVisitor)
    }
}

impl<'de> Visitor<'de> for AnsibleIntegrationVisitor {
    type Value = AnsibleIntegration;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            formatter,
            "a map with keys 'PK', 'SK', 'ansibleHostname', 'username', 'password'"
        )
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut id: Option<ObjectId> = None;
        let mut pk: Option<String> = None;
        let mut sk: Option<String> = None;
        let mut ansible_hostname: Option<String> = None;
        let mut username: Option<String> = None;
        let mut password: Option<String> = None;

        let mut integration_id: Option<String> = None;
        let mut integration_type: Option<String> = None;

        while let Some(ref k) = map.next_key::<String>()? {
            if k == "_id" {
                id = Some(map.next_value()?);
            } else if k == "PK" {
                pk = Some(map.next_value()?);
            } else if k == "SK" {
                sk = Some(map.next_value()?);
            } else if k == "ansibleHostname" {
                ansible_hostname = Some(map.next_value()?);
            } else if k == "username" {
                username = Some(map.next_value()?);
            } else if k == "password" {
                password = Some(map.next_value()?);
            } else if k == "tenantId" {
                pk = Some(map.next_value()?);
            } else if k == "integrationId" {
                integration_id = Some(map.next_value()?);
            } else if k == "integrationType" {
                integration_type = Some(map.next_value()?);
            } else {
                return Err(serde::de::Error::custom(&format!("Invalid key: {}", k)));
            }
        }

        if pk.is_none()
            || ansible_hostname.is_none()
            || username.is_none()
            || password.is_none()
        {
            return Err(serde::de::Error::custom("-- Missing Attributes -- "));
        }

        if sk.is_some() {
            let sk = sk.unwrap();
            let sk_splits = sk.split("#").collect::<Vec<&str>>();
            if sk_splits.len() != 3 {
                //sk string contains exact 4 properties
                return Err(serde::de::Error::custom("-- Wrong SK attribute format -- "));
            }

            Ok(AnsibleIntegration {
                id: id,
                tenant_id: pk.unwrap(),
                integration_type: "ansible".to_string(),
                integration_id: sk_splits.get(2).unwrap().to_string(),
                ansible_hostname: ansible_hostname.unwrap(),
                username: username.unwrap(),
                password: password.unwrap(),
            })
        } else if let Some(integration_id) = integration_id {
            if let Some(integration_type) = integration_type {
                Ok(AnsibleIntegration {
                    id: id,
                    tenant_id: pk.unwrap(),
                    integration_type: integration_type,
                    integration_id: integration_id,
                    ansible_hostname: ansible_hostname.unwrap(),
                    username: username.unwrap(),
                    password: password.unwrap(),
                })
            } else {
                Err(serde::de::Error::custom("-- Missing integrationType -- "))
            }
        } else {
            Err(serde::de::Error::custom("-- Missing integrationId -- "))
        }
    }
}

impl Display for AnsibleIntegration {
    fn display(&self) -> Value {
        json!({
            "integrationId": self.integration_id,
            "integrationType": self.integration_type,
            "tenantId": self.tenant_id,
            "ansibleHostname": self.ansible_hostname,
            "username": self.username,
            "password": self.password,
        })
    }
}