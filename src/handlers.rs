use axum::{
    extract::{Query, State},
    Json,
    http::StatusCode,
};
use chrono::DateTime;
use serde::Deserialize;
use sqlx::SqlitePool;
use tracing::info;

use crate::models::{ApiResponse, HourlyTemperature, TemperatureRecord, TemperatureReport};
use crate::storage::{batch_report_temperatures, get_hourly_data, report_temperature};

#[derive(Debug, Deserialize)]
pub struct HourlyQuery {
    pub device_id: Option<String>,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
}

pub async fn report_temperature_handler(
    State(pool): State<SqlitePool>,
    Json(payload): Json<TemperatureReport>,
) -> (StatusCode, Json<ApiResponse<TemperatureRecord>>) {
    info!(
        "收到温度上报 - 设备: {}, 温度: {}°C",
        payload.device_id, payload.temperature
    );

    match report_temperature(&pool, &payload).await {
        Ok(record) => (
            StatusCode::OK,
            Json(ApiResponse::success(record)),
        ),
        Err(e) => {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error(&format!("上报失败: {}", e))),
            )
        }
    }
}

pub async fn batch_report_handler(
    State(pool): State<SqlitePool>,
    Json(payload): Json<Vec<TemperatureReport>>,
) -> (StatusCode, Json<ApiResponse<Vec<TemperatureRecord>>>) {
    info!("批量温度上报 - 数量: {}", payload.len());

    match batch_report_temperatures(&pool, &payload).await {
        Ok(records) => (
            StatusCode::OK,
            Json(ApiResponse::success(records)),
        ),
        Err(e) => {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error(&format!("批量上报失败: {}", e))),
            )
        }
    }
}

pub async fn get_hourly_handler(
    State(pool): State<SqlitePool>,
    Query(params): Query<HourlyQuery>,
) -> (StatusCode, Json<ApiResponse<Vec<HourlyTemperature>>>) {
    let device_id = params.device_id.as_deref();

    let start_time = match params.start_time {
        Some(ref s) => match DateTime::parse_from_rfc3339(s) {
            Ok(dt) => Some(dt.with_timezone(&chrono::Utc)),
            Err(_) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(ApiResponse::error("start_time 格式错误，请使用 RFC3339 格式")),
                );
            }
        },
        None => None,
    };

    let end_time = match params.end_time {
        Some(ref s) => match DateTime::parse_from_rfc3339(s) {
            Ok(dt) => Some(dt.with_timezone(&chrono::Utc)),
            Err(_) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(ApiResponse::error("end_time 格式错误，请使用 RFC3339 格式")),
                );
            }
        },
        None => None,
    };

    match get_hourly_data(&pool, device_id, start_time, end_time).await {
        Ok(data) => (
            StatusCode::OK,
            Json(ApiResponse::success(data)),
        ),
        Err(e) => {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error(&format!("查询失败: {}", e))),
            )
        }
    }
}

pub async fn health_handler() -> (StatusCode, Json<ApiResponse<String>>) {
    (
        StatusCode::OK,
        Json(ApiResponse::success("服务运行正常".to_string())),
    )
}
