use axum::{
    routing::{get, post},
    Router,
};
use sqlx::SqlitePool;

use crate::handlers::{
    batch_report_handler, get_alert_statistics_handler, get_alerts_handler,
    get_device_config_handler, get_hourly_handler, health_handler,
    report_temperature_handler, set_device_config_handler,
};

pub fn create_router(pool: SqlitePool) -> Router {
    Router::new()
        .route("/health", get(health_handler))
        .route("/api/temperature", post(report_temperature_handler))
        .route("/api/temperature/batch", post(batch_report_handler))
        .route("/api/temperature/hourly", get(get_hourly_handler))
        .route("/api/device/config", post(set_device_config_handler))
        .route("/api/device/config", get(get_device_config_handler))
        .route("/api/alerts", get(get_alerts_handler))
        .route("/api/alerts/statistics", get(get_alert_statistics_handler))
        .with_state(pool)
}
