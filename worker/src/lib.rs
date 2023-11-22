pub mod task;

use crate::task::{Handler, Task, TaskOutput};
use actix::dev::channel;
use actix::{Message, Recipient};
use anyhow::Result;
use chrono::{self, Utc};
use core::fmt;
use core::str::FromStr;
use jsonwebtoken::{encode, EncodingKey, Header};
use regex::Regex;
use reqwest::header::{HeaderName, HeaderValue};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::error::Error;
use std::sync::{Arc, Mutex};
use std::thread;
use tera;
use tera::Tera;
use tokio;
use tokio::sync::mpsc::Sender;
use uuid::Uuid;
use xpertly_common::*;

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Worker {
    name: String,
    id: Uuid,
    #[serde(rename = "type")]
    category: Option<String>,
    available_in_avicenna: bool,
    description: String,
    tenant_id: Uuid,
    tasks: HashMap<String, Task>,
    start: String,
    latest_task: Option<String>,
    custom: Option<serde_json::Value>,
    global: Option<serde_json::Value>,
}

impl Worker {
    pub fn from_config(worker_config: &WorkerConfig) -> Result<Worker> {
        let mut tasks = HashMap::new();
        let start = worker_config.tasks[0].react_id.clone();
        for task_config in worker_config.tasks.clone().into_iter() {
            let task = Task::from_config(task_config.clone())?;
            tasks.insert(task_config.react_id, task);
        }

        Ok(Worker {
            name: worker_config.name.clone(),
            id: worker_config.id,
            category: worker_config.category.clone(),
            available_in_avicenna: worker_config.available_in_avicenna,
            description: worker_config.description.clone(),
            tenant_id: worker_config.tenant_id,
            tasks,
            start,
            latest_task: None,
            custom: worker_config.custom.clone(),
            global: worker_config.global.clone(),
        })
    }
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct WorkerInvocation {
    pub tenant_id: Uuid,
    pub triggered_by: String,
    pub triggered_by_id: Uuid,
    pub worker: Worker,
    pub execution_id: Uuid,
    pub run_id: Uuid,
    pub tag: Option<String>,
    pub auth_token: String,
    pub outputs: Arc<Mutex<HashMap<String, serde_json::Value>>>,
    #[serde(skip)]
    #[serde(default = "InvocationState::default")]
    state: Arc<Mutex<InvocationState>>,
    #[serde(skip)]
    pub client: Client,
    pub assets: Arc<Mutex<Assets>>,
    #[serde(skip)]
    pub channel: Option<Recipient<Publish>>,
    #[serde(skip)]
    pub wait_token: String,
}

impl Clone for WorkerInvocation {
    fn clone(&self) -> Self {
        WorkerInvocation {
            tenant_id: self.tenant_id.clone(),
            triggered_by: self.triggered_by.clone(),
            triggered_by_id: self.triggered_by_id.clone(),
            worker: self.worker.clone(),
            execution_id: self.execution_id.clone(),
            run_id: self.run_id.clone(),
            tag: self.tag.clone(),
            auth_token: self.auth_token.clone(),
            // shallow copying these values so that the clones don't mutate the originals
            // this is useful for quickly creating a loop context in the loop task which
            // contains all the same information up to the point of that task being invoked
            // but which will maintain a separate state for each iteration of the loop that
            // isn't valid outside of that loop
            outputs: {
                let outputs = Arc::clone(&self.outputs);
                let data = outputs.lock().unwrap();
                let cloned_data = data.clone();
                drop(data);
                Arc::new(Mutex::new(cloned_data))
            },
            state: {
                let state = Arc::clone(&self.state);
                let data = state.lock().unwrap();
                let cloned_data = data.clone();
                drop(data);
                Arc::new(Mutex::new(cloned_data))
            },
            client: self.client.clone(),
            assets: {
                let assets = Arc::clone(&self.assets);
                let data = assets.lock().unwrap();
                let cloned_data = data.clone();
                drop(data);
                Arc::new(Mutex::new(cloned_data))
            },
            channel: self.channel.clone(),
            wait_token: self.wait_token.clone(),
        }
    }
}

impl WorkerInvocation {
    // this needs much more thought put into it, probably better to deserialize somehow with serde
    pub fn from_suspended(suspended_invocation: serde_json::Value) -> Result<WorkerInvocation> {
        let worker =
            serde_json::from_value::<Worker>(suspended_invocation["worker"].clone()).unwrap();
        let outputs_map = suspended_invocation["outputs"].as_object().unwrap().clone();
        let mut outputs = HashMap::new();
        outputs_map.iter().for_each(|(k, v)| {
            outputs.insert(k.clone(), v.clone());
        });
        let client = Client::new();
        let auth_token = suspended_invocation["authToken"].as_str().unwrap();
        let wait_token = construct_wait_token(
            serde_json::from_value::<Uuid>(suspended_invocation["runId"].clone()).unwrap(),
            auth_token,
            None,
        );
        let tag = if let Some(tag) = suspended_invocation["tag"].as_str() {
            Some(tag.to_string())
        } else {
            None
        };
        let assets = Arc::new(Mutex::new(
            serde_json::from_value::<Assets>(suspended_invocation["assets"].clone()).unwrap(),
        ));

        Ok(WorkerInvocation {
            tenant_id: suspended_invocation["tenantId"]
                .as_str()
                .unwrap()
                .parse::<Uuid>()?,
            triggered_by: suspended_invocation["triggeredBy"]
                .as_str()
                .unwrap()
                .to_string(),
            triggered_by_id: suspended_invocation["triggeredById"]
                .as_str()
                .unwrap()
                .parse::<Uuid>()?,
            worker,
            execution_id: suspended_invocation["executionId"]
                .as_str()
                .unwrap()
                .parse::<Uuid>()?,
            run_id: suspended_invocation["runId"]
                .as_str()
                .unwrap()
                .parse::<Uuid>()?,
            tag,
            auth_token: suspended_invocation["authToken"]
                .as_str()
                .unwrap()
                .to_string(),
            outputs: Arc::new(Mutex::new(outputs)),
            state: Arc::new(Mutex::new(InvocationState::Pending)),
            client,
            assets,
            channel: None,
            wait_token,
        })
    }

    async fn suspend(&self) {
        let index = format!("xpertly_handler_payload_{}", self.run_id.as_hyphenated());
        let mut suspended_invocation = serde_json::to_value(&self)
            .unwrap()
            .as_object()
            .unwrap()
            .clone();
        suspended_invocation.insert(
            "@timestamp".to_string(),
            json!(chrono::Utc::now().to_rfc3339()),
        );

        let payload = json!(
            {
                "index": index,
                "payload": suspended_invocation
            }
        );

        println!("{}", serde_json::to_string_pretty(&payload).unwrap());
        self
            .client
            .post("https://api.dev.xpertly.io/v1/client/post_to_elastic")
            .header(
                HeaderName::from_str("Authorization").unwrap(),
                HeaderValue::from_str(&self.auth_token).unwrap(),
            )
            .json(&payload)
            .send()
            .await
            .unwrap()
            .json::<serde_json::Value>()
            .await
            .unwrap();
    }

    async fn start(self) {
        *self.state.lock().unwrap() = InvocationState::Running;
        self.log(Event::WorkerStart, None, None, None).await;

        // if let Some(tag) = &self.tag {
        //     match self.get_assets(tag).await {
        //         Ok(assets) => {
        //             *self.assets.lock().unwrap() = assets;
        //         }
        //         Err(e) => {
        //             self.log(Event::WorkerFail, None, None, Some(e)).await;
        //             *self.state.lock().unwrap() = InvocationState::Failed;
        //             return;
        //         }
        //     }
        // }

        self.run().await;
    }

    pub async fn resume(
        mut self,
        pending_output: &serde_json::Value,
        channel: Option<Recipient<Publish>>,
    ) {
        // if we've been given a channel to publish logs to
        if let Some(channel) = channel {
            self.channel = Some(channel);
        }
        // if there is a latest task, resume from that point, otherwise start from the beginning of the worker (this shouldn't happen)
        if let Some(latest) = &self.worker.latest_task {
            // TODO: this assumes that the next task should follow the true branch, which is only the case for endpoint tasks (because they can only have a true branch).
            // ATM endpoint tasks are the only ones that can be suspended but this should probably be handled differently in case other tasks can suspend execution in the future
            let latest_task = self.worker.tasks.get(latest).unwrap();
            let mut paused_output = self
                .outputs
                .lock()
                .unwrap()
                .get(&latest.clone())
                .unwrap()
                .clone()
                .as_object()
                .unwrap()
                .clone();
            paused_output.insert("customOutput".to_string(), pending_output.clone());
            self.outputs
                .lock()
                .unwrap()
                .insert(latest.clone(), json!(paused_output));

            let final_output = json!({
                "statusCode": 200,
                "response": paused_output.clone()
            });

            self.log(
                Event::TaskSuccess,
                Some(&latest_task),
                Some(TaskOutput::EndpointResult(final_output)),
                None,
            )
            .await;

            let next_task_name = latest_task.next.as_ref().unwrap().true_branch.as_ref();
            if let Some(name) = next_task_name {
                let next_task = self.worker.tasks.get(name).unwrap();
                self.worker.start = next_task.react_id.clone();
                *self.state.lock().unwrap() = InvocationState::Running;
                self.run().await;
            }
        } else {
            self.start().await;
        }
        println!("Worker execution complete!");
    }
    
