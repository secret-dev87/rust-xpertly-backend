use super::{Object, Objects, SchemaItem};
use crate::Display;
use mongo_api::MongoDbModel;
use mongo_derive::MongoModel;
use mongodb::bson::oid::ObjectId;
use serde::{de::MapAccess, de::Visitor, ser::SerializeMap, Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;

//Asset Customization
#[derive(Debug, Clone, MongoModel)]
pub struct Asset {
    pub id: Option<ObjectId>,
    pub tenant_id: String,
    pub asset_id: String,
    pub integration_id: String,
    pub integration_type: String,
    pub vendor_identifier: String,
    pub asset_type: String,
    pub attributes: serde_json::Value,
}

impl Display for Asset {
    fn display(&self) -> Value {
        json!({
            "tenantId": self.tenant_id,
            "assetId" : self.asset_id,
            "integrationId" : self.integration_id,
            "integrationType" : self.integration_type,
            "vendorIdentifier" : self.vendor_identifier,
            "assetType" : self.asset_type,
            "attributes": self.attributes
        })
    }
}

struct AssetVisitor;

impl Serialize for Asset {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut seq = serializer.serialize_map(Some(6))?;
        seq.serialize_entry("PK", &self.tenant_id)?;
        let sk = format!(
            "asset#{}#{}#{}",
            &self.integration_type, &self.integration_id, &self.asset_id
        );
        seq.serialize_entry("SK", &sk)?;
        seq.serialize_entry("SK1", &self.vendor_identifier)?;
        seq.serialize_entry("SK2", &self.asset_type)?;
        seq.serialize_entry("attributes", &self.attributes)?;
        if let Some(id) = &self.id {
            seq.serialize_entry("_id", id)?;
        }
        seq.end()
    }
}

impl<'de> Deserialize<'de> for Asset {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_map(AssetVisitor)
    }
}

impl<'de> Visitor<'de> for AssetVisitor {
    type Value = Asset;
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
            } else {
                return Err(serde::de::Error::custom(&format!("Invalid key: {}", k)));
            }
        }

        if pk.is_none() || sk.is_none() || sk1.is_none() || sk2.is_none() || attributes.is_none() {
            return Err(serde::de::Error::custom("-- Missing Attributes -- "));
        }
        let sk = sk.unwrap();
        let sk_splits = sk.split("#").collect::<Vec<&str>>();
        if sk_splits.len() != 4 {
            //sk string contains exact 4 properties
            return Err(serde::de::Error::custom("-- Wrong SK attribute format -- "));
        }

        Ok(Asset {
            id: id,
            tenant_id: pk.unwrap(),
            asset_id: sk_splits.get(3).unwrap().to_string(),
            integration_id: sk_splits.get(2).unwrap().to_string(),
            integration_type: sk_splits.get(1).unwrap().to_string(),
            vendor_identifier: sk1.unwrap(),
            asset_type: sk_splits.get(0).unwrap().to_string(),
            attributes: attributes.unwrap(),
        })
    }
}

//AssetTag customization
#[derive(Debug, Clone, MongoModel)]
pub struct AssetTag {
    pub id: Option<ObjectId>,
    pub tag: String,
    pub tenant_id: String,
    pub asset_id: String,
    pub integration_id: String,
    pub integration_type: String,
}

struct AssetTagVisitor;

impl Display for AssetTag {
    fn display(&self) -> Value {
        json!({
            "tenantId": self.tenant_id,
            "tag" : self.tag,
            "assetId" : self.asset_id,
            "integrationId" : self.integration_id,
            "integrationType" : self.integration_type
        })
    }
}

impl Serialize for AssetTag {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut seq = serializer.serialize_map(Some(4))?;
        let pk = format!(
            "{}#{}#{}#{}",
            &self.tenant_id, &self.integration_type, &self.integration_id, &self.asset_id
        );
        seq.serialize_entry("PK", &pk)?;
        seq.serialize_entry("SK", &format!("tag#{}", &self.tag))?;
        seq.serialize_entry("SK1", &self.tenant_id)?;

