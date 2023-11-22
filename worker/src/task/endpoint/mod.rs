pub mod auth;
use anyhow::{Result, bail};
use handlebars::Handlebars;
use http::Method;
use regex::Regex;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::iter::{FromIterator};
use std::str::FromStr;
use url::Url;
use uuid::Uuid;
use xpertly_common::{Header, Integration};

use crate::{WorkerInvocation};
use auth::InjectAuth;

// #[derive(Serialize, Deserialize, Debug, Clone)]
// pub struct Header {
//     key: String,
//     value: String,
// }

// impl Header {
//     fn as_tuple(&self) -> (String, String) {
//         (String::from(&self.key), String::from(&self.value))
//     }
// }

// #[derive(Serialize, Deserialize, Debug)]
// #[serde(rename_all = "camelCase")]
// struct EndpointFields {
//     method: String,
//     headers: Option<Vec<Header>>,
//     path_params: Option<HashMap<String, String>>,
//     query_params: Option<HashMap<String, String>>,
//     body: Option<Value>,
//     target_url: String,
// }

// #[derive(Serialize, Deserialize, Debug)]
// #[serde(rename_all = "camelCase")]
// pub struct EndpointOld {
//     pub name: String,
//     pub vendor: String,
//     #[serde(rename = "taskId")]
//     pub id: Uuid,
//     pub integration_id: Uuid,
//     pub react_id: String,
//     pub category: Option<String>,
//     pub description: String,
//     fields: Arc<Mutex<EndpointFields>>,
//     #[serde(skip_deserializing, skip_serializing)]
//     #[serde(default = "Uuid::new_v4")]
//     pub run_id: Uuid,
//     pub assets: Arc<Mutex<Assets>>,
//     next: Next
// }

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Endpoint {
    pub(crate) vendor: String,
    pub(crate) integration_id: Option<Uuid>,
    pub(crate) integration: Option<Integration>,
    pub(crate) method: String,
    pub(crate) headers: Option<Vec<Header>>,
    pub(crate) path_params: Option<HashMap<String, String>>,
    pub(crate) query_params: Option<HashMap<String, String>>,
    pub(crate) body: Option<Value>,
    pub(crate) target_url: String,
}

impl Endpoint {
    pub fn add_header(&mut self, key: String, value: String) {
        let headers = &mut self.headers;
        if let Some(headers) = headers {
            if !headers.iter().any(|header| header.key == key) {
                headers.push(Header {
                    key: key.clone(),
                    value: value.clone(),
                });
            } else {
                for header in headers {
                    if header.key == key {
                        header.value = value.clone();
                        return;
                    }
                }
            }
        } else {
            self.headers = Some(vec![Header {
                key: key.clone(),
                value: value.clone(),
            }]);
        }
    }

    fn convert_headers(&self) -> Option<HeaderMap> {
        let headers = &self.headers;
        if let Some(headers) = headers {
            let tuple_headers: Vec<(String, String)> =
                headers.iter().map(|header| header.as_tuple()).collect();
            let converted_headers: Vec<(HeaderName, HeaderValue)> = tuple_headers
                .iter()
                .map(|header| {
                    (
                        HeaderName::from_str(&header.0).unwrap(),
                        HeaderValue::from_str(&header.1).unwrap(),
                    )
                })
                .collect();
            let headers = HeaderMap::from_iter(converted_headers);
            Some(headers)
        } else {
            None
        }
    }

    fn convert_query_params(&self) -> Option<Vec<(String, String)>> {
        if let Some(params) = &self.query_params {
            let tuple_params = params
                .iter()
                .map(|(key, value)| (String::from(key), String::from(value)))
                .collect::<Vec<(String, String)>>();
            Some(tuple_params)
        } else {
            None
        }
    }

    fn convert_url(&self, integration: Option<&Integration>, context: &WorkerInvocation) -> String {
        if let Some(path_params) = &self.path_params {
            let url = &self.target_url;
            let re = Regex::new(r":([^/]+)").unwrap();
            let handlebarred = &re.replace_all(url, "{{$1}}");
            let handlebars = Handlebars::new();
            let mut substitutions = path_params.clone();

            if let Some(integration) = integration {
                let integration_json = serde_json::to_value(integration).unwrap();
                integration_json.as_object().unwrap().iter().for_each(|(key, value)| {
                    substitutions.insert(key.to_string(), value.to_string());
                });
            }
            
            let subbed = handlebars
                .render_template(&handlebarred, &substitutions)
                .unwrap();
            dbg!(&subbed);
            subbed
        } else {
            String::from(&self.target_url)
        }
    }

    pub fn get_auth(&self, integration: &Integration) -> auth::Auth {
        auth::Auth::new(integration)
    }

    pub async fn get_integration(&self, context: &WorkerInvocation) -> Option<Integration> {
        let vendor = &self.vendor;
        let integration_id = &self.integration_id.unwrap();
        let url = format!("http://localhost:8000/api/tenants/{tenant_id}/integrations/{vendor}/{integration_id}", tenant_id=context.tenant_id, vendor=vendor, integration_id=integration_id);
        dbg!(&url);
        let response = context
        .client
        .get(Url::parse(&url).unwrap())
        .header(
            HeaderName::from_str("Authorization").unwrap(),
            HeaderValue::from_str(&context.auth_token).unwrap(),
        )
        .send()
        .await
        .unwrap();
        let integration_json = response.json::<serde_json::Value>().await.unwrap();
        dbg!(&integration_json);
        let integration = Integration::new(integration_json);
        if let Ok(integration) = integration {
            dbg!(&integration);
            Some(integration)
        } else {
            dbg!("Integration not found");
            None
        }
    }

    pub async fn prepare(&mut self, context: &WorkerInvocation) -> Result<()> {
        let integration = self.get_integration(context).await;
        if let Some(integration) = integration.as_ref() {
            let mut auth = self.get_auth(integration);
            auth.inject_auth(self, context).await;
            dbg!(&self);
        } else {
            bail!("Integration not found");
        }
        self.integration = integration;

        // translate path params to Tera variables
        let re = Regex::new(r":([^\{/]+)").unwrap();
        self.target_url = re.replace_all(&self.target_url, "{{$1}}").into_owned();
        Ok(())
    }

    pub async fn execute(&mut self, context: &WorkerInvocation) -> Result<serde_json::Value> {
        // let integration = self.get_integration(context).await;
        // if let Some(integration) = integration.as_ref() {
        //     let auth = self.get_auth(integration);
        //     auth.inject_auth(self);
        //     dbg!(&self);
        // } else {
        //     bail!("Integration not found");
        // }

        // let converted = self.convert_url(integration.as_ref(), context);
        let url = Url::parse(&self.target_url)?;
        let method;
        let body;

        // scoping the mutex lock so it isn't held across the await below
        {
            method = Method::from_str(&self.method)?;
            body = self.body.clone();
        }

        let response = context
            .client
            .request(method, url)
            .headers(self.convert_headers().unwrap())
            .json(&body)
            .query(&self.convert_query_params().unwrap())
            .send()
            .await?;

        dbg!(&response);

        let status = response.status();
        let response_json = response.json::<serde_json::Value>().await?;
        dbg!(&response_json);
        let result = json!({
            "statusCode": status.as_u16(),
            "response": response_json
        });
        Ok(result)
    }
}