    pub async fn cancel(
        mut self,
        pending_output: &serde_json::Value,
        channel: Option<Recipient<Publish>>,
    ) {
        // if we've been given a channel to publish logs to
        if let Some(channel) = channel {
            self.channel = Some(channel);
        }
        // if there is a latest task, resume from that point, otherwise start from the beginning of the worker (this shouldn't happen)
        if let Some(latest) = &self.worker.latest_task {
            // TODO: this assumes that the next task should follow the true branch, which is only the case for endpoint tasks (because they can only have a true branch).
            // ATM endpoint tasks are the only ones that can be suspended but this should probably be handled differently in case other tasks can suspend execution in the future
            let latest_task = self.worker.tasks.get(latest).unwrap();

            let final_output = json!({
                "statusCode": 500,
                "response": pending_output.clone()
            });

            self.log(
                Event::APIFail,
                Some(&latest_task),
                Some(TaskOutput::EndpointResult(final_output)),
                None,
            )
            .await;

        }
        println!("worker failed due to cancellation");
        self.log(Event::WorkerFail, None, None, None).await;
        *self.state.lock().unwrap() = InvocationState::Failed;
        return;
    }
    async fn run(mut self) {
        // change to hashmap lookup beginning with task found under 'start' key in worker, following 'next' key of each task
        let mut next = self.worker.tasks.get(&self.worker.start);
        while let Some(task) = next {
            let mut task = task.clone();
            task.prepare(&self).await.unwrap();
            let mut task = match task.handler {
                // turning off variable subsitiution for loops as inner tasks may not have required variables available yet
                // those inner tasks will be rendered when they are executed
                Handler::Loop(_) => task,
                _ => {
                    // rendering twice is a workaround for a path parameter translation bug
                    let task = self.render_variables(&task);
                    self.render_variables(&task)
                }
            };

            self.log(Event::TaskStart, Some(&task), None, None).await;
            match task.execute(&self).await {
                Ok(task_result) => {
                    println!("task finished");
                    self.worker.latest_task = Some(task.react_id.clone());

                    if let Some(branches) = &task.next {
                        match task_result {
                            TaskOutput::ConditionalResult(ref result) => {
                                let result = result["statusCode"].as_bool().unwrap();
                                let next_task_name;
                                if result {
                                    next_task_name = branches.true_branch.as_ref();
                                } else {
                                    next_task_name = branches.false_branch.as_ref();
                                }

                                if let Some(name) = next_task_name {
                                    next = self.worker.tasks.get(name);
                                } else {
                                    next = None;
                                }
                            }
                            TaskOutput::FilterResult(ref result) => {
                                let found = result["statusCode"].as_bool().unwrap();
                                let next_task_name = match found {
                                    true => branches.true_branch.as_ref(),
                                    false => branches.false_branch.as_ref(),
                                };

                                if let Some(name) = next_task_name {
                                    next = self.worker.tasks.get(name);
                                } else {
                                    next = None;
                                }
                            }
                            _ => {
                                let next_task_name = branches.true_branch.as_ref();
                                if let Some(name) = next_task_name {
                                    next = self.worker.tasks.get(name);
                                } else {
                                    next = None;
                                }
                            }
                        }

                        if task.needs_to_wait {
                            *self.state.lock().unwrap() = InvocationState::Waiting;
                            self.suspend().await;
                            break;
                        }

                        if branches.true_branch.is_none() & branches.false_branch.is_none() {
                            // execution has finished
                            self.log(Event::TaskSuccess, Some(&task), Some(task_result), None)
                                .await;
                            self.log(Event::WorkerSuccess, None, None, None).await;
                            println!("execution has finished");
                            *self.state.lock().unwrap() = InvocationState::Complete;
                            break;
                        }

                        self.log(Event::TaskSuccess, Some(&task), Some(task_result), None)
                            .await;
                    } else {
                        // execution has finished
                        self.log(Event::TaskSuccess, Some(&task), Some(task_result), None)
                            .await;
                        self.log(Event::WorkerSuccess, None, None, None).await;
                        *self.state.lock().unwrap() = InvocationState::Complete;
                        break;
                    }
                }
                Err(err) => {
                    self.log(Event::TaskFail, Some(&task), None, Some(err))
                        .await;
                    println!("worker failed");
                    self.log(Event::WorkerFail, None, None, None).await;
                    *self.state.lock().unwrap() = InvocationState::Failed;
                    return;
                }
            };
        }
    }

    pub fn render_variables(&self, task: &Task) -> Task {
        // generate a mapping of task names to that task's unique ID. Outputs are recorded against the ID,
        // not the task name so we need to translate user-facing task names to IDs
        let task_name_map = self
            .worker
            .tasks
            .iter()
            .map(|(react_id, task)| (task.name.clone(), react_id.clone()))
            .collect::<HashMap<String, String>>();

        let serialized = serde_json::to_string(task).unwrap();
        let variable_re = Regex::new(r"\{\{((?P<var_type>[^:\{\}]*):)?(?P<var_identifier>[^\[\.\{\}]+)\.?(?P<var_path>[^\}\{]*)\}\}").unwrap();

        // translate variable syntax to Tera, making white space in path segments acceptable, and replacing task names with IDs
        let translated = &variable_re.replace_all(&serialized, |groups: &regex::Captures| {
            dbg!(&groups);
            let var_type = groups.name("var_type");
            let var_identifier = String::from(groups.name("var_identifier").unwrap().as_str());
            let var_path = String::from(groups.name("var_path").unwrap().as_str());

            // split the path into segments to be rearranged in a format that Tera can understand
            // e.g. [0].key1.key2[3] -> ["[0]", "key1", "key2", "[3]"] -> ["[0]", "['key1']", "['key2']", "[3]"] -> "[0]['key1']['key2'][3]"
            let segment_re = Regex::new(r"([^\[\.\}]+|\[\d+\])").unwrap();
            let tokens = segment_re.captures_iter(&var_path).map(|capture| {
                let segment = capture.get(1).unwrap().as_str();
                if segment.starts_with("[") {
                    segment.to_string()
                } else {
                    format!("['{}']", segment)
                }
            }).collect::<Vec<String>>();

            match var_type {
                Some(var_type) => match var_type.as_str() {
                    "OUTPUT" => {
                        let task_id = task_name_map.get(&var_identifier).unwrap_or(&"default".to_string()).to_owned();
                        let path = tokens.join("");
                        // relies on patched Tera package to support the `is defined` operator for variables using square bracket notation
                        // https://github.com/p-ackland/tera
                        // should be replaced once Tera v2 is released as the maintainer has marked the patch as "won't fix"
                        format!("{{% if output.{task_id}{path} is defined %}}{{{{ output.{task_id}{path} }}}}{{% else %}}undefined{{% endif %}}", task_id = task_id, path = path)
                    },
                    "ASSET" => {
                        format!("{{{{asset.{}{}}}}}", var_identifier, tokens.join(""))
                    },
                    "CUSTOM" => {
                        format!("{{{{custom['{}']}}}}", var_identifier)
                    },
                    "GLOBAL" => {
                        format!("{{{{global['GLOBAL:{}']}}}}", var_identifier)
                    },
                    _ => {
                        panic!("Invalid variable type: {}", var_type.as_str());
                    }
                },
                // relies on patched Tera package to support the `is defined` operator for variables using square bracket notation
                // https://github.com/p-ackland/tera
                // should be replaced once Tera v2 is released as the maintainer has marked the patch as "won't fix"
                None => format!("{{% if {var_identifier} is defined %}}{{{{{var_identifier}}}}}{{% else %}}undefined{{% endif %}}", var_identifier = var_identifier),
            }
        }).to_string();
        dbg!(&translated);
        let mut context = tera::Context::new();
        context.insert("output", &self.outputs.lock().unwrap().clone());
        context.insert("asset", &task.asset_vars.as_ref().unwrap().clone());
        context.insert("global", &self.worker.global.clone());
        context.insert("custom", &self.worker.custom.clone());
        //TODO: should only be available if needs_to_wait is true
        context.insert("xpertlyRequestToken", &self.wait_token.clone());
        if let Some(tag) = &self.tag {
            context.insert("tagName", &tag);
        }

        // endpoint task specific logic shouldn't live here
        if let Handler::Endpoint(endpoint) = &task.handler {
            if let Some(integration) = endpoint.integration.as_ref() {
                let integration_json = serde_json::to_value(integration).unwrap();
                integration_json
                    .as_object()
                    .unwrap()
                    .iter()
                    .for_each(|(key, value)| {
                        context.insert(key.to_string(), value);
                    });
            }

            if let Some(path_params) = endpoint.path_params.as_ref() {
                let path_params_json = serde_json::to_value(path_params).unwrap();
                path_params_json
                    .as_object()
                    .unwrap()
                    .iter()
                    .for_each(|(key, value)| {
                        context.insert(key.to_string(), value);
                    });
            }
        }

        dbg!(&task);

        let rendered = Tera::one_off(translated, &context, false).unwrap_or_else(|err| {
            panic!("Failed to render task: {}", err.source().unwrap());
        });

        dbg!(&rendered);
        serde_json::from_str::<Task>(&rendered).unwrap()
    }

    // async fn get_assets(&self, tag: &str) -> Result<Assets> {
    //     let result = self
    //         .client
    //         .get(&format!(
    //             "https://api.dev.xpertly.io/v1/tenants/{}/assets-by-tags",
    //             self.tenant_id
    //         ))
    //         .header(
    //             HeaderName::from_str("Authorization")?,
    //             HeaderValue::from_str(&self.auth_token)?,
    //         )
    //         .query(&[("tags", tag)])
    //         .send()
    //         .await?
    //         .json::<serde_json::Value>()
    //         .await?;

    //     let devices = result["devices"][tag].as_array().cloned();
    //     let assets = result["assets"][tag].as_array().cloned();

    //     Ok(Assets::from_lists(assets, devices))
    // }

