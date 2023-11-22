use serde::{Deserialize, Serialize};
use std::{fmt::Display, ops::Deref, str::FromStr};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Auth {
    pub token: BearerToken,
    pub claims: Claims,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Claims {
    sub: Uuid,
    event_id: Uuid,
    token_use: String,
    scope: String,
    auth_time: usize,
    iss: String,
    exp: usize,
    iat: usize,
    jti: Uuid,
    client_id: String,
    pub username: Uuid,
}
#[derive(Debug, Clone)]
pub struct TokenError;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BearerToken(String);

impl FromStr for BearerToken {
    type Err = TokenError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(BearerToken(
            String::from(s).replace("Bearer", "").trim().to_string(),
        ))
    }
}

impl Deref for BearerToken {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Display for BearerToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
