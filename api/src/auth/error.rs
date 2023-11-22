use actix_web::{body::BoxBody, http::StatusCode, HttpResponse};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Authentication failure")]
    AuthenticationError,
    
    #[error("Unauthorized")]
    AuthorizationError,
}

impl actix_web::error::ResponseError for Error {
    fn error_response(&self) -> HttpResponse<BoxBody> {
        HttpResponse::build(self.status_code()).body(self.to_string())
    }

    fn status_code(&self) -> StatusCode {
        match self {
            Error::AuthenticationError => StatusCode::UNAUTHORIZED,
            Error::AuthorizationError => StatusCode::FORBIDDEN,
            _ => StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}