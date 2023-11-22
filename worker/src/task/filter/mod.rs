use std::collections::HashMap;

use super::Task;
use crate::WorkerInvocation;
use anyhow::Result;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_json::json;
use tera::Tera;
use url::quirks::search;
use xpertly_common::*;

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Filter {
    pub object_to_filter: String,
    pub json_obj: Option<Value>,
    pub search_key: String,
    pub search_value: String,
    pub condition: String,
}

fn search_json(
    json_obj: &Value,
    search_key: String,
    search_value: String,
    condition: &str,
    parent: Option<Value>,
    found: bool,
    response: &mut Vec<Value>,
) {
    match json_obj {
        Value::Object(ref obj) => {
            if obj.contains_key(&search_key) {
                match condition {
                    "=" => {
                        // let keys: Vec<&String> = obj.keys().collect();
                        // dbg!(keys);
                        if search_value == obj[&search_key] {
                            match parent {
                                Some(ref par) => response.push(par.clone()),
                                None => response.push(json_obj.clone()),
                            }
                        }
                    }
                    "!=" => {
                        if search_value != obj[&search_key] {
                            match parent {
                                Some(ref par) => response.push(par.clone()),
                                None => response.push(json_obj.clone()),
                            }
                        }
                    }
                    "contains" => {
                        let var = obj.get(&search_key);
                        if let Some(var) = var {
                            match var {
                                Value::String(string) => {
                                    if string.contains(&search_value) {
                                        match parent {
                                            Some(ref par) => response.push(par.clone()),
                                            None => response.push(json_obj.clone()),
                                        }
                                    }
                                }
                                Value::Array(arr) => {
                                    for item in arr {
                                        if item.eq(&search_value) {
                                            match parent {
                                                Some(ref par) => response.push(par.clone()),
                                                None => response.push(json_obj.clone()),
                                            }
                                        }
                                    }
                                },
                                Value::Object(obj) => {
                                    if obj.contains_key(&search_value) {
                                        match parent {
                                            Some(ref par) => response.push(par.clone()),
                                            None => response.push(json_obj.clone()),
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    "startsWith" => {
                        if let Some(val) = obj[&search_key].as_str() {
                            if val.starts_with(&search_value) {
                                match parent {
                                    Some(ref par) => response.push(par.clone()),
                                    None => response.push(json_obj.clone()),
                                }
                            }
                        }
                    }
                    ">" => {
                        if let Some(val) = obj[&search_key].as_f64() {
                            let search_val = match search_value.parse::<f64>() {
                                Ok(val) => val,
                                _ => 0.0,
                            };

                            if val > search_val {
                                match parent {
                                    Some(ref par) => response.push(par.clone()),
                                    None => response.push(json_obj.clone()),
                                }
                            }
                        }
                    }
                    "<" => {
                        if let Some(val) = obj[&search_key].as_f64() {
                            let search_val = match search_value.parse::<f64>() {
                                Ok(val) => val,
                                _ => 0.0,
                            };

                            if val < search_val {
                                match parent {
                                    Some(ref par) => response.push(par.clone()),
                                    None => response.push(json_obj.clone()),
                                }
                            }
                        }
                    }
                    _ => {
                        println!("Condition not supported: {}", condition);
                    }
                }
            }

            for obj in json_obj.as_object().unwrap() {
                let (key, val) = obj;
                search_json(
                    val,
                    search_key.clone(),
                    search_value.clone(),
                    condition,
                    parent.clone(),
                    found,
                    response,
                );
            }
        }
        Value::Array(arr) => {
            for arr_item in arr {
                search_json(
                    arr_item,
                    search_key.clone(),
                    search_value.clone(),
                    condition,
                    parent.clone(),
                    found,
                    response,
                );
            }
        }
        _ => {},
    }
}

impl Filter {
    pub async fn prepare(&mut self, context: &WorkerInvocation) -> Result<()> {
        let task_name_map = context
            .worker
            .tasks
            .iter()
            .map(|(react_id, task)| (task.name.clone(), react_id.clone()))
            .collect::<HashMap<String, String>>();
        let variable_re = Regex::new(r"\{\{((?P<var_type>[^:\{\}]*):)?(?P<var_identifier>[^\[\.\{\}]+)\.?(?P<var_path>[^\}\{]*)\}\}").unwrap();
        let object_key = variable_re
            .replace(&self.object_to_filter, |groups: &regex::Captures| {
                let var_type = groups.name("var_type");
                let var_identifier = String::from(groups.name("var_identifier").unwrap().as_str());
                let var_path = String::from(groups.name("var_path").unwrap().as_str());

                // split the path into segments to be rearranged in a format that Tera can understand
                // e.g. [0].key1.key2[3] -> ["[0]", "key1", "key2", "[3]"] -> ["[0]", "['key1']", "['key2']", "[3]"] -> "[0]['key1']['key2'][3]"
                let segment_re = Regex::new(r"([^\[\.\}]+|\[\d+\])").unwrap();
                let tokens = segment_re
                    .captures_iter(&var_path)
                    .map(|capture| {
                        let segment = capture.get(1).unwrap().as_str();
                        if segment.starts_with("[") {
                            segment.to_string()
                        } else {
                            format!("['{}']", segment)
                        }
                    })
                    .collect::<Vec<String>>();

                match var_type {
                    Some(var_type) => match var_type.as_str() {
                        "OUTPUT" => {
                            let task_id = task_name_map.get(&var_identifier).unwrap();
                            format!(
                                "{{{{output.{}{} | json_encode() }}}}",
                                task_id,
                                tokens.join("")
                            )
                        }
                        "ASSET" => {
                            format!(
                                "{{{{asset.{}{} | json_encode() }}}}",
                                var_identifier,
                                tokens.join("")
                            )
                        }
                        "CUSTOM" => {
                            format!("{{{{custom.{} | json_encode() }}}}", var_identifier)
                        }
                        "GLOBAL" => {
                            format!(
                                "{{{{global['GLOBAL:{}'] | json_encode() }}}}",
                                var_identifier
                            )
                        }
                        _ => {
                            panic!("Invalid variable type: {}", var_type.as_str());
                        }
                    },
                    None => format!("{{{{{}}}}}", var_identifier),
                }
            })
            .to_string();

        let mut tera_context = tera::Context::new();
        tera_context.insert("output", &context.outputs.lock().unwrap().clone());
        let rendered = Tera::one_off(&object_key, &tera_context, false).unwrap();
        self.json_obj = Some(serde_json::from_str::<Value>(&rendered).unwrap());

        Ok(())
    }

    pub async fn execute(&self, context: &WorkerInvocation) -> Value {
        dbg!("Filtering object: ", &self.object_to_filter);
        let mut res: Vec<Value> = vec![];
        if let Some(json_obj) = &self.json_obj {
            search_json(
                &json_obj,
                self.search_key.clone(),
                self.search_value.clone(),
                &self.condition.clone(),
                None,
                false,
                &mut res
            );            
            
        };
        json!({
            "statusCode" : if res.len() > 0 { true } else { false },
            "response": {
                "results": res,
                "count": res.len()
            }
        })
    }
}
