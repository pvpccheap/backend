use actix_web::{HttpResponse, ResponseError};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    #[error("Bad request: {0}")]
    BadRequest(String),

    #[error("External API error: {0}")]
    ExternalApi(String),

    #[error("Validation error: {field} - {message}")]
    Validation { field: String, message: String },

    #[error("Conflict: {0}")]
    Conflict(String),
}

impl AppError {
    /// Crea un error de validació per un camp específic
    pub fn validation(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Validation {
            field: field.into(),
            message: message.into(),
        }
    }

    /// Crea un error de recurs no trobat amb context
    pub fn not_found(resource: impl Into<String>, id: impl std::fmt::Display) -> Self {
        Self::NotFound(format!("{} with id '{}' not found", resource.into(), id))
    }
}

impl ResponseError for AppError {
    fn error_response(&self) -> HttpResponse {
        use actix_web::http::StatusCode;

        let (status, error_type, message) = match self {
            Self::Database(e) => {
                // Log the full error but return generic message to client
                tracing::error!(error = ?e, "Database error occurred");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "database_error",
                    "A database error occurred".to_string(),
                )
            }
            Self::NotFound(msg) => (StatusCode::NOT_FOUND, "not_found", msg.clone()),
            Self::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, "unauthorized", msg.clone()),
            Self::BadRequest(msg) => (StatusCode::BAD_REQUEST, "bad_request", msg.clone()),
            Self::ExternalApi(msg) => {
                tracing::warn!(message = %msg, "External API error");
                (StatusCode::BAD_GATEWAY, "external_api_error", msg.clone())
            }
            Self::Validation { field, message } => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "validation_error",
                format!("{}: {}", field, message),
            ),
            Self::Conflict(msg) => (StatusCode::CONFLICT, "conflict", msg.clone()),
        };

        HttpResponse::build(status).json(serde_json::json!({
            "error": {
                "type": error_type,
                "message": message
            }
        }))
    }
}

// Note: From<sqlx::Error> is derived automatically via #[from] attribute

impl From<jsonwebtoken::errors::Error> for AppError {
    fn from(e: jsonwebtoken::errors::Error) -> Self {
        tracing::warn!(error = %e, "JWT validation failed");
        Self::Unauthorized(format!("Invalid token: {}", e))
    }
}

impl From<reqwest::Error> for AppError {
    fn from(e: reqwest::Error) -> Self {
        tracing::warn!(error = %e, url = ?e.url(), "HTTP request failed");
        Self::ExternalApi(format!("External API error: {}", e))
    }
}

pub type AppResult<T> = Result<T, AppError>;