    async fn log(
        &self,
        event: Event,
        task: Option<&Task>,
        output: Option<TaskOutput>,
        error: Option<anyhow::Error>,
    ) {
        let error_text = match error {
            Some(error) => Some(error.to_string()),
            None => None,
        };

        let tag = match &self.tag {
            Some(tag) => tag.clone(),
            None => "None".to_string(),
        };

        let log = WorkerLog {
            timestamp: chrono::Utc::now().to_rfc3339(),
            tenant_id: self.tenant_id,
            worker_name: self.worker.name.clone(),
            worker_id: self.worker.id,
            execution_id: self.execution_id,
            worker_run_id: self.run_id,
            run_by: self.triggered_by.clone(),
            run_by_user_id: self.triggered_by_id,
            tag,
            task_name: if let Some(task) = task {
                Some(task.name.clone())
            } else {
                None
            },
            task_type: if let Some(task) = task {
                Some(format!("{}", task.handler))
            } else {
                None
            },
            react_id: if let Some(task) = task {
                Some(task.react_id.clone())
            } else {
                None
            },
            event: event,
            reason: error_text,
            outputs: serde_json::to_string(&output).unwrap(),
        };

        println!("Logging: {:?}", log);
        // log to ES, record but ignore errors as they're not critical to execution
        let index = format!("xpertly_worker_run_{}", self.tenant_id);
        let payload = json!({"index": index, "payload": log});

        match self
            .client
            .post("https://api.dev.xpertly.io/v1/client/post_to_elastic")
            .header(
                HeaderName::from_str("Authorization").unwrap(),
                HeaderValue::from_str(&self.auth_token).unwrap(),
            )
            .json(&payload)
            .send()
            .await
        {
            Err(e) => println!("Error logging to Elasticsearch: {}", e),
            Ok(resp) => println!("response: {:?}", resp.text().await),
        }

        // log to channel for live updates
        if let Some(channel) = &self.channel {
            println!("Sending log to channel");
            match channel
                .send(Publish {
                    id: self.execution_id,
                    msg: log,
                })
                .await
            {
                Ok(_) => {}
                Err(e) => {
                    println!("Error sending log: {}", e);
                }
            }
        }
    }

    fn add_task_output(&mut self, output: TaskOutput, task: &Task) {
        let mut outputs = self.outputs.lock().unwrap();
        match output {
            TaskOutput::EndpointResult(result) => outputs.insert(task.react_id.clone(), result),
            TaskOutput::WebhookResult(result) => outputs.insert(task.react_id.clone(), result),
            TaskOutput::ConditionalResult(result) => {
                outputs.insert(task.react_id.clone(), json!(result))
            }
            TaskOutput::LoopResult(result) => outputs.insert(task.react_id.clone(), json!(result)),
            TaskOutput::FilterResult(result) => outputs.insert(task.react_id.clone(), result),
        };
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
enum InvocationState {
    Pending,
    Running,
    Complete,
    Failed,
    Waiting,
}

impl InvocationState {
    fn default() -> Arc<Mutex<InvocationState>> {
        Arc::new(Mutex::new(InvocationState::Pending))
    }
}

#[derive(Serialize, Deserialize, Debug, Copy, Clone)]
#[serde(rename_all = "snake_case")]
pub enum Event {
    WorkerStart,
    WorkerSuccess,
    WorkerFail,
    TaskStart,
    TaskSuccess,
    TaskFail,
    APIFail,
}

impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Event::WorkerStart => write!(f, "worker_start"),
            Event::WorkerSuccess => write!(f, "worker_success"),
            Event::WorkerFail => write!(f, "worker_fail"),
            Event::TaskStart => write!(f, "task_start"),
            Event::TaskSuccess => write!(f, "task_success"),
            Event::TaskFail => write!(f, "task_fail"),
            Event::APIFail => write!(f, "api_fail"),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Message)]
#[serde(rename_all = "camelCase")]
#[rtype(result = "()")]
pub struct WorkerLog {
    #[serde(rename = "@timestamp")]
    pub timestamp: String,
    pub tenant_id: Uuid,
    pub worker_name: String,
    pub worker_id: Uuid,
    pub execution_id: Uuid,
    pub worker_run_id: Uuid,
    pub task_name: Option<String>,
    pub task_type: Option<String>,
    pub react_id: Option<String>,
    // pub task_id: Option<Uuid>,
    pub run_by: String,
    pub run_by_user_id: Uuid,
    pub tag: String,
    pub event: Event,
    pub reason: Option<String>,
    pub outputs: String,
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct Publish {
    pub id: Uuid,
    pub msg: WorkerLog,
}

pub fn construct_wait_token(
    run_id: Uuid,
    auth_token: &str,
    exp: Option<chrono::DateTime<Utc>>,
) -> String {
    let secret = EncodingKey::from_secret("wow much secret".as_ref());
    let expiry = match exp {
        Some(exp) => exp.timestamp(),
        None => (chrono::Utc::now() + chrono::Duration::hours(24)).timestamp(),
    };
    let token_claims = json!({"id": run_id, "auth": auth_token, "exp": expiry});
    let wait_token = jsonwebtoken::encode(&Header::default(), &token_claims, &secret).unwrap();
    wait_token
}

pub fn execute_worker(
    tags: Option<Vec<String>>,
    worker: Worker,
    auth_token: &str,
    user: AvicennaUser,
) {
    let execution_id = Uuid::new_v4();
    // create reusable client. Reqwest clients implement request pools internally
    // so the same instance can be used between all invocations and tasks.
    let client = reqwest::Client::new();
    let mut invocations = vec![];

    let secret = EncodingKey::from_secret("wow much secret".as_ref());
    if let Some(tags) = tags {
        for tag in tags.iter() {
            let worker = worker.clone();
            let run_id = Uuid::new_v4();
            let wait_token = construct_wait_token(run_id, auth_token, None);

            invocations.push(WorkerInvocation {
                tenant_id: worker.tenant_id,
                triggered_by: String::from(&user.user_email),
                triggered_by_id: user.user_id.clone(),
                worker,
                execution_id,
                run_id,
                tag: Some(String::from(tag.as_str())),
                auth_token: auth_token.to_string(),
                outputs: Arc::new(Mutex::new(HashMap::<String, serde_json::Value>::new())),
                state: Arc::new(Mutex::new(InvocationState::Pending)),
                client: client.clone(),
                assets: Arc::new(Mutex::new(Assets::new())),
                channel: None,
                wait_token,
            });
        }
    } else {
        let run_id = Uuid::new_v4();
        let token_claims = json!({"id": run_id, "auth": auth_token, "exp": (chrono::Utc::now() + chrono::Duration::hours(24)).timestamp()});
        let wait_token = jsonwebtoken::encode(&Header::default(), &token_claims, &secret).unwrap();

        invocations.push(WorkerInvocation {
            tenant_id: worker.tenant_id,
            triggered_by: String::from(&user.user_email),
            triggered_by_id: user.user_id.clone(),
            worker,
            execution_id,
            run_id,
            tag: None,
            auth_token: auth_token.to_string(),
            outputs: Arc::new(Mutex::new(HashMap::<String, serde_json::Value>::new())),
            state: Arc::new(Mutex::new(InvocationState::Pending)),
            client: client.clone(),
            assets: Arc::new(Mutex::new(Assets::new())),
            channel: None,
            wait_token,
        });
    }

    let runtime = tokio::runtime::Runtime::new().unwrap();
    let mut join_handles = vec![];
    for invocation in invocations {
        join_handles.push(runtime.spawn(async move { invocation.start().await }));
    }

    runtime.block_on(async move {
        for join_handle in join_handles {
            join_handle.await.unwrap();
        }
    });
}

pub fn resume_worker(invocation: WorkerInvocation) {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async move { invocation.start().await });
}

pub fn execute(
    tags: &Vec<String>,
    worker: Worker,
    user: AvicennaUser,
    token: &BearerToken,
    exe_id: Uuid,
    channel: Option<Recipient<Publish>>,
) {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let mut handles = vec![];
    let execution_id = exe_id;
    let client = reqwest::Client::new();
    let secret = EncodingKey::from_secret("wow much secret".as_ref());
    if tags.is_empty() {
        println!("no tags");
        let worker = worker.clone();
        let user = user.clone();
        let token = token.clone();
        let client = client.clone();
        let channel = channel.clone();
        let run_id = Uuid::new_v4();
        let token_claims = json!({"id": run_id, "auth": token, "exp": (chrono::Utc::now() + chrono::Duration::hours(24)).timestamp()});
        let wait_token = jsonwebtoken::encode(&Header::default(), &token_claims, &secret).unwrap();
        handles.push(runtime.spawn(async move {
            let invocation = WorkerInvocation {
                tenant_id: worker.tenant_id,
                triggered_by: String::from(&user.user_email),
                triggered_by_id: user.user_id,
                worker,
                execution_id,
                run_id,
                tag: None,
                auth_token: token.to_string(),
                outputs: Arc::new(Mutex::new(HashMap::<String, serde_json::Value>::new())),
                state: Arc::new(Mutex::new(InvocationState::Pending)),
                client,
                assets: Arc::new(Mutex::new(Assets::new())),
                channel,
                wait_token,
            };
            invocation.start().await;
        }))
    } else {
        // spawn a task on the async runtime for each tag
        for tag in tags.clone() {
            let worker = worker.clone();
            let user = user.clone();
            let token = token.clone();
            let client = client.clone();
            let channel = channel.clone();
            let run_id = Uuid::new_v4();
            let token_claims = json!({"id": run_id, "auth": token, "exp": (chrono::Utc::now() + chrono::Duration::hours(24)).timestamp()});
            let wait_token =
                jsonwebtoken::encode(&Header::default(), &token_claims, &secret).unwrap();
            handles.push(runtime.spawn(async move {
                let invocation = WorkerInvocation {
                    tenant_id: worker.tenant_id,
                    triggered_by: String::from(&user.user_email),
                    triggered_by_id: user.user_id,
                    worker,
                    execution_id,
                    run_id,
                    tag: Some(tag),
                    auth_token: token.to_string(),
                    outputs: Arc::new(Mutex::new(HashMap::<String, serde_json::Value>::new())),
                    state: Arc::new(Mutex::new(InvocationState::Pending)),
                    client,
                    assets: Arc::new(Mutex::new(Assets::new())),
                    channel,
                    wait_token,
                };
                invocation.start().await;
            }))
        }
    }
    // wait for threads to finish
    runtime.block_on(async move {
        for handle in handles {
            handle.await.unwrap();
        }
    });
}

pub fn test(channel: Sender<String>) {
    for i in 0..10 {
        let channel = channel.clone();
        tokio::spawn(async move {
            thread::sleep(std::time::Duration::from_secs(i));
            channel
                .send(format!("Hello from thread {}", i))
                .await
                .unwrap();
        });
    }
}

fn get_user_details(org_id: &str, user_id: &str, token: &str) -> User {
    let client = reqwest::blocking::Client::new();
    let response = client
        .get(format!(
            "https://api.dev.xpertly.io/v1/orgs/{org_id}/users/{user_id}",
            org_id = org_id,
            user_id = user_id
        ))
        .header(
            HeaderName::from_str("Authorization").unwrap(),
            HeaderValue::from_str(token).unwrap(),
        )
        .send()
        .unwrap()
        .json::<serde_json::Value>()
        .unwrap();

    dbg!(&response);
    let user = serde_json::from_value::<User>(response).unwrap();
    return user;
}

mod tests {
    use crate::task::{Filter, Endpoint};
    use super::*;

