mod asset;
mod device;
use serde::{Deserialize, Serialize};

pub use asset::*;
pub use device::*;

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Objects {
    pub assets: Option<Vec<Asset>>,
    pub devices: Option<Vec<Device>>,
}

impl Objects {
    pub fn add_asset(&mut self, asset: Asset) {
        if self.assets.is_none() {
            self.assets = Some(Vec::new());
        }
        self.assets.as_mut().unwrap().push(asset);
    }

    pub fn add_device(&mut self, device: Device) {
        if self.devices.is_none() {
            self.devices = Some(Vec::new());
        }
        self.devices.as_mut().unwrap().push(device);
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SchemaItem {
    pub vendor: String,
    pub asset_type: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum Object {
    Asset(Asset),
    Device(Device),
}
