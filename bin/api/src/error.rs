use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use kernel::AppError;
use serde::Serialize;

pub struct ApiError(pub AppError);

impl From<AppError> for ApiError {
    fn from(error: AppError) -> Self {
        Self(error)
    }
}

#[derive(Serialize)]
struct Problem {
    r#type: &'static str,
    title: &'static str,
    status: u16,
    detail: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, title, detail) = match self.0 {
            AppError::NotFound => (StatusCode::NOT_FOUND, "Not Found", "not found".to_string()),
            AppError::Validation(detail) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "Unprocessable Entity",
                detail,
            ),
            AppError::BadRequest(detail) => (StatusCode::BAD_REQUEST, "Bad Request", detail),
            AppError::Unauthorized(detail) => (StatusCode::UNAUTHORIZED, "Unauthorized", detail),
            AppError::Forbidden(detail) => (StatusCode::FORBIDDEN, "Forbidden", detail),
            AppError::RateLimited => (
                StatusCode::TOO_MANY_REQUESTS,
                "Too Many Requests",
                "too many requests, try again later".to_string(),
            ),
            AppError::Internal(detail) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal Server Error",
                detail,
            ),
        };

        let problem = Problem {
            r#type: "about:blank",
            title,
            status: status.as_u16(),
            detail,
        };

        let mut response = (status, Json(problem)).into_response();
        response.headers_mut().insert(
            axum::http::header::CONTENT_TYPE,
            axum::http::HeaderValue::from_static("application/problem+json"),
        );
        response
    }
}