    fn create_mock_invocation() -> WorkerInvocation {
        WorkerInvocation {
            tenant_id: Uuid::new_v4(),
            triggered_by: String::from("mock@dummy.com"),
            triggered_by_id: Uuid::new_v4(),
            worker: Worker {
                id: Uuid::new_v4(),
                name: String::from("mock worker"),
                available_in_avicenna: false,
                description: String::from("mock description"),
                tasks: HashMap::new(),
                tenant_id: Uuid::new_v4(),
                category: None,
                start: String::from("mock workers don't have tasks"),
                latest_task: None,
                custom: None,
                global: None,
            },
            execution_id: Uuid::new_v4(),
            run_id: Uuid::new_v4(),
            tag: None,
            auth_token: String::from("auth_token"),
            outputs: Arc::new(Mutex::new(HashMap::<String, serde_json::Value>::new())),
            state: Arc::new(Mutex::new(InvocationState::Pending)),
            client: reqwest::Client::new(),
            assets: Arc::new(Mutex::new(Assets::new())),
            channel: None,
            wait_token: String::from("wait_token"),
        }
    }

    #[tokio::test]
    async fn test_filter() {
        let mut inv = create_mock_invocation();
        inv.worker.tasks.insert(
            String::from("mock_react_id"),
            Task {
                name: String::from("mock_output"),
                react_id: String::from("mock_react_id"),
                next: None,
                assets: Assets {
                    schema: None,
                    objects: None,
                },
                asset_vars: None,
                needs_to_wait: false,
                handler: Handler::Endpoint(Endpoint {
                    method: String::from("GET"),
                    target_url: String::from("https://jsonplaceholder.typicode.com/todos/1"),
                    headers: None,
                    body: None,
                    vendor: String::from("none"),
                    integration: None,
                    integration_id: None,
                    path_params: None,
                    query_params: None,
                }),
            },
        );

        inv.outputs.lock().unwrap().insert(
            String::from("mock_react_id"),
            json!({
              "customOutput": {
                "/interfaces/interface": {
                  "interfaces": [
                    {
                      "interface": [
                        {
                          "name": "Cellular0/2/0",
                          "interface-type": "ana-iftype-prop-p2p-serial",
                          "admin-status": "if-state-up",
                          "oper-status": "if-oper-state-ready",
                          "last-change": "2023-03-16T15:29:01.7+00:00",
                          "if-index": "11",
                          "phys-address": "3c:13:cc:d0:88:00",
                          "speed": "50000000",
                          "statistics": {
                              "discontinuity-time": "2023-03-02T00:52:48+00:00",
                              "in-octets": "335093127",
                              "in-unicast-pkts": "1466278"
                            }
                        },
                        {
                          "name": "Cellular0/2/1",
                          "interface-type": "iana-iftype-prop-p2p-serial",
                          "admin-status": "if-state-down",
                          "oper-status": "if-oper-state-no-pass",
                          "last-change": "2023-03-02T00:54:40.852+00:00",
                          "if-index": "12",
                          "phys-address": "3c:13:cc:d0:88:00",
                          "speed": "50000000",
                          "statistics": {
                              "discontinuity-time": "2023-03-02T00:52:48+00:00",
                              "in-octets": "335093127",
                              "in-unicast-pkts": "1466278"
                            }
                        },
                        {
                          "name": "GigabitEthernet0/0/0",
                          "interface-type": "iana-iftype-ethernet-csmacd",
                          "admin-status": "if-state-up",
                          "oper-status": "if-oper-state-ready",
                          "last-change": "2023-03-23T04:07:50.392+00:00",
                          "if-index": "1",
                          "phys-address": "3c:13:cc:d0:88:00",
                          "speed": "10000000",
                          "statistics": {
                              "discontinuity-time": "2023-03-02T00:52:48+00:00",
                              "in-octets": "335093127",
                              "in-unicast-pkts": "1466278"
                          }
                        }
                      ]
                    }
                  ]
                }
              }
            }),
        );

        let mut filter_task = Task {
            name: String::from("filter"),
            react_id: String::from("filter_task_react_id"),
            next: None,
            assets: Assets {
                schema: None,
                objects: None
            },
            asset_vars: None,
            needs_to_wait: false,
            handler: Handler::Filter(Filter {
                object_to_filter: String::from("{{OUTPUT:mock_output.customOutput./interfaces/interface.interfaces[0].interface}}"),
                search_key: String::from("interface-type"),
                search_value: String::from("iana"),
                condition: String::from("contains"),
                json_obj: None
            })
        };

        filter_task.prepare(&inv).await.unwrap();
        let mut rendered = inv.render_variables(&filter_task);

        let result = rendered.execute(&inv).await;
        dbg!(result);
    }

    #[test]
    fn test_conditional() {
        let conditional_str = r#"{
            "name": "Conditional",
            "vendor": null,
            "category": null,
            "type": "conditional",
            "taskId": null,
            "reactId": "dnd_conditional_node_lp40540crbc",
            "description": null,
            "xPos": 520,
            "yPos": 372,
            "needsToWait": true,
            "fields": {
                "expression": [
                    {
                       "op":null,
                       "conditions":[
                          {
                             "op":"AND",
                             "comparitor":"==",
                             "var1":"if-state-up",
                             "var2":"if-state-up"
                          },
                          {
                             "op":"AND",
                             "comparitor":"==",
                             "var1":"if-oper-state-ready",
                             "var2":"if-oper-state-ready"
                          },
                          {
                             "op":"AND",
                             "comparitor":"==",
                             "var1":"if-state-up",
                             "var2":"if-state-up"
                          },
                          {
                             "op":"AND",
                             "comparitor":"==",
                             "var1":"if-oper-state-ready",
                             "var2":"if-oper-state-ready"
                          },
                          {
                             "op":"AND",
                             "comparitor":"==",
                             "var1":"if-state-up",
                             "var2":"if-state-up"
                          }
                       ]
                    }
                ]
            },
            "output": null,
            "prev": {
                "true": "dnd_task_node_m3hk1zc9tfp",
                "false": null
            },
            "next": {
                "true": "dnd_task_node_ttuc9bt74z",
                "false": null
            },
            "assets": {
                "schema": null,
                "objects": null
            },
            "pathParamsPair": [],
            "queryParamsPair": [],
            "integrationId": ""
        }"#;

        let conditional_task_cfg = serde_json::from_str::<TaskConfig>(conditional_str).unwrap();
        let conditional_task = Task::from_config(conditional_task_cfg).unwrap();
        if let Handler::Conditional(conditional) = conditional_task.handler {
            dbg!(conditional.build_expression_str().unwrap());
        }
    }

    #[test]
    fn test_simple() {
        let worker_config = r#"{
                "id": "560ca980-1f0d-4987-a883-589f2878d966",
                "schemaId": null,
                "name": "Rust test",
                "type": "Test",
                "availableInAvicenna": false,
                "description": "testing the rust backend",
                "tasks": [
                    {
                        "name": "List the Networks in an Organization",
                        "vendor": "meraki",
                        "category": null,
                        "type": "endpoint",
                        "taskId": "2b8cc662-fa44-426d-a1e0-1f1b50416e9c",
                        "reactId": "dnd_task_node_m3hk1zc9tfp",
                        "description": "List the networks that the user has privileges on in an organization",
                        "xPos": 463,
                        "yPos": 142,
                        "needsToWait": false,
                        "fields": {
                            "headers": [
                                {
                                    "value": "",
                                    "key": "X-Cisco-Meraki-API-Key"
                                }
                            ],
                            "body": null,
                            "method": "GET",
                            "pathParams": {
                                "organizationId": "701665"
                            },
                            "targetUrl": "https://api.meraki.com/api/v1/organizations/:organizationId/networks",
                            "queryParams": {}
                        },
                        "prev": {
                            "true": null,
                            "false": null
                        },
                        "next": {
                            "true": "dnd_conditional_node_lp40540crbc",
                            "false": null
                        },
                        "assets": {
                            "schema": [],
                            "objects": null
                        },
                        "pathParamsPair": [
                            {
                                "key": "organizationId",
                                "value": "701665"
                            }
                        ],
                        "queryParamsPair": [],
                        "integrationId": "850b1f5d-57f9-42a3-becd-fb623c364ff6"
                    },
                    {
                        "name": "Conditional",
                        "vendor": null,
                        "category": null,
                        "type": "conditional",
                        "taskId": null,
                        "reactId": "dnd_conditional_node_lp40540crbc",
                        "description": null,
                        "xPos": 520,
                        "yPos": 372,
                        "needsToWait": true,
                        "fields": {
                            "expression": [
                                {
                                    "conditions": [
                                        {
                                            "op": "",
                                            "id": "2",
                                            "comparitor": "==",
                                            "var2": "701665",
                                            "var1": "{{OUTPUT:List the Networks in an Organization[0].organizationId}}"
                                        },
                                        {
                                            "op": "OR",
                                            "id": "3",
                                            "comparitor": "!=",
                                            "var2": "{{xpertlyRequestToken}}",
                                            "var1": "test"
                                        }
                                    ],
                                    "op": null,
                                    "id": "1"
                                }
                            ],
                            "pathParams": {},
                            "queryParams": {}
                        },
                        "output": null,
                        "prev": {
                            "true": "dnd_task_node_m3hk1zc9tfp",
                            "false": null
                        },
                        "next": {
                            "true": "dnd_task_node_ttuc9bt74z",
                            "false": null
                        },
                        "assets": {
                            "schema": null,
                            "objects": null
                        },
                        "pathParamsPair": [],
                        "queryParamsPair": [],
                        "integrationId": ""
                    },
                    {
                        "name": "List the Organizations",
                        "vendor": "meraki",
                        "category": null,
                        "type": "endpoint",
                        "taskId": "f3c14b71-9ec9-407d-a68b-1811ef5ce98e",
                        "reactId": "dnd_task_node_ttuc9bt74z",
                        "description": "List the organizations that the user has privileges on",
                        "xPos": 396,
                        "yPos": 553,
                        "needsToWait": false,
                        "fields": {
                            "headers": [
                                {
                                    "value": "",
                                    "key": "X-Cisco-Meraki-API-Key"
                                }
                            ],
                            "body": null,
                            "method": "GET",
                            "pathParams": {},
                            "targetUrl": "https://api.meraki.com/api/v1/organizations",
                            "queryParams": {}
                        },
                        "output": {
                            "name": "My organization",
                            "url": "https://dashboard.meraki.com/o/VjjsAd/manage/organization/overview",
                            "id": "2930418"
                        },
                        "prev": {
                            "true": "dnd_conditional_node_lp40540crbc",
                            "false": null
                        },
                        "next": {
                            "true": null,
                            "false": null
                        },
                        "assets": {
                            "schema": [],
                            "objects": null
                        },
                        "pathParamsPair": [],
                        "queryParamsPair": [],
                        "integrationId": "850b1f5d-57f9-42a3-becd-fb623c364ff6"
                    }
                ],
                "global": {},
                "custom": null,
                "tenantId": "537c096f-1862-476a-ad34-2dd2e8c16626"
            }"#;

