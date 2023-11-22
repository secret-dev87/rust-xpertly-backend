use std::fmt::Display;

use anyhow::{Result, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use xpertly_common::*;

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Conditional {
    pub(crate) expression: Vec<ConditionGroup>,
}

impl Conditional {
    // pub fn execute(&self) -> Result<bool> {
    //     let expr_context = context_map! {
    //         "date" => Function::new(|argument| {
    //             println!("{}", argument);
    //             if let Ok(date) = argument.as_string().unwrap().parse::<DateTime<Utc>>() {
    //                 return Ok(Value::Int(date.timestamp()));
    //             } else {
    //                 Err(EvalexprError::expected_string(argument.clone()))
    //             }
    //         })
    //     }.unwrap();
        
    //     let expr = self.build_expression_str()?;
    //     dbg!(&expr);
    //     match eval_boolean_with_context(&expr, &expr_context) {
    //         Ok(result) => {
    //             println!("{}", result);
    //             return Ok(result);
    //         }
    //         Err(err) => {
    //             dbg!(err);
    //             return Ok(false);
    //         }
    //     }
    // }
    
    pub fn execute(&self) -> Result<serde_json::Value> {
        let result = self.eval()?;
        let expression_str = self.build_expression_str()?;

        Ok(json!({
            "statusCode": result,
            "response": {"expression": expression_str,}
        }))
    }

    pub fn eval(&self) -> Result<bool> {
        // **NOT short-circuiting**
        // not all arms of the expression need to be evaluated,
        // e.g. (a AND b AND c) if a is false, b and c do not need to be evaluated
        // similarly for (a OR b OR c) if a is true, b and c do not need to be evaluated
        // this applies between conditions and groups of conditions
        // to skip unnecessary evaluation, we'd need a tree structure to represent the expression
        // which are difficult to implement in Rust (technically not impossible, but prohibitively difficult)
        // for now, we'll just evaluate the expression as a whole

        let mut result = true;
        for group in self.expression.iter() {
            result = match group.op {
                Some(Operator::And) => result && group.eval()?,
                Some(Operator::Or) => result || group.eval()?,
                None => group.eval()?,
            };
        }
        return Ok(result);
    }

    pub fn build_expression_str(&self) -> Result<String> {
        let mut expression_str = String::new();
        for group in &self.expression {
            if let Some(operator) = &group.op {
                expression_str.push_str(&format!(" {} ", operator.as_ref()));
            }
            expression_str.push_str("(");
            for condition in &group.conditions {
                expression_str.push_str(
                    &format!(
                        "{} {} {}", 
                        self.parse_operand(&condition.var1)?, 
                        condition.comparitor.to_string(), 
                        self.parse_operand(&condition.var2)?
                    )
                );
                if let Some(operator) = &condition.op {
                    expression_str.push_str(&format!(" {} ", operator.as_ref()));
                }
            }
            expression_str.push_str(")");
        }

        Ok(expression_str)
    }

    fn parse_operator(&self, op: &str) -> Result<&str> {
        match op {
            "AND" => Ok("&&"),
            "and" => Ok("&&"),
            "&&" => Ok("&&"),
            "OR" => Ok("||"),
            "or" => Ok("||"),
            "||" => Ok("||"),
            _ => bail!("Invalid operator: {}", op)
        }
    }

    fn parse_operand(&self, op: &str) -> Result<Box<dyn Display>>
    {
        if let Ok(date) = op.parse::<DateTime<Utc>>() {
            Ok(Box::new(date.timestamp()))
        } else if let Ok(float) = op.parse::<f64>() {
            return Ok(Box::new(float));
        } else if let Ok(int) = op.parse::<i64>() {
            return Ok(Box::new(int));
        } else if let Ok(bool) = op.parse::<bool>() {
            return Ok(Box::new(bool));
        } else {
            return Ok(Box::new(format!("\"{}\"", op)));
        }
    }
}

// #[derive(Serialize, Deserialize, Debug, Clone)]
// pub struct ConditionGroup {
//     op: Option<Operator>,
//     conditions: Vec<Condition>,
// }

// impl ConditionGroup {
//     fn eval(&self) -> Result<bool> {
//         let mut result = true;
//         for condition in self.conditions.iter() {
//             result = match condition.op {
//                 Some(Operator::And) => result && condition.eval()?,
//                 Some(Operator::Or) => result || condition.eval()?,
//                 None => condition.eval()?,
//             }
//         }
//         Ok(result)
//     }
// }

// #[derive(Serialize, Deserialize, Debug, Clone)]
// #[serde(rename_all = "camelCase")]
// struct Condition {
//     #[serde(with = "serde_with::rust::string_empty_as_none")]
//     op: Option<Operator>,
//     comparitor: Comparitor,
//     var1: String,
//     var2: String
// }

// impl Condition {
//     fn eval(&self) -> Result<bool> {
//         if let Ok(var1) = self.parse_var(&self.var1) {
//             if let Ok(var2) = self.parse_var(&self.var2) {
//                 // check if enum variants are the same, operations should only be carried
//                 // out on variables of the same type
//                 if std::mem::discriminant(&var1) == std::mem::discriminant(&var2) {
//                     return match self.comparitor {
//                         Comparitor::Equal => Ok(var1 == var2),
//                         Comparitor::NotEqual => Ok(var1 != var2),
//                         Comparitor::GreaterThan => Ok(var1 > var2),
//                         Comparitor::LessThan => Ok(var1 < var2),
//                     }
//                 } else {
//                     bail!("Cannot compare variables of different types")
//                 }
//             } else {
//                 bail!("Could not parse second variable")
//             }
//         } else {
//             bail!("Could not parse first variable")
//         }
//     }

//     fn parse_var(&self, var: &str) -> Result<Var> {
//         if let Ok(date) = var.parse::<DateTime<Utc>>() {
//             Ok(Var::Date(date))
//         } else if let Ok(float) = var.parse::<f64>() {
//             return Ok(Var::Number(float));
//         } else if let Ok(bool) = var.parse::<bool>() {
//             return Ok(Var::Boolean(bool));
//         } else {
//             return Ok(Var::String(var.to_string()));
//         }
//     }
// }

// #[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
// #[serde(rename_all = "camelCase")]
// enum Operator {
//     #[serde(rename = "AND")]
//     And,
//     #[serde(rename = "OR")]
//     Or
// }

// impl FromStr for Operator {
//     type Err = anyhow::Error;
//     fn from_str(s: &str) -> Result<Self, Self::Err> {
//         match s {
//             "AND" => Ok(Operator::And),
//             "OR" => Ok(Operator::Or),
//             _ => bail!("Invalid operator: {}", s)
//         }
//     }
// }

// impl AsRef<str> for Operator {
//     fn as_ref(&self) -> &str {
//         match self {
//             Operator::And => "AND",
//             Operator::Or => "OR"
//         }
//     }
// }

// #[derive(Serialize, Deserialize, Debug, Clone)]
// enum Comparitor {
//     #[serde(rename = "==")]
//     Equal,
//     #[serde(rename = "!=")]
//     NotEqual,
//     #[serde(rename = ">")]
//     GreaterThan,
//     #[serde(rename = ">=")]
//     LessThan,
// }

// impl Comparitor {
//     fn to_string(&self) -> String {
//         match self {
//             Comparitor::Equal => "==",
//             Comparitor::NotEqual => "!=",
//             Comparitor::GreaterThan => ">",
//             Comparitor::LessThan => "<",
//         }.to_string()
//     }
// }

// #[derive(Serialize, Deserialize, Debug, PartialEq, PartialOrd)]
// enum Var {
//     String(String),
//     Number(f64),
//     Date(DateTime<Utc>),
//     Boolean(bool),
// }

// #[derive(Serialize, Deserialize, Debug)]
// #[serde(rename_all = "camelCase")]
// pub struct Conditional {
//     pub react_id: String,
//     pub category: Option<String>,
//     pub description: Option<String>,
//     fields: ConditionalFields,
//     #[serde(skip_deserializing, skip_serializing)]
//     #[serde(default = "Uuid::new_v4")]
//     pub run_id: Uuid,
//     pub next: Next
// }



// mod tests {
//     use super::*;

//     #[test]
//     fn test_conditional() {
//         let conditional = Conditional {
//             react_id: "test".to_string(),
//             category: None,
//             description: None,
//             fields: ConditionalFields {
//                 expression: vec![
//                     ConditionGroup {
//                         conditions: vec![
//                             Condition {
//                                 comparitor: "
//                             }
//                         ]
//                     }
//                 ]
//             }
//         }
//     }
// }