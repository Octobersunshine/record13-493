use axum::{
    routing::{get, post},
    Router,
};
use sqlx::SqlitePool;

use crate::handlers::{batch_report_handler, get_hourly_handler, health_handler, report_temperature_handler};

pub fn create_router(pool: SqlitePool) -> Router {
    Router::new()
        .route("/health", get(health_handler))
        .route("/api/temperature", post(report_temperature_handler))
        .route("/api/temperature/batch", post(batch_report_handler))
        .route("/api/temperature/hourly", get(get_hourly_handler))
        .with_state(pool)
}