        if let Some(id) = &self.id {
            seq.serialize_entry("_id", id)?;
        }
        seq.end()
    }
}

impl<'de> Deserialize<'de> for AssetTag {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_map(AssetTagVisitor)
    }
}

impl<'de> Visitor<'de> for AssetTagVisitor {
    type Value = AssetTag;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(formatter, "a map with keys 'PK', 'SK', 'SK1'")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut id: Option<ObjectId> = None;
        let mut pk: Option<String> = None;
        let mut sk: Option<String> = None;
        let mut sk1: Option<String> = None;
        while let Some(ref k) = map.next_key::<String>()? {
            if k == "_id" {
                id = Some(map.next_value()?);
            } else if k == "PK" {
                pk = Some(map.next_value()?);
            } else if k == "SK" {
                sk = Some(map.next_value()?);
            } else if k == "SK1" {
                sk1 = Some(map.next_value()?);
            } else {
                return Err(serde::de::Error::custom(&format!("Invalid key: {}", k)));
            }
        }

        if pk.is_none() || sk.is_none() || sk1.is_none() {
            return Err(serde::de::Error::custom("-- Missing Attributes -- "));
        }
        let pk = pk.unwrap();
        let pk_splits = pk.split("#").collect::<Vec<&str>>();
        if pk_splits.len() != 4 {
            //sk string contains exact 4 properties
            return Err(serde::de::Error::custom("-- Wrong SK attribute format -- "));
        }

        let sk = sk.unwrap();
        let sk_splits = sk.split("#").collect::<Vec<&str>>();
        if sk_splits.len() != 2 {
            //sk string contains exact 4 properties
            return Err(serde::de::Error::custom("-- Wrong SK attribute format -- "));
        }

        Ok(AssetTag {
            id: id,
            tag: sk_splits.get(1).unwrap().to_string(),
            tenant_id: pk_splits.get(0).unwrap().to_string(),
            asset_id: pk_splits.get(3).unwrap().to_string(),
            integration_id: pk_splits.get(1).unwrap().to_string(),
            integration_type: pk_splits.get(2).unwrap().to_string(),
        })
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Assets {
    pub schema: Option<Vec<SchemaItem>>,
    pub objects: Option<HashMap<String, Objects>>,
}

impl Assets {
    pub fn add_object(&mut self, tag: &str, obj: Object) {
        if self.objects.is_none() {
            self.objects = Some(HashMap::new());
        }
        let objects = self
            .objects
            .as_mut()
            .unwrap()
            .entry(tag.to_string())
            .or_insert(Objects {
                assets: None,
                devices: None,
            });

        match obj {
            Object::Asset(asset) => objects.add_asset(asset),
            Object::Device(device) => objects.add_device(device),
        }
    }

    pub fn new() -> Assets {
        Assets {
            schema: Some(Vec::<SchemaItem>::new()),
            objects: Some(HashMap::new()),
        }
    }

    // pub fn from_lists(assets: Option<Vec<Value>>, devices: Option<Vec<Value>>) -> Assets {
    //     let mut assets_objects = vec![];
    //     let objects = Objects {
    //         assets: {
    //             if let Some(assets) = assets {
    //                 Some(vec![assets.iter().map(|asset| serde_json::from_value::<Asset>(asset).unwrap()).collect()])
    //             } else {
    //                 None
    //             }
    //         },
    //         devices: None,
    //     };

    //     if let Some(devices) = devices {
    //         for device in devices {
    //             assets_objects.push(Object::Device(serde_json::from_value(device).unwrap()));
    //         }
    //     }
    //     Assets {
    //         schema: Some(Vec::<SchemaItem>::new()),
    //         objects: Some(HashMap::new().insert("tag", assets_objects)),
    //     }
    // }
}
