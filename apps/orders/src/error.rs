/// Application-level errors returned by HTTP handlers.
///
/// Each variant maps to an appropriate HTTP status code so that invalid
/// input and application failures produce structured error responses instead
/// of panics.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("internal error: {0}")]
    Internal(String),
}

impl AppError {
    /// HTTP status code associated with this error.
    pub fn status_code(&self) -> axum::http::StatusCode {
        match self {
            AppError::InvalidRequest(_) => axum::http::StatusCode::BAD_REQUEST,
            AppError::NotFound(_) => axum::http::StatusCode::NOT_FOUND,
            AppError::Internal(_) => axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl axum::response::IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        let status = self.status_code();
        let body = serde_json::json!({
            "error": self.to_string(),
            "status": status.as_u16(),
        });
        (status, axum::Json(body)).into_response()
    }
}
