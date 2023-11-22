use super::asset::Assets;
use anyhow::{bail, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_with;
use std::collections::HashMap;
use std::str::FromStr;
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WorkerConfig {
    pub name: String,
    pub id: Uuid,
    pub tenant_id: Uuid,
    #[serde(rename = "type")]
    pub category: Option<String>,
    pub available_in_avicenna: bool,
    pub schedule: Option<Schedule>,
    pub description: String,
    pub tasks: Vec<TaskConfig>,
    pub global: Option<Value>,
    pub custom: Option<Value>,
    pub schema_id: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ScheduledExecution {
    schedule: Schedule,
    run_parameters: WorkerConfig,
    date_last_modified: String,
    date_last_run: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Schedule {
    datetime: String,
    timezone: String,
    repeat: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TaskConfig {
    pub name: Option<String>,
    pub vendor: Option<String>,
    #[serde(rename = "type")]
    pub category: Option<String>,
    pub task_id: Option<Uuid>,
    pub react_id: String,
    pub description: Option<String>,
    pub x_pos: i64,
    pub y_pos: i64,
    pub needs_to_wait: bool,
    pub fields: TaskFields,
    pub next: Option<Next>,
    pub assets: Assets,
    pub path_params_pair: Option<Vec<HashMap<String, String>>>,
    pub query_params_pair: Option<Vec<HashMap<String, String>>>,
    #[serde(with = "serde_with::rust::string_empty_as_none")]
    #[serde(skip_serializing)]
    pub integration_id: Option<Uuid>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
#[serde(untagged)]
pub enum TaskFields {
    Endpoint(EndpointFields),
    Loop(LoopFields),
    Conditional(ConditionalFields),
    Filter(FilterFields),
}

/**
 * Endpoint tasks
 */
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct EndpointFields {
    pub method: String,
    pub headers: Option<Vec<Header>>,
    pub path_params: Option<HashMap<String, String>>,
    pub query_params: Option<HashMap<String, HashMap<String, String>>>,
    pub body: Option<Value>,
    pub target_url: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Header {
    pub key: String,
    pub value: String,
}

impl Header {
    pub fn as_tuple(&self) -> (String, String) {
        (String::from(&self.key), String::from(&self.value))
    }
}

/**
 * Loop tasks
 */

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LoopFields {
    pub tasks: Vec<TaskConfig>,
}

/**
 * Conditional tasks
 */
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ConditionalFields {
    pub expression: Vec<ConditionGroup>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ConditionGroup {
    #[serde(with = "serde_with::rust::string_empty_as_none")]
    pub op: Option<Operator>,
    pub conditions: Vec<Condition>,
}

impl ConditionGroup {
    pub fn eval(&self) -> Result<bool> {
        let mut result = true;
        let mut operator: Option<Operator> = None;
        for condition in self.conditions.iter() {
            result = match operator {
                Some(Operator::And) => result && condition.eval()?,
                Some(Operator::Or) => result || condition.eval()?,
                None => condition.eval()?,
            };

            // keeping track of the previous operator, as operators are tied to the condition that precedes them in the JSON representation.
            // Each condition actually needs to use the previous condition's operator to apply its result to the overall result.
            operator = condition.op.clone();
        }
        Ok(result)
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Condition {
    #[serde(with = "serde_with::rust::string_empty_as_none")]
    pub op: Option<Operator>,
    pub comparitor: Comparitor,
    pub var1: String,
    pub var2: String,
}

impl Condition {
    pub fn eval(&self) -> Result<bool> {
        return match self.comparitor {
            Comparitor::Equal => {
                let (parsed_var1, parsed_var2) = self.parse_to_comparable(&self.var1, &self.var2)?;
                Ok(parsed_var1 == parsed_var2)
            },
            Comparitor::NotEqual => {
                let (parsed_var1, parsed_var2) = self.parse_to_comparable(&self.var1, &self.var2)?;
                Ok(parsed_var1 != parsed_var2)
            },
            Comparitor::GreaterThan => {
                let (parsed_var1, parsed_var2) = self.parse_to_comparable(&self.var1, &self.var2)?;
                Ok(parsed_var1 > parsed_var2)
            },
            Comparitor::GreaterThanOrEqual => {
                let (parsed_var1, parsed_var2) = self.parse_to_comparable(&self.var1, &self.var2)?;
                Ok(parsed_var1 >= parsed_var2)
            },
            Comparitor::LessThan => {
                let (parsed_var1, parsed_var2) = self.parse_to_comparable(&self.var1, &self.var2)?;
                Ok(parsed_var1 < parsed_var2)
            },
            Comparitor::LessThanOrEqual => {
                let (parsed_var1, parsed_var2) = self.parse_to_comparable(&self.var1, &self.var2)?;
                Ok(parsed_var1 <= parsed_var2)
            },
            // String comparitors
            // skip parsing as variable, as we want to compare the string literal
            Comparitor::Contains => Ok(self.var1.contains(&self.var2)),
            Comparitor::NotContains => Ok(!self.var1.contains(&self.var2)),
            Comparitor::BeginsWith => Ok(self.var1.starts_with(&self.var2)),
            Comparitor::NotBeginsWith => Ok(!self.var1.starts_with(&self.var2)),
            Comparitor::EndsWith => Ok(self.var1.ends_with(&self.var2)),
            Comparitor::NotEndsWith => Ok(!self.var1.ends_with(&self.var2)),
        };
    }

    fn parse_var(&self, var: &str) -> Var {
        if let Ok(date) = var.parse::<DateTime<Utc>>() {
            Var::Date(date)
        } else if let Ok(float) = var.parse::<f64>() {
            return Var::Number(float);
        } else if let Ok(bool) = var.parse::<bool>() {
            return Var::Boolean(bool);
        } else {
            return Var::String(var.to_string());
        }
    }

    fn parse_to_comparable(&self, var1: &str, var2: &str) -> Result<(Var, Var)> {
        let var1 = self.parse_var(&var1);
        let var2 = self.parse_var(&var2);
        if std::mem::discriminant(&var1) != std::mem::discriminant(&var2) {
            bail!("Cannot compare variables of different types")
        }
        return Ok((var1, var2));
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(rename_all = "camelCase")]
pub enum Operator {
    #[serde(rename = "AND")]
    And,
    #[serde(rename = "OR")]
    Or,
}

impl AsRef<str> for Operator {
    fn as_ref(&self) -> &str {
        match self {
            Operator::And => "AND",
            Operator::Or => "OR",
        }
    }
}

impl FromStr for Operator {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "AND" => Ok(Operator::And),
            "OR" => Ok(Operator::Or),
            _ => bail!("Invalid operator: {}", s),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Comparitor {
    #[serde(rename = "==")]
    Equal,
    #[serde(rename = "!=")]
    NotEqual,
    #[serde(rename = ">")]
    GreaterThan,
    #[serde(rename = ">=")]
    GreaterThanOrEqual,
    #[serde(rename = "<")]
    LessThan,
    #[serde(rename = "<=")]
    LessThanOrEqual,
    #[serde(rename = "contains")]
    Contains,
    #[serde(rename = "!contains")]
    NotContains,
    #[serde(rename = "begins_with")]
    BeginsWith,
    #[serde(rename = "!begins_with")]
    NotBeginsWith,
    #[serde(rename = "ends_with")]
    EndsWith,
    #[serde(rename = "!ends_with")]
    NotEndsWith,
}

impl Comparitor {
    pub fn to_string(&self) -> String {
        match self {
            Comparitor::Equal => "==",
            Comparitor::NotEqual => "!=",
            Comparitor::GreaterThan => ">",
            Comparitor::GreaterThanOrEqual => ">=",
            Comparitor::LessThan => "<",
            Comparitor::LessThanOrEqual => "<=",
            Comparitor::Contains => "contains",
            Comparitor::NotContains => "!contains",
            Comparitor::BeginsWith => "begins_with",
            Comparitor::NotBeginsWith => "!begins_with",
            Comparitor::EndsWith => "ends_with",
            Comparitor::NotEndsWith => "!ends_with",
        }
        .to_string()
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, PartialOrd)]
pub enum Var {
    String(String),
    Number(f64),
    Date(DateTime<Utc>),
    Boolean(bool),
}


/**
 * Filter tasks
 */
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FilterFields {
    pub condition: String,
    pub object_to_filter: String,
    pub search_key: String,
    pub search_value: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Next {
    #[serde(rename = "true")]
    pub true_branch: Option<String>,
    #[serde(rename = "false")]
    pub false_branch: Option<String>,
}

// #[derive(Serialize, Deserialize, Debug, Clone)]
// #[serde(rename_all = "camelCase")]
// pub struct Worker {
//     name: String,
//     id: Uuid,
//     #[serde(rename = "type")]
//     category: String,
//     available_in_avicenna: bool,
//     description: String,
//     tenant_id: Uuid,
//     tasks: HashMap<String, Task>,
//     #[serde(skip)]
//     #[serde(default)]
//     start: String
// }
