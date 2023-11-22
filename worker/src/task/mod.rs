pub mod conditional;
pub mod endpoint;
pub mod looping;
pub mod filter;

use std::{collections::HashMap, fmt::{Display, Formatter}};

pub use conditional::Conditional;
pub use endpoint::Endpoint;
pub use looping::Loop;
pub use filter::Filter;

use xpertly_common::*;
use anyhow::{bail, Result};
use async_recursion::async_recursion;
use serde_json::{ json, Value };
use serde::{Deserialize, Serialize};

// use crate::asset::Assets;
use crate::WorkerInvocation;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Task {
    pub name: String,
    pub react_id: String,
    pub next: Option<Next>,
    pub assets: Assets,
    pub asset_vars: Option<HashMap<String, HashMap<String, Value>>>,
    pub needs_to_wait: bool,
    pub handler: Handler
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum TaskOutput {
    ConditionalResult(serde_json::Value),
    LoopResult(bool),
    EndpointResult(serde_json::Value),
    WebhookResult(serde_json::Value),
    FilterResult(serde_json::Value)
}

impl Task {
    pub async fn prepare(&mut self, context: &WorkerInvocation) -> Result<()> {
        // build asset variable structure
        let mut asset_vars = HashMap::new();
        if let Some(ref assets) = self.assets.objects {
            let tag_assets = assets.get(context.tag.clone().unwrap().as_str()).unwrap();
            if let Some(ref assets) = tag_assets.assets {
                for asset in assets.iter() {
                    let vendor = asset.integration_type.clone();
                    let asset_type = asset.asset_type.clone();
                    let attributes = asset.attributes.clone();
    
                    asset_vars
                        .entry(vendor)
                        .or_insert(HashMap::new())
                        .insert(asset_type, attributes);
                }
            }

            if let Some(ref devices) = tag_assets.devices {
                for device in devices.iter() {
                    let vendor = device.integration_type.clone();
                    let asset_type = device.attributes.get("deviceType").unwrap().as_str().unwrap().to_string();
                    let mut attributes = device.attributes.clone();
                    // injecting top-level attributes into the substitution vars
                    attributes.as_object_mut().unwrap().insert("device_model".to_string(), json!(device.device_model.clone()));
                    attributes.as_object_mut().unwrap().insert("device_serial".to_string(), json!(device.device_serial.clone()));

                    asset_vars
                        .entry(vendor)
                        .or_insert(HashMap::new())
                        .insert(asset_type, attributes);
                }
            }
        }

        self.asset_vars = Some(asset_vars);

        match &mut self.handler {
            Handler::Endpoint(endpoint_task) => {
                endpoint_task.prepare(context).await?;
            },
            Handler::Loop(loop_task) => {
                loop_task.prepare(context).await?;
            },
            Handler::Filter(filter_test) => {
                filter_test.prepare(context).await?;
            },
            _ => {}
        }
        Ok(())
    }

    #[async_recursion]
    pub async fn execute(&mut self, context: &WorkerInvocation) -> Result<TaskOutput> {
        match &mut self.handler {
            Handler::Endpoint(endpoint_task) => {
                match endpoint_task.execute(context).await {
                    Ok(result) => {
                        context
                            .outputs
                            .lock()
                            .unwrap()
                            .insert(self.react_id.clone(), result["response"].clone());
                        Ok(TaskOutput::EndpointResult(result.clone()))
                    }
                    Err(err) => {
                        bail!("Endpoint task failed: {}", err);
                    }
                }
            },
            Handler::Webhook(endpoint_task) => {
                match endpoint_task.execute(context).await {
                    Ok(result) => {
                        context
                            .outputs
                            .lock()
                            .unwrap()
                            .insert(self.react_id.clone(), result.clone());
                        Ok(TaskOutput::WebhookResult(json!({ "statusCode": 200, "response": "Webhook sent" })))
                    }
                    Err(err) => {
                        bail!("Webhook task failed: {}", err);
                    }
                }
            },
            Handler::Conditional(conditional_task) => match conditional_task.execute() {
                Ok(result) => {
                    context
                        .outputs
                        .lock()
                        .unwrap()
                        .insert(self.react_id.clone(), result["statusCode"].clone());
                    Ok(TaskOutput::ConditionalResult(result))
                }
                Err(err) => {
                    bail!("Conditional task failed: {}", err);
                }
            },
            Handler::Loop(loop_task) => {
                match loop_task.execute(context).await {
                    Ok(result) => {
                        context
                            .outputs
                            .lock()
                            .unwrap()
                            .insert(self.react_id.clone(), json!(result));
                        Ok(TaskOutput::LoopResult(true))
                    }
                    Err(err) => {
                        bail!("Loop task failed: {}", err);
                    }
                }
            },
            Handler::Filter(filter_task) => {
                let ret = filter_task.execute(context).await;

                if let Some(result) = ret.get("response") {
                    dbg!(&result);
                    context
                        .outputs
                        .lock()
                        .unwrap()
                        .insert(self.react_id.clone(), result.clone());
                }
                
                Ok(TaskOutput::FilterResult(ret))
            }
        }
    }

    pub fn from_config(task_config: TaskConfig) -> Result<Task> {
        // seems redundant but the data structure needs to be altered slightly before execution
        let handler = match task_config.fields {
            TaskFields::Endpoint(endpoint_fields) => {
                // query params are received from the frontend in a needlessly nested format:
                // {
                //     "<param>": { "value": "<value>" },
                //     "<param>": { "value": "<value>" },
                // }
                // below we unwrap them to a simple key-value map
                let query_params = if let Some(query_params_config) = endpoint_fields.query_params {
                    Some(query_params_config
                        .iter()
                        .map(|(key, value)| { (key.clone(), value.get("value").unwrap().clone()) })
                        .collect())
                } else {
                    None
                };

                let endpoint_task = Endpoint {
                    vendor: task_config.vendor.unwrap_or("".to_string()),
                    integration_id: task_config.integration_id,
                    integration: None,
                    method: endpoint_fields.method,
                    headers: endpoint_fields.headers,
                    path_params: endpoint_fields.path_params,
                    query_params,
                    body: endpoint_fields.body,
                    target_url: endpoint_fields.target_url,
                };

                if let Some(category) = task_config.category {
                    if category == "webhook" {
                        Handler::Webhook(endpoint_task)
                    } else {
                        if let None = task_config.integration_id {
                            bail!("Endpoint task must have an integration");
                        }
                        Handler::Endpoint(endpoint_task)
                    }
                } else {
                    bail!("unknown task category");
                }
            }
            TaskFields::Conditional(conditional_config) => {
                Handler::Conditional(Conditional {
                    expression: conditional_config.expression,
                })
            }
            TaskFields::Loop(loop_config) => {
                // convert all tasks in the loop task config to task objects
                let tasks = loop_config.tasks.into_iter().map(|task_config| Task::from_config(task_config).unwrap()).collect();
                Handler::Loop(Loop {
                    tasks,
                    schema: task_config.assets.schema.clone(),
                    loop_assets: None
                })
            }
            TaskFields::Filter(filter_fields) => {
                Handler::Filter(Filter {
                    object_to_filter: filter_fields.object_to_filter,
                    condition: filter_fields.condition,
                    search_key: filter_fields.search_key,
                    search_value: filter_fields.search_value,
                    json_obj: None
                })
            }
        };

        Ok(Task { 
            name: task_config.name.unwrap_or("".to_string()), 
            react_id: task_config.react_id, 
            next: task_config.next,
            assets: task_config.assets,
            asset_vars: None,
            needs_to_wait: task_config.needs_to_wait,
            handler 
        })
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Handler {
    Endpoint(Endpoint),
    Conditional(Conditional),
    Loop(Loop),
    Webhook(Endpoint),
    Filter(Filter),
}

impl Handler {
    pub fn get_integration(&self) -> Option<&Integration> {
        match self {
            Handler::Endpoint(endpoint_task) => {
                if let Some(integration) = &endpoint_task.integration {
                    Some(integration)
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

impl Display for Handler {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Handler::Endpoint(_) => {
                write!(f, "endpoint")
            }
            Handler::Conditional(_) => {
                write!(f, "conditional")
            }
            Handler::Loop(_) => {
                write!(f, "loop")
            }
            Handler::Webhook(_) => {
                write!(f, "webhook")
            }
            Handler::Filter(_) => {
                write!(f, "filter")
            }
        }
    }
}

// #[derive(Serialize, Deserialize, Debug, Clone)]
// pub struct Next {
//     #[serde(rename = "true")]
//     true_branch: Option<String>,
//     #[serde(rename = "false")]
//     false_branch: Option<String>,
// }
