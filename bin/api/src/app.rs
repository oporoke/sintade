use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use sqlx::PgPool;

pub fn build_router(pool: PgPool) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .with_state(pool)
}

async fn healthz() -> StatusCode {
    StatusCode::OK
}

async fn readyz(State(pool): State<PgPool>) -> StatusCode {
    match sqlx::query!("SELECT 1 as one").fetch_one(&pool).await {
        Ok(_) => StatusCode::OK,
        Err(error) => {
            tracing::error!(%error, "readyz db check failed");
            StatusCode::SERVICE_UNAVAILABLE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    #[sqlx::test]
    async fn healthz_returns_200(pool: PgPool) {
        let app = build_router(pool);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/healthz")
                    .body(Body::empty())
                    .expect("valid request"),
            )
            .await
            .expect("router call succeeds");
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[sqlx::test]
    async fn readyz_returns_200_with_real_db_check(pool: PgPool) {
        let app = build_router(pool);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/readyz")
                    .body(Body::empty())
                    .expect("valid request"),
            )
            .await
            .expect("router call succeeds");
        assert_eq!(response.status(), StatusCode::OK);
    }
}
