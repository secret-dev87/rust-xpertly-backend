use std::f32::consts::E;

use crate::Display;

use super::SplunkIntegration;
use mongodb::bson::oid::ObjectId;
use serde::{de::MapAccess, de::Visitor, ser::SerializeMap, Deserialize, Serialize};
use serde_json::{json, Value};

struct SplunkIntegrationVisitor;

impl Serialize for SplunkIntegration {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut seq = serializer.serialize_map(Some(6))?;
        seq.serialize_entry("PK", &self.tenant_id)?;
        let sk = format!("integration#{}#{}", &self.integration_type.to_string(), &self.integration_id.to_string());
        seq.serialize_entry("SK", &sk)?;

        seq.serialize_entry("hostname", &self.hostname)?;
        seq.serialize_entry("port", &self.port)?;
        seq.serialize_entry("hecToken", &self.hec_token)?;
        if let Some(id) = &self.id {
            seq.serialize_entry("_id", id)?;
        }
        seq.end()
    }
}

impl<'de> Deserialize<'de> for SplunkIntegration {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_map(SplunkIntegrationVisitor)
    }
}

impl<'de> Visitor<'de> for SplunkIntegrationVisitor {
    type Value = SplunkIntegration;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            formatter,
            "a map with keys 'PK', 'SK', 'hostname', 'port', 'hecToken'"
        )
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut id: Option<ObjectId> = None;
        let mut pk: Option<String> = None;
        let mut sk: Option<String> = None;
        let mut hostname: Option<String> = None;
        let mut port: Option<String> = None;
        let mut hec_token: Option<String> = None;

        let mut integration_id: Option<String> = None;
        let mut integration_type: Option<String> = None;

        while let Some(ref k) = map.next_key::<String>()? {
            if k == "_id" {
                id = Some(map.next_value()?);
            } else if k == "PK" {
                pk = Some(map.next_value()?);
            } else if k == "SK" {
                sk = Some(map.next_value()?);
            } else if k == "hostname" {
                hostname = Some(map.next_value()?);
            } else if k == "port" {
                port = Some(map.next_value()?);
            } else if k == "hecToken" {
                hec_token = Some(map.next_value()?);
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
            || hostname.is_none()
            || port.is_none()
            || hec_token.is_none()
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
    
            Ok(SplunkIntegration {
                id: id,
                tenant_id: pk.unwrap(),
                integration_type: "splunk".to_string(),
                integration_id: sk_splits.get(2).unwrap().to_string(),
                hostname: hostname.unwrap(),
                port: port.unwrap(),
                hec_token: hec_token.unwrap(),
            })
        } else if let Some(integration_id) = integration_id {
            if let Some(integration_type) = integration_type {
                Ok(SplunkIntegration {
                    id: id,
                    tenant_id: pk.unwrap(),
                    integration_type: integration_type,
                    integration_id: integration_id,
                    hostname: hostname.unwrap(),
                    port: port.unwrap(),
                    hec_token: hec_token.unwrap(),
                })
            } else {
                Err(serde::de::Error::custom("-- Missing integrationType -- "))
            }
        } else {
            Err(serde::de::Error::custom("-- Missing integrationId -- "))
        }
    }
}

impl Display for SplunkIntegration {
    fn display(&self) -> Value {
        json!({
            "integrationId": self.integration_id,
            "integrationType": self.integration_type,
            "tenantId": self.tenant_id,
            "hostname": self.hostname,
            "port": self.port,
            "hecToken": self.hec_token,
        })
    }
}