        // deserialize the json string to a WorkerConfig struct
        let worker_config: WorkerConfig = serde_json::from_str(&worker_config).unwrap();
        dbg!(&worker_config);
        let worker = Worker::from_config(&worker_config).unwrap();
        let token = "eyJraWQiOiJVMnJhUUY5ZnpKOThsSUpyekZMSEgyRzhnRFNTaU5SODJ6Z3YxQzg5YU5BPSIsImFsZyI6IlJTMjU2In0.eyJzdWIiOiJlMmI5OWMwZS0xZjE4LTQ4NzgtYmI4Yi04MzZlMWRhM2I2ZGIiLCJldmVudF9pZCI6ImE0YmE2OTg2LWY0NjMtNGQ1ZS1hNDk5LWQ5NjcyZmY4ZTYyYSIsInRva2VuX3VzZSI6ImFjY2VzcyIsInNjb3BlIjoiYXdzLmNvZ25pdG8uc2lnbmluLnVzZXIuYWRtaW4iLCJhdXRoX3RpbWUiOjE2NzMzMjUxNTksImlzcyI6Imh0dHBzOlwvXC9jb2duaXRvLWlkcC5hcC1zb3V0aGVhc3QtMi5hbWF6b25hd3MuY29tXC9hcC1zb3V0aGVhc3QtMl9yZjdocG5nYlkiLCJleHAiOjE2NzU1NzU5MzAsImlhdCI6MTY3NTU3MjMzMSwianRpIjoiMTFjZjlmODMtZmRhZS00MzM3LWI2NTctOWEyYTVjYzUyNWY4IiwiY2xpZW50X2lkIjoiNXVpZXVmODZiYzR0NzM3bmhnbHZyMmQ4bmkiLCJ1c2VybmFtZSI6ImUyYjk5YzBlLTFmMTgtNDg3OC1iYjhiLTgzNmUxZGEzYjZkYiJ9.q7UT6RcKtldmVviMGXHL4JjRwQLw2Ifa4zo2ln4uKFoUXtqSwd4LzxTFVk_aRJQSInlCRW9GNc3LQmjDnig7Yj9kSV30pGkIyrUJCLQSCgzqU-hD4uCfWpe_Kl-fyTuihZTNWAv-QfYNLBMpe7rk3mQUVH4D_2g1-KkOuGLHZiDeYDgDmmGiozRAGp26jsOSjtW9K0AaAHAlAPcYCHsbpYxS3Y5uSn24PB4o_6iBYPHTQdsrGQBhcM8h8BY_Gjdhkpw05ESBTLUMbWrAHqshg6H0_1_ws0tlu4iq6_J2TMGfFx0aetUMnHL4NSdyop84hxoL2rcyId1d0wUgrRXU_g";

        let user = AvicennaUser {
            tenant_id: Uuid::from_str("537c096f-1862-476a-ad34-2dd2e8c16626").unwrap(),
            tenant_name: String::from("Pat's Org"),
            user_id: Uuid::from_str("e2b99c0e-1f18-4878-bb8b-836e1da3b6db").unwrap(),
            first_name: "Patrick".to_string(),
            last_name: "Ackland".to_string(),
            user_email: "packland@overip.io".to_string(),
            xpertly_executions: XpertlyExecutions {
                count: Some(0),
                quota: Some(99999),
            },
            role: UserRole::Owner,
        };

