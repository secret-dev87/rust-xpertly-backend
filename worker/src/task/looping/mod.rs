use super::{Handler, Task, TaskOutput};
use crate::{Event, WorkerInvocation};
use anyhow::{bail, Result};
use core::str::FromStr;
use reqwest::header::{HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::json;
use xpertly_common::*;

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Loop {
    pub(crate) tasks: Vec<Task>,
    pub(crate) schema: Option<Vec<SchemaItem>>,
    pub(crate) loop_assets: Option<Vec<Object>>,
}

impl Loop {
    pub async fn prepare(&mut self, context: &WorkerInvocation) -> Result<()> {
        let tag = context.tag.as_ref().expect("Loop tasks require a tag");
        let result = context
            .client
            .get(&format!(
                "http://localhost:8000/api/tenants/{}/assets-by-tags",
                context.tenant_id
            ))
            .header(
                HeaderName::from_str("Authorization")?,
                HeaderValue::from_str(&context.auth_token)?,
            )
            .query(&[("tags", tag)])
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;

        let devices = result["devices"][tag].as_array();
        let assets = result["assets"][tag].as_array();
        let mut objects = Vec::new();

        if let Some(devices) = devices {
            devices.iter().for_each(|object| {
                dbg!(&object);
                objects.push(serde_json::from_value::<Object>(object.clone()).unwrap())
            });
        }

        if let Some(assets) = assets {
            assets.iter().for_each(|object| {
                dbg!(&object);
                objects.push(serde_json::from_value::<Object>(object.clone()).unwrap())
            });
        }

        self.loop_assets = Some(objects);
        Ok(())
    }

    pub async fn execute(&self, context: &WorkerInvocation) -> Result<()> {
        if let Some(objects) = &self.loop_assets {
            for object in objects {
                // create local loop context (probably clone the WorkerInvocation passed to this task)
                // local loop context wont live beyond this task
                // should enable inner tasks to reference each other within an iteration
                let mut loop_context = context.clone();
                let tag = context.tag.as_ref().expect("Loop tasks require a tag");
                // this needs to follow the `next` chain, same as in WorkerInvocation.
                // the two implementations should be merged somehow as the only difference is that this repeats
                // each contained task for each object in the loop_assets field.
                // ideally the "Next" object would be smarter and could somehow locate the task that is supposed to run
                // next and return it. Each task could store the context and the whole system would look more like a linked
                // list than a worker invocation that contains a list of tasks.
                for task in self.tasks.iter() {
                    loop_context
                        .log(Event::TaskStart, Some(&task), None, None)
                        .await;
                    let mut task = task.clone();
                    task.assets.add_object(tag, object.clone());
                    task.prepare(&loop_context).await?;

                    let mut task = match task.handler {
                        // turning off variable subsitiution for loops as inner tasks may not have required variables available yet
                        // those inner tasks will be rendered when they are executed
                        Handler::Loop(_) => task,
                        _ => {
                            // rendering twice is a workaround for a path parameter translation bug
                            let task = loop_context.render_variables(&task);
                            loop_context.render_variables(&task)
                        }
                    };

                    match task.execute(&loop_context).await {
                        Ok(task_result) => {
                            loop_context
                                .log(
                                    Event::TaskSuccess,
                                    Some(&task),
                                    Some(task_result.clone()),
                                    None,
                                )
                                .await;
                            match task_result {
                                TaskOutput::EndpointResult(result)
                                | TaskOutput::WebhookResult(result)
                                | TaskOutput::ConditionalResult(result) => {
                                    loop_context
                                        .outputs
                                        .lock()
                                        .unwrap()
                                        .insert(task.react_id.clone(), json!(result));
                                }
                                _ => {}
                            }
                        }
                        Err(e) => {
                            loop_context
                                .log(Event::TaskFail, Some(&task), None, Some(e))
                                .await;
                            bail!("Loop task failed because an inner task failed");
                        }
                    }
                }
            }

            // potentially produce some useful outputs from the loop task and save them to the global worker context,
            // e.g. the number of iterations or the outputs of each iteration behind an iteration key (or even the device that was iterated over)
            // later tasks can reference these outputs
        }
        Ok(())
    }
}
