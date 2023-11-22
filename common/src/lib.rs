pub mod asset;
pub mod auth;
pub mod user;
pub mod worker;
pub mod integration;

pub use integration::*;
pub use asset::*;
pub use auth::*;
use serde_json::{json, Value};
pub use user::*;
pub use worker::*;

pub trait Display {
    fn display(&self) -> Value;
}

impl<T: Display> Display for Vec<T> {
    fn display(&self) -> Value {
        let ret: Vec<Value> = self.into_iter().map(|item| item.display()).collect();
        json!(ret)
    }
}