        // run the worker
        execute_worker(None, worker, token, user)
    }

    #[test]
    fn test_resume() {
        let suspended_worker = r#"{
            "tenantId": "537c096f-1862-476a-ad34-2dd2e8c16626",
            "triggeredBy": "packland@overip.io",
            "triggeredById": "e2b99c0e-1f18-4878-bb8b-836e1da3b6db",
            "worker":{
                "name":"Rust test",
                "id":"560ca980-1f0d-4987-a883-589f2878d966",
                "type":"Test",
                "availableInAvicenna":false,
                "description":"testing the rust backend",
                "tenantId":"537c096f-1862-476a-ad34-2dd2e8c16626",
                "latestTask": "dnd_task_node_ttuc9bt74z",
                "start": "dnd_conditional_node_lp40540crbc",
                "tasks": {
                    "dnd_task_node_m3hk1zc9tfp":{
                        "name":"List the Networks in an Organization",
                        "react_id":"dnd_task_node_m3hk1zc9tfp",
                        "next":{
                            "true":"dnd_conditional_node_lp40540crbc",
                            "false":null
                        },
                        "assets":{
                            "schema":[],
                            "objects":null
                        },
                        "needs_to_wait":false,
                        "handler":{
                            "Endpoint":{
                                "vendor":"meraki",
                                "integrationId":"850b1f5d-57f9-42a3-becd-fb623c364ff6",
                                "method":"GET",
                                "headers":[
                                    {
                                        "key":"X-Cisco-Meraki-API-Key",
                                        "value":""
                                    }
                                ],
                                "pathParams":{
                                    "organizationId":"701665"
                                },
                                "queryParams":{},
                                "body":null,
                                "targetUrl":"https://api.meraki.com/api/v1/organizations/:organizationId/networks"
                            }
                        }
                    },
                    "dnd_task_node_ttuc9bt74z":{
                        "name":"List the Organizations",
                        "react_id":"dnd_task_node_ttuc9bt74z",
                        "next":{
                            "true":null,
                            "false":null
                        },
                        "assets":{
                            "schema":[],
                            "objects":null
                        },
                        "needs_to_wait":false,
                        "handler":{
                            "Endpoint":{
                                "vendor":"meraki",
                                "integrationId":"850b1f5d-57f9-42a3-becd-fb623c364ff6",
                                "method":"GET",
                                "headers":[
                                    {
                                        "key":"X-Cisco-Meraki-API-Key",
                                        "value":""
                                    }
                                ],
                                "pathParams":{},
                                "queryParams":{},
                                "body":null,
                                "targetUrl":"https://api.meraki.com/api/v1/organizations"
                            }
                        }
                    },
                    "dnd_conditional_node_lp40540crbc":{
                        "name":"Conditional",
                        "react_id":"dnd_conditional_node_lp40540crbc",
                        "next":{
                            "true":"dnd_task_node_ttuc9bt74z",
                            "false":null
                        },
                        "assets":{
                            "schema":null,
                            "objects":null
                        },
                        "needs_to_wait":true,
                        "handler":{
                            "Conditional":{
                                "expression":[
                                    {
                                        "op":null,
                                        "conditions":[
                                            {
                                                "op":"",
                                                "comparitor":"==",
                                                "var1":"{{OUTPUT:List the Networks in an Organization[0].organizationId}}",
                                                "var2":"701665"
                                            }
                                        ]
                                    }
                                ]
                            }
                        }
                    }
                }
            },
            "executionId":"27f8856d-1ce7-481e-a3b2-92dec96499c1",
            "runId":"10602fe9-b53b-4ce4-98f5-144c2618193f",
            "tag":null,
            "captureEventId":null,
            "authToken":"eyJraWQiOiJVMnJhUUY5ZnpKOThsSUpyekZMSEgyRzhnRFNTaU5SODJ6Z3YxQzg5YU5BPSIsImFsZyI6IlJTMjU2In0.eyJzdWIiOiJlMmI5OWMwZS0xZjE4LTQ4NzgtYmI4Yi04MzZlMWRhM2I2ZGIiLCJldmVudF9pZCI6ImE0YmE2OTg2LWY0NjMtNGQ1ZS1hNDk5LWQ5NjcyZmY4ZTYyYSIsInRva2VuX3VzZSI6ImFjY2VzcyIsInNjb3BlIjoiYXdzLmNvZ25pdG8uc2lnbmluLnVzZXIuYWRtaW4iLCJhdXRoX3RpbWUiOjE2NzMzMjUxNTksImlzcyI6Imh0dHBzOlwvXC9jb2duaXRvLWlkcC5hcC1zb3V0aGVhc3QtMi5hbWF6b25hd3MuY29tXC9hcC1zb3V0aGVhc3QtMl9yZjdocG5nYlkiLCJleHAiOjE2NzUxNjA2MjYsImlhdCI6MTY3NTE1NzAyNiwianRpIjoiNzdmMTU4NGMtMmI0Yi00YzEyLTk4NjgtMTIxMDhmOGIwNzEwIiwiY2xpZW50X2lkIjoiNXVpZXVmODZiYzR0NzM3bmhnbHZyMmQ4bmkiLCJ1c2VybmFtZSI6ImUyYjk5YzBlLTFmMTgtNDg3OC1iYjhiLTgzNmUxZGEzYjZkYiJ9.b7b971EqwRCypTDytWy9FrXWF_gF4UO59l_fNSzG92yj-VW3XAtHhdVvpLOfgl-ubP-pjwg-EmNSgJUSHIri3IeZyq3hRZ-fTTQmhaBozTK_ZJJpE-Off-jCi9ZJlgMsI0I0a7K8KUhgltSbRah0sxgXYw4ZqqkM_iadxqToD8qlVPr8pL-wbY5t1_-VrdIleQAAnF2zwhhr4j1rBSOS1i89HUvXRyfG4VFXzfwjlik4vvEN5o-6jOkivAig8DLK7CrlcXDghJcJKzLDQnqE6lexCZJne1XXHkFtRnhTIZNj2gKHrljTrysyrQHAVtZtrbZ-ZidT2h8xErsfMIejJg",
            "outputs":{
                "dnd_task_node_m3hk1zc9tfp":[
                    {
                        "configTemplateId":"L_669347494617948659",
                        "enrollmentString":null,
                        "id":"L_726205439913493040",
                        "isBoundToConfigTemplate":true,
                        "name":"Demo Store 2",
                        "notes":"",
                        "organizationId":"701665",
                        "productTypes":[
                            "appliance",
                            "switch",
                            "wireless"
                        ],
                        "tags":[],
                        "timeZone":"Australia/Sydney",
                        "url":"https://n290.meraki.com/Demo-Store-2-swi/n/td534cIe/manage/usage/list"
                    },
                    {
                        "enrollmentString":null,
                        "id":"L_726205439913496270",
                        "isBoundToConfigTemplate":false,
                        "name":"Xpertly Demo",
                        "notes":null,
                        "organizationId":"701665",
                        "productTypes":[
                            "appliance",
                            "switch",
                            "wireless"
                        ],
                        "tags":[],
                        "timeZone":"Australia/Sydney",
                        "url":"https://n290.meraki.com/Xpertly-Demo-app/n/3ZUK0dIe/manage/usage/list"
                    },
                    {
                        "enrollmentString":null,
                        "id":"L_726205439913496528",
                        "isBoundToConfigTemplate":false,
                        "name":"Core",
                        "notes":null,
                        "organizationId":"701665",
                        "productTypes":[
                            "appliance",
                            "camera",
                            "cellularGateway",
                            "sensor",
                            "switch",
                            "wireless"
                        ],
                        "tags":[],
                        "timeZone":"America/Los_Angeles",
                        "url":"https://n290.meraki.com/Core-cellular-ga/n/UkakocIe/manage/usage/list"
                    },
                    {
                        "enrollmentString":null,
                        "id":"L_726205439913497503",
                        "isBoundToConfigTemplate":false,
                        "name":"test networ",
                        "notes":null,
                        "organizationId":"701665",
                        "productTypes":[
                            "appliance",
                            "wireless"
                        ],
                        "tags":[],
                        "timeZone":"Australia/Sydney",
                        "url":"https://n290.meraki.com/test-networ-appl/n/KT7XYaIe/manage/usage/list"
                    },
                    {
                        "enrollmentString":null,
                        "id":"L_726205439913497507",
                        "isBoundToConfigTemplate":false,
                        "name":"WWtest",
                        "notes":null,
                        "organizationId":"701665",
                        "productTypes":[
                            "appliance",
                            "wireless"
                        ],
                        "tags":[],
                        "timeZone":"Australia/Sydney",
                        "url":"https://n290.meraki.com/WWtest-appliance/n/KsgqpcIe/manage/usage/list"
                    },
                    {
                        "enrollmentString":null,
                        "id":"L_726205439913497692",
                        "isBoundToConfigTemplate":false,
                        "name":"OverIP",
                        "notes":null,
                        "organizationId":"701665",
                        "productTypes":[
                            "appliance",
                            "wireless"
                        ],
                        "tags":[],
                        "timeZone":"Australia/Sydney",
                        "url":"https://n290.meraki.com/OverIP-wireless/n/XF_H0cIe/manage/usage/list"
                    },
                    {
                        "enrollmentString":null,
                        "id":"L_726205439913497769",
                        "isBoundToConfigTemplate":false,
                        "name":"pat test",
                        "notes":null,
                        "organizationId":"701665",
                        "productTypes":[
                            "appliance",
                            "camera",
                            "cellularGateway",
                            "sensor",
                            "switch",
                            "wireless"
                        ],
                        "tags":[],
                        "timeZone":"America/Los_Angeles",
                        "url":"https://n290.meraki.com/pat-test-cellula/n/1hx7CbIe/manage/usage/list"
                    },
                    {
                        "enrollmentString":null,
                        "id":"L_726205439913498016",
                        "isBoundToConfigTemplate":false,
                        "name":"Test Network",
                        "notes":null,
                        "organizationId":"701665",
                        "productTypes":[
                            "appliance",
                            "wireless"
                        ],
                        "tags":[],
                        "timeZone":"Australia/Sydney",
                        "url":"https://n290.meraki.com/Test-Network-app/n/5lGotaIe/manage/usage/list"
                    },
                    {
                        "enrollmentString":null,
                        "id":"L_726205439913498059",
                        "isBoundToConfigTemplate":false,
                        "name":"AI Test",
                        "notes":null,
                        "organizationId":"701665",
                        "productTypes":[
                            "appliance",
                            "wireless"
                        ],
                        "tags":[],
                        "timeZone":"Australia/Sydney",
                        "url":"https://n290.meraki.com/AI-Test-wireless/n/hMRzcaIe/manage/usage/list"
                    },
                    {
                        "enrollmentString":null,
                        "id":"N_669347494618000631",
                        "isBoundToConfigTemplate":false,
                        "name":"VMX-TEST",
                        "notes":null,
                        "organizationId":"701665",
                        "productTypes":[
                            "appliance"
                        ],
                        "tags":[],
                        "timeZone":"America/Los_Angeles",
                        "url":"https://n290.meraki.com/VMX-TEST/n/r5o92b9c/manage/usage/list"
                    },
                    {
                        "enrollmentString":null,
                        "id":"N_726205439913495706",
                        "isBoundToConfigTemplate":false,
                        "name":"Demo Store - Camera",
                        "notes":"",
                        "organizationId":"701665",
                        "productTypes":["camera"],
                        "tags":[],
                        "timeZone":"Australia/Sydney",
                        "url":"https://n290.meraki.com/Demo-Store-Camer/n/yVgD0bIe/manage/usage/list"
                    },
                    {
                        "configTemplateId":"L_669347494617948659",
                        "enrollmentString":null,
                        "id":"N_726205439913496264",
                        "isBoundToConfigTemplate":true,
                        "name":"Sherif Lab",
                        "notes":"",
                        "organizationId":"701665",
                        "productTypes":["appliance"],
                        "tags":[],
                        "timeZone":"Australia/NSW",
                        "url":"https://n290.meraki.com/Sherif-Lab/n/WhbjRdIe/manage/usage/list"
                    }
                ],
                "dnd_task_node_ttuc9bt74z":[
                    {
                        "api":{"enabled":true},
                        "cloud":{"region":{"name":"Asia"}},
                        "id":"669347494617941911",
                        "licensing":{"model":"co-term"},
                        "management":{"details":[]},
                        "name":"BT Demo Organisation",
                        "url":"https://n189.meraki.com/o/Wb6LHd9c/manage/organization/overview"
                    },
                    {
                        "api":{"enabled":false},
                        "cloud":{"region":{"name":"Asia"}},
                        "id":"735121",
                        "licensing":{"model":"co-term"},
                        "management":{"details":[]},
                        "name":"Caltex Digital",
                        "url":"https://n290.meraki.com/o/FPbfcb/manage/organization/overview"
                    },
                    {
                        "api":{"enabled":true},
                        "cloud":{"region":{"name":"Asia"}},
                        "id":"701665",
                        "licensing":{"model":"co-term"},
                        "management":{"details":[]},
                        "name":"OverIP",
                        "url":"https://n290.meraki.com/o/hVK5zd/manage/organization/overview"
                    }
                ],
                "dnd_conditional_node_lp40540crbc":true
            },
            "assets":{
                "schema":[],
                "objects":[]
            }
        }"#;
        println!("test");
        let suspended_worker_value = serde_json::from_str(&suspended_worker).unwrap();
        let invocation = WorkerInvocation::from_suspended(suspended_worker_value).unwrap();
        resume_worker(invocation);
    }

    #[test]
    fn test_substitution() {
        let worker = r#"{
            "id": "4fa0481c-5b5a-4732-86a8-d0fbbde55e6e",
            "schemaId": null,
            "name": "Work Package 0",
            "type": null,
            "availableInAvicenna": false,
            "description": "Work Package 0",
            "tasks": [
                {
                    "name": "Launch a Job Template",
                    "vendor": "ansible",
                    "category": "Job Templates",
                    "type": "endpoint",
                    "taskId": "dc97363b-f971-4e55-9493-97e6c8da3d26",
                    "reactId": "dnd_task_node_wdm8falcdte",
                    "description": "Launch a Job Template",
                    "xPos": 531,
                    "yPos": 174,
                    "needsToWait": true,
                    "fields": {
                        "headers": [
                            {
                                "value": "application/json",
                                "key": "Content-Type"
                            },
                            {
                                "value": "application/json",
                                "key": "Accept"
                            }
                        ],
                        "body": {
                            "extra_vars": {
                                "token": "{{xpertlyRequestToken}}"
                            }
                        },
                        "method": "POST",
                        "pathParams": {
                            "id": "10"
                        },
                        "targetUrl": "{{ansibleHostname}}/api/v2/job_templates/:id/launch/",
                        "queryParams": {}
                    },
                    "output": "",
                    "prev": {
                        "true": null,
                        "false": null
                    },
                    "next": {
                        "true": "dnd_conditional_node_6ecrssooz9h",
                        "false": null
                    },
                    "assets": {
                        "schema": [],
                        "objects": null
                    },
                    "pathParamsPair": [
                        {
                            "key": "id",
                            "value": "10"
                        }
                    ],
                    "queryParamsPair": [],
                    "integrationId": "9039e1f8-d492-4224-9c09-0dc6de980d60"
                },
                {
                    "name": "Post RAM Non-Compliance",
                    "vendor": "splunk",
                    "category": "HTTP Event Collector",
                    "type": "endpoint",
                    "taskId": "a75977ed-a65f-4cd3-9fed-c34eb9f3ea05",
                    "reactId": "dnd_task_node_toxcs75noir",
                    "description": "Sends timestamped events to the HTTP Event Collector using the Splunk platform JSON event protocol when you set the auto_extract_timestamp argument to true in the /event URL.",
                    "xPos": 723,
                    "yPos": 423,
                    "needsToWait": false,
                    "fields": {
                        "headers": null,
                        "body": {
                            "event": {
                                "message": "RAM not within compliance. {{OUTPUT:Launch a Job Template.customOutput.RAM}}",
                                "Site ID": "{{GLOBAL:Site ID}}"
                            },
                            "index": "soinetworks_ciscosdwan_nonprod"
                        },
                        "method": "POST",
                        "pathParams": {},
                        "targetUrl": "{{hostname}}:{{port}}/services/collector/event",
                        "queryParams": {}
                    },
                    "output": "",
                    "prev": {
                        "true": "dnd_conditional_node_6ecrssooz9h",
                        "false": null
                    },
                    "next": {
                        "true": "dnd_conditional_node_pgtjpq0otuk",
                        "false": null
                    },
                    "assets": {
                        "schema": [],
                        "objects": null
                    },
                    "pathParamsPair": [],
                    "queryParamsPair": [],
                    "integrationId": "8c572a0a-837f-48d6-a328-4b4e62bac285"
                }
            ],
            "global": {
                "Site ID": "Site 6"
            },
            "custom": {
                "Site ID": "ansible.router.siteId"
            },
            "tenantId": "ab5623ba-d5b2-4289-9906-5502223447cc"
        }"#;

        let worker_config = serde_json::from_str::<WorkerConfig>(worker).unwrap();
        let worker = Worker::from_config(&worker_config).unwrap();
        dbg!(&worker.tasks);
        let task = worker.tasks.get("dnd_task_node_toxcs75noir").unwrap();
        let mut invocation = WorkerInvocation {
            tenant_id: worker.tenant_id.clone(),
            triggered_by: "packland@testing.com".to_string(),
            triggered_by_id: Uuid::from_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").unwrap(),
            worker: worker.clone(),
            execution_id: Uuid::new_v4(),
            run_id: Uuid::new_v4(),
            tag: None,
            auth_token: "not-a-real-token".to_string(),
            outputs: Arc::new(Mutex::new(HashMap::<String, serde_json::Value>::new())),
            state: Arc::new(Mutex::new(InvocationState::Running)),
            client: reqwest::Client::new(),
            assets: Arc::new(Mutex::new(Assets::new())),
            channel: None,
            wait_token: "adsofnsdlfn".to_string(),
        };

        invocation.outputs.lock().unwrap().insert(
            "dnd_task_node_wdm8falcdte".to_string(),
            serde_json::json!({"customOutput": {"RAM": 111}}),
        );
        let rendered_task = invocation.render_variables(task);
        dbg!(rendered_task);
    }

    #[test]
    fn test_serialization() {
        let worker_str = r#"{
              "id": "ccdde071-c089-400e-8527-505c5a5c9e46",
              "schemaId": null,
              "name": "Conditional test",
              "type": null,
              "availableInAvicenna": false,
              "description": "conditional only",
              "tasks": [
                {
                  "name": "Conditional",
                  "vendor": null,
                  "category": null,
                  "type": "conditional",
                  "taskId": null,
                  "reactId": "dnd_conditional_node_6qbe0c0bbde",
                  "description": null,
                  "xPos": 861,
                  "yPos": 271,
                  "needsToWait": false,
                  "fields": {
                    "expression": [
                      {
                        "conditions": [
                          {
                            "op": "",
                            "id": "2",
                            "comparitor": "==",
                            "var2": "1",
                            "var1": "1"
                          }
                        ],
                        "op": null,
                        "id": "1"
                      },
                      {
                        "conditions": [
                          {
                            "op": "AND",
                            "id": "4",
                            "comparitor": "<",
                            "var2": "3",
                            "var1": "2"
                          },
                          {
                            "op": "",
                            "id": "5",
                            "comparitor": ">",
                            "var2": "2",
                            "var1": "4"
                          }
                        ],
                        "op": "AND",
                        "id": "3"
                      },
                      {
                        "conditions": [
                          {
                            "op": "OR",
                            "id": "7",
                            "comparitor": "!=",
                            "var2": "notstring",
                            "var1": "string"
                          },
                          {
                            "op": "",
                            "id": "8",
                            "comparitor": "==",
                            "var2": "test",
                            "var1": "test"
                          }
                        ],
                        "op": "OR",
                        "id": "6"
                      }
                    ]
                  },
                  "output": null,
                  "prev": { "true": null, "false": null },
                  "next": { "true": "dnd_conditional_node_9n5u4zjsom", "false": null },
                  "assets": { "schema": null, "objects": null },
                  "pathParamsPair": [],
                  "queryParamsPair": [],
                  "integrationId": ""
                },
                {
                  "name": "Conditional",
                  "vendor": null,
                  "category": null,
                  "type": "conditional",
                  "taskId": null,
                  "reactId": "dnd_conditional_node_9n5u4zjsom",
                  "description": null,
                  "xPos": 753,
                  "yPos": 687,
                  "needsToWait": false,
                  "fields": {
                    "expression": [
                      {
                        "conditions": [
                          {
                            "op": "",
                            "id": "2",
                            "comparitor": "==",
                            "var2": "success",
                            "var1": "great"
                          }
                        ],
                        "op": null,
                        "id": "1"
                      }
                    ],
                    "pathParams": {},
                    "queryParams": {}
                  },
                  "output": null,
                  "prev": { "true": "dnd_conditional_node_6qbe0c0bbde", "false": null },
                  "next": { "true": null, "false": null },
                  "assets": { "schema": null, "objects": null },
                  "pathParamsPair": [],
                  "queryParamsPair": [],
                  "integrationId": ""
                }
              ],
              "global": {},
              "custom": {},
              "tenantId": "537c096f-1862-476a-ad34-2dd2e8c16626"
            }"#;

        let worker_config = serde_json::from_str::<WorkerConfig>(worker_str).unwrap();
    }

    #[test]
    fn test_loop() -> Result<(), Box<dyn std::error::Error>> {
        let worker_str = r#"{
            "tags": [
              "networks1"
            ],
            "worker": {
              "id": "60ae1be8-e28c-4c0a-87d8-7bf36b46baa8",
              "schemaId": null,
              "name": "Loop Test",
              "type": null,
              "availableInAvicenna": false,
              "description": "testing loops",
              "tasks": [
                {
                  "name": "List the Networks in an Organization",
                  "vendor": "meraki",
                  "category": null,
                  "type": "endpoint",
                  "taskId": "2b8cc662-fa44-426d-a1e0-1f1b50416e9c",
                  "reactId": "dnd_task_node_0q5ob4kiwtrf",
                  "description": "List the networks that the user has privileges on in an organization",
                  "xPos": 561,
                  "yPos": 198,
                  "needsToWait": false,
                  "fields": {
                    "headers": [
                      {
                        "value": "",
                        "key": "X-Cisco-Meraki-API-Key"
                      }
                    ],
                    "body": null,
                    "method": "GET",
                    "pathParams": {
                      "organizationId": "701665"
                    },
                    "targetUrl": "https://api.meraki.com/api/v1/organizations/:organizationId/networks",
                    "queryParams": {}
                  },
                  "output": {
                    "organizationId": "2930418",
                    "name": "Long Island Office",
                    "timeZone": "America/Los_Angeles",
                    "productTypes": [
                      "appliance",
                      "switch",
                      "wireless"
                    ],
                    "id": "L_123456",
                    "enrollmentString": "long-island-office",
                    "tags": [
                      "tag1",
                      "tag2"
                    ]
                  },
                  "prev": {
                    "true": null,
                    "false": null
                  },
                  "next": {
                    "true": "dnd_group_node_hpcry9tkup",
                    "false": null
                  },
                  "assets": {
                    "schema": [],
                    "objects": null
                  },
                  "pathParamsPair": [
                    {
                      "key": "organizationId",
                      "value": "701665"
                    }
                  ],
                  "queryParamsPair": [],
                  "integrationId": "850b1f5d-57f9-42a3-becd-fb623c364ff6"
                },
                {
                  "name": "Looping Task",
                  "vendor": null,
                  "category": null,
                  "type": "loop",
                  "taskId": null,
                  "reactId": "dnd_group_node_hpcry9tkup",
                  "description": null,
                  "xPos": 548,
                  "yPos": 310,
                  "needsToWait": false,
                  "fields": {
                    "width": 477,
                    "tasks": [
                      {
                        "next": {
                          "true": "dnd_task_node_6auf2ys6blu",
                          "false": null
                        },
                        "yPos": 55,
                        "prev": {
                          "true": null,
                          "false": null
                        },
                        "description": "List the devices in a network",
                        "xPos": 88,
                        "type": "endpoint",
                        "output": {
                          "address": "1600 Pennsylvania Ave",
                          "notes": "My AP's note",
                          "floorPlanId": "g_1234567",
                          "lng": "-122.098531723022",
                          "beaconIdParams": {
                            "major": "5",
                            "minor": "3",
                            "uuid": "00000000-0000-0000-0000-000000000000"
                          },
                          "mac": "00:11:22:33:44:55",
                          "tags": " recently-added ",
                          "serial": "Q234-ABCD-5678",
                          "name": "My AP",
                          "model": "MR34",
                          "networkId": "N_24329156",
                          "firmware": "wireless-25-14",
                          "lat": "37.4180951010362",
                          "lanIp": "1.2.3.4"
                        },
                        "needsToWait": false,
                        "assets": {
                          "schema": [
                            {
                              "vendor": "meraki",
                              "assetType": "network"
                            }
                          ],
                          "objects": null
                        },
                        "vendor": "meraki",
                        "name": "List The Devices In A Network",
                        "category": "GENERAL#Networks#Configure#Devices",
                        "fields": {
                          "headers": [
                            {
                              "value": "",
                              "key": "X-Cisco-Meraki-API-Key"
                            }
                          ],
                          "body": null,
                          "method": "GET",
                          "pathParams": {
                            "networkId": "{{ASSET:meraki.network.id}}"
                          },
                          "targetUrl": "https://api.meraki.com/api/v1/networks/:networkId/devices",
                          "queryParams": {}
                        },
                        "taskId": "cb512cbb-3bb9-4edd-94ae-a9dbc80beb19",
                        "reactId": "dnd_task_node_p4hb3pzbti",
                        "pathParamsPair": [
                          {
                            "key": "networkId",
                            "value": "{{ASSET:meraki.network.id}}"
                          }
                        ],
                        "queryParamsPair": [],
                        "integrationId": "850b1f5d-57f9-42a3-becd-fb623c364ff6"
                      },
                      {
                        "next": {
                          "true": null,
                          "false": null
                        },
                        "yPos": 186,
                        "prev": {
                          "true": "dnd_task_node_p4hb3pzbti",
                          "false": null
                        },
                        "description": "List the devices in a network",
                        "xPos": 77,
                        "type": "endpoint",
                        "output": {
                          "address": "1600 Pennsylvania Ave",
                          "notes": "My AP's note",
                          "floorPlanId": "g_1234567",
                          "lng": "-122.098531723022",
                          "beaconIdParams": {
                            "major": "5",
                            "minor": "3",
                            "uuid": "00000000-0000-0000-0000-000000000000"
                          },
                          "mac": "00:11:22:33:44:55",
                          "tags": " recently-added ",
                          "serial": "Q234-ABCD-5678",
                          "name": "My AP",
                          "model": "MR34",
                          "networkId": "N_24329156",
                          "firmware": "wireless-25-14",
                          "lat": "37.4180951010362",
                          "lanIp": "1.2.3.4"
                        },
                        "needsToWait": false,
                        "assets": {
                          "schema": [
                            {
                              "vendor": "meraki",
                              "assetType": "network"
                            }
                          ],
                          "objects": null
                        },
                        "vendor": "meraki",
                        "name": "List The Devices In A Network 2",
                        "category": "GENERAL#Networks#Configure#Devices",
                        "fields": {
                          "headers": [
                            {
                              "value": "",
                              "key": "X-Cisco-Meraki-API-Key"
                            }
                          ],
                          "body": null,
                          "method": "GET",
                          "pathParams": {
                            "networkId": "{{ASSET:meraki.network.id}}"
                          },
                          "targetUrl": "https://api.meraki.com/api/v1/networks/:networkId/devices",
                          "queryParams": {}
                        },
                        "taskId": "cb512cbb-3bb9-4edd-94ae-a9dbc80beb19",
                        "reactId": "dnd_task_node_6auf2ys6blu",
                        "pathParamsPair": [
                          {
                            "key": "networkId",
                            "value": "{{ASSET:meraki.network.id}}"
                          }
                        ],
                        "queryParamsPair": [],
                        "integrationId": "850b1f5d-57f9-42a3-becd-fb623c364ff6"
                      }
                    ],
                    "height": 289
                  },
                  "output": null,
                  "prev": {
                    "true": "dnd_task_node_0q5ob4kiwtrf",
                    "false": null
                  },
                  "next": {
                    "true": "dnd_task_node_tyft2aeo29",
                    "false": null
                  },
                  "assets": {
                    "schema": [
                      {
                        "vendor": "meraki",
                        "assetType": "network"
                      }
                    ],
                    "objects": null
                  }
                },
                {
                  "name": "List the Networks in an Organization",
                  "vendor": "meraki",
                  "category": null,
                  "type": "endpoint",
                  "taskId": "2b8cc662-fa44-426d-a1e0-1f1b50416e9c",
                  "reactId": "dnd_task_node_tyft2aeo29",
                  "description": "List the networks that the user has privileges on in an organization",
                  "xPos": 625,
                  "yPos": 712,
                  "needsToWait": false,
                  "fields": {
                    "headers": [
                      {
                        "value": "",
                        "key": "X-Cisco-Meraki-API-Key"
                      }
                    ],
                    "body": null,
                    "method": "GET",
                    "pathParams": {
                      "organizationId": "701665"
                    },
                    "targetUrl": "https://api.meraki.com/api/v1/organizations/:organizationId/networks",
                    "queryParams": {}
                  },
                  "output": {
                    "organizationId": "2930418",
                    "name": "Long Island Office",
                    "timeZone": "America/Los_Angeles",
                    "productTypes": [
                      "appliance",
                      "switch",
                      "wireless"
                    ],
                    "id": "L_123456",
                    "enrollmentString": "long-island-office",
                    "tags": [
                      "tag1",
                      "tag2"
                    ]
                  },
                  "prev": {
                    "true": "dnd_group_node_hpcry9tkup",
                    "false": null
                  },
                  "next": {
                    "true": null,
                    "false": null
                  },
                  "assets": {
                    "schema": [],
                    "objects": null
                  },
                  "pathParamsPair": [
                    {
                      "key": "organizationId",
                      "value": "701665"
                    }
                  ],
                  "queryParamsPair": [],
                  "integrationId": "850b1f5d-57f9-42a3-becd-fb623c364ff6"
                }
              ],
              "global": {},
              "custom": {},
              "tenantId": "537c096f-1862-476a-ad34-2dd2e8c16626"
            },
            "testPlanId": null
        }"#;
        let client = reqwest::blocking::Client::new();
        let worker_json = serde_json::from_str::<serde_json::Value>(worker_str)?;
        dbg!("starting");
        let worker_conf = serde_json::from_value::<WorkerConfig>(worker_json["worker"].clone())?;
        dbg!("deserializing worker config");
        let worker = Worker::from_config(&worker_conf)?;
        dbg!("decoding token");
        let token = "eyJraWQiOiJVMnJhUUY5ZnpKOThsSUpyekZMSEgyRzhnRFNTaU5SODJ6Z3YxQzg5YU5BPSIsImFsZyI6IlJTMjU2In0.eyJzdWIiOiJlMmI5OWMwZS0xZjE4LTQ4NzgtYmI4Yi04MzZlMWRhM2I2ZGIiLCJldmVudF9pZCI6ImUyZDhiYzg1LWRmNDctNDBmNS1iZjYyLTBhMjhlMmM1M2Q0ZSIsInRva2VuX3VzZSI6ImFjY2VzcyIsInNjb3BlIjoiYXdzLmNvZ25pdG8uc2lnbmluLnVzZXIuYWRtaW4iLCJhdXRoX3RpbWUiOjE2Nzk0NTExNDAsImlzcyI6Imh0dHBzOlwvXC9jb2duaXRvLWlkcC5hcC1zb3V0aGVhc3QtMi5hbWF6b25hd3MuY29tXC9hcC1zb3V0aGVhc3QtMl9yZjdocG5nYlkiLCJleHAiOjE2ODEyMTMzMjAsImlhdCI6MTY4MTIwOTcyMCwianRpIjoiODdkNWM5MmYtNWYxOC00MWIwLWFjMGItOTk5NWFkOTBlNjQwIiwiY2xpZW50X2lkIjoiNXVpZXVmODZiYzR0NzM3bmhnbHZyMmQ4bmkiLCJ1c2VybmFtZSI6ImUyYjk5YzBlLTFmMTgtNDg3OC1iYjhiLTgzNmUxZGEzYjZkYiJ9.hQo90nD1WXGfj-MSlPG9DK06CqmSz3JfHTXTOBe-s_m2OcjOMfobKW1J-IUvoQGoGDfSH9eR87hYzz-UeBFYIyYSe5aBaMiCkzq8_rd4nq5fXQN5JcoRYkyJESmZlvshOXRe641C5Kaj_Zbtyt8k84LsIPs8xTX1UHR8x97Zkkx9NE2AejbfD2fjZuNWUYOb_wtxzYAcIE7TZv8Rj5ZWeYgaMe1zDX5-8-VxikoMD-2pvKTLZ_sHnnn7Mp_swjO_oevgXXEoubHyQcaT4Y7NVXMhgECtSx9SrdiU4QH0voIOnocam-sqSxVjtN5n8iexSQlvQxZ4TXonNLJr4ePOUg";
        let headers = jsonwebtoken::decode_header(token)?;
        let kid = headers.kid.unwrap();
        let jwks: serde_json::Value = client.get("https://cognito-idp.ap-southeast-2.amazonaws.com/ap-southeast-2_rf7hpngbY/.well-known/jwks.json").send().unwrap().json().unwrap();
        let jwk = jwks["keys"]
            .as_array()
            .unwrap()
            .iter()
            .find(|jwk| jwk["kid"] == kid)
            .unwrap();
        let public_key = jsonwebtoken::DecodingKey::from_rsa_components(
            &jwk["n"].as_str().unwrap(),
            &jwk["e"].as_str().unwrap(),
        )?;
        let auth = jsonwebtoken::decode::<Claims>(
            token,
            &public_key,
            &jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::RS256),
        )?;
        let user_response = client
            .get(format!(
                "https://api.dev.avicenna.io/v1/tenants/{tenant_id}/users/{user_id}",
                tenant_id = "537c096f-1862-476a-ad34-2dd2e8c16626",
                user_id = auth.claims.username
            ))
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .unwrap();
        dbg!(&user_response);
        let resp_json = user_response.json::<serde_json::Value>().unwrap();
        let user = serde_json::from_value::<AvicennaUser>(resp_json).unwrap();
        let bearer_token = BearerToken::from_str(token).unwrap();
        let tags = worker_json["tags"]
            .as_array()
            .unwrap()
            .into_iter()
            .map(|tag| tag.as_str().unwrap().to_string())
            .collect();
        execute(&tags, worker, user, &bearer_token, Uuid::new_v4(), None);
        Ok(())
    }
}
