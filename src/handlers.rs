use axum::{
    extract::{Query, State},
    Json,
    http::StatusCode,
};
use chrono::DateTime;
use serde::Deserialize;
use sqlx::SqlitePool;
use tracing::{info, warn};

use crate::models::{
    AlertStatistics, ApiResponse, DeviceTempConfig, DeviceTempConfigRequest, HourlyTemperature,
    TemperatureAlert, TemperatureRecord, TemperatureReport,
};
use crate::storage::{
    batch_report_temperatures, calculate_alert_statistics, get_alert_list, get_device_temp_config,
    get_hourly_data, report_temperature, set_device_temp_config,
};

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
    if let Some(ct) = payload.client_timestamp {
        info!(
            "收到温度上报 - 设备: {}, 温度: {}°C, 客户端时间: {} (服务器时间为准)",
            payload.device_id, payload.temperature, ct
        );
    } else {
        info!(
            "收到温度上报 - 设备: {}, 温度: {}°C (服务器时间为准)",
            payload.device_id, payload.temperature
        );
    }

    match report_temperature(&pool, &payload).await {
        Ok(record) => {
            if let Some(ct) = record.client_timestamp {
                let diff = (record.timestamp - ct).num_seconds();
                if diff.abs() > 300 {
                    warn!(
                        "设备 {} 客户端时间与服务器时间偏差较大: {} 秒",
                        record.device_id, diff
                    );
                }
            }
            (
                StatusCode::OK,
                Json(ApiResponse::success(record)),
            )
        }
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

#[derive(Debug, Deserialize)]
pub struct AlertQuery {
    pub device_id: Option<String>,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub resolved: Option<String>,
}

pub async fn set_device_config_handler(
    State(pool): State<SqlitePool>,
    Json(payload): Json<DeviceTempConfigRequest>,
) -> (StatusCode, Json<ApiResponse<DeviceTempConfig>>) {
    info!(
        "设置设备温度配置 - 设备: {}, 温度范围: {}°C ~ {}°C",
        payload.device_id, payload.min_temp, payload.max_temp
    );

    if payload.min_temp >= payload.max_temp {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::error("最低温度必须低于最高温度")),
        );
    }

    match set_device_temp_config(&pool, &payload).await {
        Ok(config) => (
            StatusCode::OK,
            Json(ApiResponse::success(config)),
        ),
        Err(e) => {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error(&format!("配置失败: {}", e))),
            )
        }
    }
}

pub async fn get_device_config_handler(
    State(pool): State<SqlitePool>,
    Query(params): Query<AlertQuery>,
) -> (StatusCode, Json<ApiResponse<DeviceTempConfig>>) {
    let device_id = match params.device_id {
        Some(ref id) => id.clone(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiResponse::error("缺少 device_id 参数")),
            );
        }
    };

    match get_device_temp_config(&pool, &device_id).await {
        Ok(Some(config)) => (
            StatusCode::OK,
            Json(ApiResponse::success(config)),
        ),
        Ok(None) => {
            (
                StatusCode::NOT_FOUND,
                Json(ApiResponse::error("设备配置不存在")),
            )
        }
        Err(e) => {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error(&format!("查询失败: {}", e))),
            )
        }
    }
}

pub async fn get_alerts_handler(
    State(pool): State<SqlitePool>,
    Query(params): Query<AlertQuery>,
) -> (StatusCode, Json<ApiResponse<Vec<TemperatureAlert>>>) {
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

    let resolved = params.resolved.as_deref().and_then(|s| match s {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    });

    match get_alert_list(&pool, device_id, start_time, end_time, resolved).await {
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

pub async fn get_alert_statistics_handler(
    State(pool): State<SqlitePool>,
    Query(params): Query<AlertQuery>,
) -> (StatusCode, Json<ApiResponse<AlertStatistics>>) {
    let device_id = match params.device_id {
        Some(ref id) => id.clone(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiResponse::error("缺少 device_id 参数")),
            );
        }
    };

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

    match calculate_alert_statistics(&pool, &device_id, start_time, end_time).await {
        Ok(stats) => (
            StatusCode::OK,
            Json(ApiResponse::success(stats)),
        ),
        Err(e) => {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error(&format!("统计失败: {}", e))),
            )
        }
    }
}
