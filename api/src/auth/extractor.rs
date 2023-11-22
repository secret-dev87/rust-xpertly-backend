use super::middleware::{ XpertlyAuth };
use actix_web::{FromRequest, HttpMessage, HttpRequest};
use super::error::Error;
use xpertly_common::auth::{ Auth, BearerToken, Claims };

use std::{
    future::{ready, Ready},
};

#[derive(Debug)]
pub struct Authenticated(Auth);

impl FromRequest for Authenticated {
    type Error = Error;
    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, payload: &mut actix_web::dev::Payload) -> Self::Future {
        if let Some(error) = req.extensions().get::<Box<dyn std::error::Error>>() {
            dbg!(error);
            return ready(Err(Error::AuthenticationError))
        }

        let verified_claims = req.extensions().get::<XpertlyAuth>().cloned();
        let token = req.extensions().get::<BearerToken>().cloned();
        let result = match (verified_claims, token) {
            (Some(claims), Some(token)) => Ok(Authenticated(Auth { claims: claims.claims.clone(), token })),
            _ => Err(Error::AuthenticationError)
        };
        ready(result)
    }
}

impl std::ops::Deref for Authenticated {
    type Target = Auth;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}