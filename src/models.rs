use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Serialize, Deserialize)]
pub struct TemperatureReport {
    pub device_id: String,
    pub temperature: f64,
    pub humidity: Option<f64>,
    pub timestamp: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct TemperatureRecord {
    pub id: String,
    pub device_id: String,
    pub temperature: f64,
    pub humidity: Option<f64>,
    pub timestamp: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct HourlyTemperature {
    pub id: String,
    pub device_id: String,
    pub hour_start: DateTime<Utc>,
    pub avg_temp: f64,
    pub min_temp: f64,
    pub max_temp: f64,
    pub sample_count: i64,
    pub avg_humidity: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HourlyQueryParams {
    pub device_id: Option<String>,
    pub start_time: Option<DateTime<Utc>>,
    pub end_time: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub message: String,
    pub data: Option<T>,
}

impl<T> ApiResponse<T> {
    pub fn success(data: T) -> Self {
        Self {
            success: true,
            message: "操作成功".to_string(),
            data: Some(data),
        }
    }

    pub fn success_msg(message: &str) -> Self {
        Self {
            success: true,
            message: message.to_string(),
            data: None,
        }
    }

    pub fn error(message: &str) -> Self {
        Self {
            success: false,
            message: message.to_string(),
            data: None,
        }
    }
}
