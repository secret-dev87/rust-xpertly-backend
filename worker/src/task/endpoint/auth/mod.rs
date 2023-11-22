use std::iter::FromIterator;

use xpertly_common::Integration;
use super::Endpoint;
use http::StatusCode;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use crate::WorkerInvocation;
use async_trait::async_trait;

#[derive(Debug, Serialize, Deserialize)]
pub enum Auth {
    Meraki(MerakiAuth),
    Jira(JiraAuth),
    Ansible(AnsibleAuth),
    Netbox(NetboxAuth),
    Avicenna(AvicennaAuth),
    Oauth(OAuth),
    Splunk(SplunkAuth),
    Dnac(DnacAuth),
    Viptela(ViptelaAuth),
}

impl Auth {
    pub fn new(integration: &Integration) -> Self {
        match &integration {
            Integration::Meraki(meraki_integration) => Auth::Meraki(MerakiAuth {
                api_key: meraki_integration.api_key.clone(),
            }),
            Integration::Ansible(ansible_integration) => Auth::Ansible(AnsibleAuth {
                username: ansible_integration.username.clone(),
                password: ansible_integration.password.clone(),
            }),
            Integration::Splunk(splunk_integration) => Auth::Splunk(SplunkAuth {
                hec_token: splunk_integration.hec_token.clone(),
            }),
            Integration::Dnac(dnac_integration) => Auth::Dnac(DnacAuth {
                username: dnac_integration.username.clone(),
                password: dnac_integration.password.clone(),
                token: None,
                dnac_hostname: dnac_integration.dnac_hostname.clone(),
            }),
            Integration::Viptela(viptela_integration) => Auth::Viptela(ViptelaAuth {
                username: viptela_integration.username.clone(),
                password: viptela_integration.password.clone(),
                v_manage_hostname: viptela_integration.v_manage_hostname.clone(),
            }),
        }
    }
}
#[async_trait]
pub trait InjectAuth {
    async fn inject_auth(&mut self, task: &mut Endpoint, context: &WorkerInvocation);
}

#[async_trait]
impl InjectAuth for Auth {
    async fn inject_auth(&mut self, task: &mut Endpoint, context: &WorkerInvocation) {
        match self {
            Auth::Meraki(meraki_auth) => meraki_auth.inject_auth(task, context).await,
            Auth::Jira(jira_auth) => jira_auth.inject_auth(task, context).await,
            Auth::Ansible(ansible_auth) => ansible_auth.inject_auth(task, context).await,
            Auth::Netbox(netbox_auth) => netbox_auth.inject_auth(task, context).await,
            Auth::Avicenna(avicenna_auth) => avicenna_auth.inject_auth(task, context).await,
            Auth::Oauth(oauth_auth) => oauth_auth.inject_auth(task, context).await,
            Auth::Splunk(splunk_auth) => splunk_auth.inject_auth(task, context).await,
            Auth::Dnac(dnac_auth) => dnac_auth.inject_auth(task, context).await,
            Auth::Viptela(viptela_auth) => viptela_auth.inject_auth(task, context).await,
        };
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MerakiAuth {
    api_key: String,
}

#[async_trait]
impl InjectAuth for MerakiAuth {
    async fn inject_auth(&mut self, task: &mut Endpoint, context: &WorkerInvocation) {
        task.add_header(
            String::from("X-Cisco-Meraki-API-Key"),
            String::from(&self.api_key),
        );
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JiraAuth {
    username: String,
    api_key: String,
}

#[async_trait]
impl InjectAuth for JiraAuth {
    async fn inject_auth(&mut self, task: &mut Endpoint, context: &WorkerInvocation) {
        task.add_header(
            String::from("Authorization"),
            format!("Basic {}", base64::encode(&format!("{}:{}", self.username, self.api_key))),
        );
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AnsibleAuth {
    username: String,
    password: String,
}

#[async_trait]
impl InjectAuth for AnsibleAuth {
    async fn inject_auth(&mut self, task: &mut Endpoint, context: &WorkerInvocation) {
        task.add_header(
            String::from("Authorization"),
            format!("Basic {}", base64::encode(&format!("{}:{}", self.username, self.password))),
        );
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NetboxAuth {
    api_key: String,
}

#[async_trait]
impl InjectAuth for NetboxAuth {
    async fn inject_auth(&mut self, task: &mut Endpoint, context: &WorkerInvocation) {
        task.add_header(
            String::from("Authorization"),
            format!("Token {}", self.api_key),
        );
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AvicennaAuth {
    auth_token: String,
}

#[async_trait]
impl InjectAuth for AvicennaAuth {
    async fn inject_auth(&mut self, task: &mut Endpoint, context: &WorkerInvocation) {
        task.add_header(
            String::from("Authorization"),
            format!("Bearer {}", self.auth_token),
        );
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OAuth {
    token: String,
}

#[async_trait]
impl InjectAuth for OAuth {
    async fn inject_auth(&mut self, task: &mut Endpoint, context: &WorkerInvocation) {
        task.add_header(String::from("Authorization"), format!("Bearer {}", self.token));
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SplunkAuth {
    hec_token: String,
}

#[async_trait]
impl InjectAuth for SplunkAuth {
    async fn inject_auth(&mut self, task: &mut Endpoint, context: &WorkerInvocation) {
        task.add_header(
            String::from("Authorization"),
            format!("Splunk {}", self.hec_token),
        );
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DnacAuth {
    username: String,
    password: String,
    token: Option<String>,
    dnac_hostname: String,
}

#[async_trait]
impl InjectAuth for DnacAuth {
    async fn inject_auth(&mut self, task: &mut Endpoint, context: &WorkerInvocation) {
        // fetch token
        let url = format!("https://{}/dna/system/api/v1/auth/token", self.dnac_hostname);
        let response = context.client.post(url)
            .basic_auth(&self.username, Some(&self.password))
            .send().await.unwrap();

        let token = match response.status() {
            StatusCode::OK => {
                let response_body: serde_json::Value = response.json().await.unwrap();
                response_body["Token"].as_str().unwrap().to_string()
            },
            _ => {
                dbg!(&response.text().await);
                panic!("Failed to get token from DNAC");
            }
        };

        task.add_header("x-auth-token".to_string(), token);
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViptelaAuth {
    username: String,
    password: String,
    v_manage_hostname: String,
}

#[async_trait]
impl InjectAuth for ViptelaAuth {
    async fn inject_auth(&mut self, task: &mut Endpoint, context: &WorkerInvocation) {
        // fetch jsessionid first
        let url = format!("https://{}/j_security_check", self.v_manage_hostname);
        let payload = [("j_username", &self.username), ("j_password", &self.password)];
        let response = context.client.post(url)
            .form(&payload)
            .send().await.unwrap();

        let jsessionid = match response.headers().get("Set-Cookie") {
            Some(cookies) => {
                let jsessionid = cookies.to_str().unwrap().split(';').next().unwrap();
                jsessionid.to_string()
            }
            _ => {
                panic!("Failed to get JSESSION ID from Viptela");
            }
        };

        let url = format!("https://{}/dataservice/client/token", self.v_manage_hostname);
        let response = context.client.get(url)
            .header("Cookie", jsessionid.clone())
            .send().await
            .unwrap();

        let token = match response.status() {
            StatusCode::OK => {
                Some(response.text().await.unwrap())
            },
            _ => {
                None
            }
        };

        task.add_header("Content-Type".to_string(), "application/json".to_string());
        task.add_header("Cookie".to_string(), jsessionid);

        if let Some(token) = token {
            task.add_header("X-XSRF-TOKEN".to_string(), token);
        }
    }
}
