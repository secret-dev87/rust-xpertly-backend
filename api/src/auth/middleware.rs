use actix_service::Transform;
use actix_web::{
    dev::{Service, ServiceRequest, ServiceResponse}, 
    Error, HttpMessage, ResponseError
};
use serde::{Serialize, Deserialize};
use uuid::Uuid;
use xpertly_common::{BearerToken, Claims};
use std::{rc::Rc, ops::Deref, fmt::Display};
use futures::{
    future::{ready, LocalBoxFuture, Ready},
    FutureExt
};
use std::str::FromStr;
use super::error::Error as AuthorizationError;
use jsonwebtoken::{decode, decode_header, DecodingKey, Validation, Algorithm, TokenData};

pub type XpertlyAuth = Rc<TokenData<Claims>>;

#[derive(Serialize, Deserialize, Clone)]
pub struct XpertlyJwk {
    alg: String,
    e: String,
    kid: String,
    kty: String,
    n: String,
    #[serde(rename="use")]
    u: String,
}

pub struct AuthenticateMiddlewareFactory {
    key_store: Rc<Vec<XpertlyJwk>>
}

impl AuthenticateMiddlewareFactory {
    pub fn new(key_store: Vec<XpertlyJwk>) -> Self {
        AuthenticateMiddlewareFactory { key_store: Rc::new(key_store) }
    }
}

impl<S, B> Transform<S, ServiceRequest> for AuthenticateMiddlewareFactory
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Transform = AuthenticateMiddleware<S>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(AuthenticateMiddleware {
            key_store: self.key_store.clone(),
            service: Rc::new(service)
        }))
    }
}

pub struct AuthenticateMiddleware<S> {
    key_store: Rc<Vec<XpertlyJwk>>,
    service: Rc<S>
}

impl <S, B> Service<ServiceRequest> for AuthenticateMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;
    actix_service::forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let srv = Rc::clone(&self.service);
        let key_store = self.key_store.clone();

        async move {
            // if token exists
            if let Some(auth) = req.headers().get("Authorization") {
                dbg!("header present");
                // if token is a valid str
                if let Ok(token) = auth.to_str() {
                    dbg!("valid str");
                    let token = BearerToken::from_str(token).map_err(|_| AuthorizationError::AuthenticationError)?;
                    if let Ok(header) = decode_header(&token) {
                        dbg!("valid header");
                        // look for matching key in the jwks
                        key_store.iter().for_each(|k| {
                            if &k.kid == header.kid.as_ref().unwrap() {
                                dbg!(&k.kid);
                                // if token can be successfully decoded
                                match decode::<Claims>(token.as_str(), 
                                    &DecodingKey::from_rsa_components(&k.n, &k.e).unwrap(), 
                                    &Validation::new(Algorithm::RS256)) {
                                    Ok(claims) => {
                                        dbg!(&claims);
                                        req.extensions_mut().insert::<XpertlyAuth>(Rc::new(claims));
                                        req.extensions_mut().insert::<BearerToken>(token.clone());
                                    },
                                    Err(e) => {
                                        req.extensions_mut().insert::<Box<dyn std::error::Error>>(e.into());
                                    }
                                }
                            }
                        })
                    }
                }
            }

            let res = srv.call(req).await?;
            Ok(res)
        }
        .boxed_local()
    }
}

