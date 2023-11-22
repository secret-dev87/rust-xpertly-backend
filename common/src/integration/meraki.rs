use crate::Display;

use super::MerakiIntegration;
use mongodb::bson::oid::ObjectId;
use serde::{de::MapAccess, de::Visitor, ser::SerializeMap, Deserialize, Serialize};
use serde_json::{Value, json};

struct MerakiIntegrationVisitor;

impl Serialize for MerakiIntegration {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut seq = serializer.serialize_map(Some(6))?;
        seq.serialize_entry("PK", &self.tenant_id)?;
        let sk = format!("integration#{}#{}", &self.integration_type.to_string(), &self.integration_id.to_string());
        seq.serialize_entry("SK", &sk)?;

        seq.serialize_entry("apiKey", &self.api_key)?;
        seq.serialize_entry("organization", &self.organization)?;
        if let Some(id) = &self.id {
            seq.serialize_entry("_id", id)?;
        }
        seq.end()
    }
}

impl<'de> Deserialize<'de> for MerakiIntegration {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_map(MerakiIntegrationVisitor)
    }
}

impl<'de> Visitor<'de> for MerakiIntegrationVisitor {
    type Value = MerakiIntegration;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            formatter,
            "a map with keys 'PK', 'SK', 'apiKey', 'organization'"
        )
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut id: Option<ObjectId> = None;
        let mut pk: Option<String> = None;
        let mut sk: Option<String> = None;
        let mut api_key: Option<String> = None;
        let mut organization: Option<String> = None;
        while let Some(ref k) = map.next_key::<String>()? {
            match k.as_str() {
                "PK" => {
                    pk = Some(map.next_value()?);
                },
                "SK" => {
                    sk = Some(map.next_value()?);
                },
                "apiKey" => {
                    api_key = Some(map.next_value()?);
                },
                "organization" => {
                    organization = Some(map.next_value()?);
                },
                "_id" => {
                    id = Some(map.next_value()?);
                },
                k => {
                    return Err(serde::de::Error::custom(&format!("Invalid key: {}", k)))
                }
            }
        }
        if pk.is_none() || sk.is_none() || api_key.is_none() || organization.is_none() {
            return Err(serde::de::Error::custom("-- Missing Attributes -- "));
        }
        let sk = sk.unwrap();
        let sk_splits = sk.split("#").collect::<Vec<&str>>();
        if sk_splits.len() != 3 {
            //sk string contains exact 4 properties
            return Err(serde::de::Error::custom("-- Wrong SK attribute format -- "));
        }

        Ok(MerakiIntegration {
            id: id,
            tenant_id: pk.unwrap(),
            integration_type: "meraki".to_string(),
            integration_id: sk_splits.get(2).unwrap().to_string(),
            api_key: api_key.unwrap(),
            organization: organization.unwrap(),
        })
    }
}

impl Display for MerakiIntegration {
    fn display(&self) -> Value {
        json!({
            "integrationId": self.integration_id,
            "integrationType": self.integration_type,
            "tenantId": self.tenant_id,
            "apiKey": self.api_key,
            "organization": self.organization,
        })
    }
}