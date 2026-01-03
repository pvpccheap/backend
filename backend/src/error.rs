use actix_web::{HttpResponse, ResponseError};
use std::fmt;

#[derive(Debug)]
pub enum AppError {
    Database(sqlx::Error),
    NotFound(String),
    Unauthorized(String),
    BadRequest(String),
    Internal(String),
    ExternalApi(String),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(e) => write!(f, "Database error: {}", e),
            Self::NotFound(msg) => write!(f, "Not found: {}", msg),
            Self::Unauthorized(msg) => write!(f, "Unauthorized: {}", msg),
            Self::BadRequest(msg) => write!(f, "Bad request: {}", msg),
            Self::Internal(msg) => write!(f, "Internal error: {}", msg),
            Self::ExternalApi(msg) => write!(f, "External API error: {}", msg),
        }
    }
}

impl ResponseError for AppError {
    fn error_response(&self) -> HttpResponse {
        let (status, message) = match self {
            Self::Database(_) => (
                actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Database error".to_string(),
            ),
            Self::NotFound(msg) => (actix_web::http::StatusCode::NOT_FOUND, msg.clone()),
            Self::Unauthorized(msg) => (actix_web::http::StatusCode::UNAUTHORIZED, msg.clone()),
            Self::BadRequest(msg) => (actix_web::http::StatusCode::BAD_REQUEST, msg.clone()),
            Self::Internal(msg) => (
                actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
                msg.clone(),
            ),
            Self::ExternalApi(msg) => (actix_web::http::StatusCode::BAD_GATEWAY, msg.clone()),
        };

        HttpResponse::build(status).json(serde_json::json!({
            "error": message
        }))
    }
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        tracing::error!("Database error: {:?}", e);
        Self::Database(e)
    }
}

impl From<jsonwebtoken::errors::Error> for AppError {
    fn from(e: jsonwebtoken::errors::Error) -> Self {
        tracing::error!("JWT error: {:?}", e);
        Self::Unauthorized(format!("Invalid token: {}", e))
    }
}

impl From<reqwest::Error> for AppError {
    fn from(e: reqwest::Error) -> Self {
        tracing::error!("HTTP client error: {:?}", e);
        Self::ExternalApi(format!("External API error: {}", e))
    }
}

pub type AppResult<T> = Result<T, AppError>;
