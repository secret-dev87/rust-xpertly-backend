use crate::Display;
use mongo_api::MongoDbModel;
use mongo_derive::MongoModel;
use mongodb::bson::oid::ObjectId;
use serde::{de::MapAccess, de::Visitor, ser::SerializeMap, Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, MongoModel)]
pub struct Device {
    pub id: Option<ObjectId>,
    pub tenant_id: String,
    pub device_id: String,
    pub device_serial: String,
    pub device_model: String,
    pub integration_id: String,
    pub integration_type: String,
    pub attributes: serde_json::Value,
}

impl Display for Device {
    fn display(&self) -> Value {
        json!({
            "tenantId": self.tenant_id,
            "deviceId" : self.device_id,
            "deviceSerial" : self.device_serial,
            "deviceModel" : self.device_model,
            "integrationId" : self.integration_id,
            "integrationType" : self.integration_type,
            "attributes": self.attributes
        })
    }
}

struct DeviceVisitor;

impl Serialize for Device {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut seq = serializer.serialize_map(Some(8))?;
        seq.serialize_entry("PK", &self.tenant_id)?;
        let sk = format!(
            "device#{}#{}#{}",
            &self.integration_type, &self.integration_id, &self.device_id
        );
        seq.serialize_entry("SK", &sk)?;
        seq.serialize_entry("SK1", &self.device_serial)?;
        seq.serialize_entry("SK2", &self.device_model)?;
        seq.serialize_entry("attributes", &self.attributes)?;
        if let Some(id) = &self.id {
            seq.serialize_entry("_id", id)?;
        }
        seq.end()
    }
}

impl<'de> Deserialize<'de> for Device {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_map(DeviceVisitor)
    }
}

impl<'de> Visitor<'de> for DeviceVisitor {
    type Value = Device;
    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            formatter,
            "a map with keys 'PK', 'SK', 'attributes', 'SK1', 'SK2'"
        )
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut id: Option<ObjectId> = None;
        let mut pk: Option<String> = None;
        let mut sk: Option<String> = None;
        let mut attributes: Option<Value> = None;
        let mut sk1: Option<String> = None;
        let mut sk2: Option<String> = None;

        let mut device_id:Option<String> = None;
        let mut integration_id: Option<String> = None;
        let mut integration_type: Option<String> = None;

        while let Some(ref k) = map.next_key::<String>()? {
            if k == "_id" {
                id = Some(map.next_value()?);
            } else if k == "PK" {
                pk = Some(map.next_value()?);
            } else if k == "SK" {
                sk = Some(map.next_value()?);
            } else if k == "SK1" {
                sk1 = Some(map.next_value()?);
            } else if k == "SK2" {
                sk2 = Some(map.next_value()?);
            } else if k == "attributes" {
                attributes = Some(map.next_value()?);
            } else if k == "tenantId" {
                pk = Some(map.next_value()?);
            } else if k == "deviceId" {
                device_id = Some(map.next_value()?);
            } else if k == "deviceSerial" {
                sk1 = Some(map.next_value()?);
            } else if k == "deviceModel" {
                sk2 = Some(map.next_value()?);
            } else if k == "integrationId" {
                integration_id = Some(map.next_value()?);
            } else if k == "integrationType" {
                integration_type = Some(map.next_value()?);
            } else {
                return Err(serde::de::Error::custom(&format!("Invalid key: {}", k)));
            }
        }

        if pk.is_none() || sk1.is_none() || sk2.is_none() || attributes.is_none() {
            return Err(serde::de::Error::custom("-- Missing Attributes -- "));
        }
        
        if sk.is_some() {
            let sk = sk.unwrap();

            let sk_splits = sk.split("#").collect::<Vec<&str>>();
            if sk_splits.len() != 4 {
                //sk string contains exact 4 properties
                return Err(serde::de::Error::custom("-- Wrong SK attribute format -- "));
            }
    
            Ok(Device {
                id: id,
                tenant_id: pk.unwrap(),
                device_id: sk_splits.get(3).unwrap().to_string(),
                integration_id: sk_splits.get(2).unwrap().to_string(),
                integration_type: sk_splits.get(1).unwrap().to_string(),
                device_serial: sk1.unwrap(),
                device_model: sk2.unwrap(),
                attributes: attributes.unwrap(),
            })
        } else if let Some(device_id) = device_id {
            if let Some(integration_id) = integration_id {
                if let Some(integration_type) = integration_type {
                    Ok(Device {
                        id: id,
                        tenant_id: pk.unwrap(),
                        device_id: device_id,
                        integration_id: integration_id,
                        integration_type: integration_type,
                        device_serial: sk1.unwrap(),
                        device_model: sk2.unwrap(),
                        attributes: attributes.unwrap(),
                    })
                } else {
                    return Err(serde::de::Error::custom("-- Missing integrationType -- "));
                }
            } else {
                return Err(serde::de::Error::custom("-- Missing integrationId -- "));
            }
        } else {
            return Err(serde::de::Error::custom("-- Missing SK components -- "));
        }

    }
}
