use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    BadRequest(String),
    #[error("{0}")]
    Conflict(String),
    #[error("PLC unavailable: {0}")]
    PlcUnavailable(String),
    #[error("layout mismatch on {plc}.{db}: {detail}")]
    LayoutMismatch { plc: String, db: String, detail: String },
    #[error("OPC UA not ready: {0}")]
    OpcNotReady(String),
    #[error("{0}")]
    Internal(String),
}

impl ApiError {
    pub fn code(&self) -> &'static str {
        match self {
            ApiError::NotFound(_) => "not_found",
            ApiError::BadRequest(_) => "bad_request",
            ApiError::Conflict(_) => "conflict",
            ApiError::PlcUnavailable(_) => "plc_unavailable",
            ApiError::LayoutMismatch { .. } => "layout_mismatch",
            ApiError::OpcNotReady(_) => "opc_not_ready",
            ApiError::Internal(_) => "internal",
        }
    }
    fn status(&self) -> StatusCode {
        match self {
            ApiError::NotFound(_) => StatusCode::NOT_FOUND,
            ApiError::BadRequest(_) => StatusCode::BAD_REQUEST,
            ApiError::Conflict(_) => StatusCode::CONFLICT,
            ApiError::PlcUnavailable(_) | ApiError::LayoutMismatch { .. } | ApiError::OpcNotReady(_) => StatusCode::SERVICE_UNAVAILABLE,
            ApiError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = json!({ "error": self.to_string(), "code": self.code() });
        (self.status(), axum::Json(body)).into_response()
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        ApiError::Internal(format!("{e:#}"))
    }
}
impl From<rusqlite::Error> for ApiError {
    fn from(e: rusqlite::Error) -> Self {
        ApiError::Internal(format!("sqlite: {e}"))
    }
}
impl From<serde_json::Error> for ApiError {
    fn from(e: serde_json::Error) -> Self {
        ApiError::BadRequest(format!("json: {e}"))
    }
}
impl From<plc_layout::LayoutError> for ApiError {
    fn from(e: plc_layout::LayoutError) -> Self {
        ApiError::Internal(format!("layout: {e}"))
    }
}

pub type ApiResult<T> = Result<axum::Json<T>, ApiError>;
