use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use sqlx::{SqlitePool, sqlite::SqlitePoolOptions};
use uuid::Uuid;

use crate::models::{DeviceTempConfig, DeviceTempConfigRequest, HourlyTemperature, TemperatureAlert, TemperatureRecord, TemperatureReport};

pub async fn init_db(database_url: &str) -> Result<SqlitePool> {
    let pool = SqlitePoolOptions::new()
        .max_connections(10)
        .connect(database_url)
        .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS temperature_records (
            id TEXT PRIMARY KEY,
            device_id TEXT NOT NULL,
            temperature REAL NOT NULL,
            humidity REAL,
            timestamp DATETIME NOT NULL,
            client_timestamp DATETIME,
            created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_temp_records_device_time 
        ON temperature_records(device_id, timestamp)
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS hourly_temperatures (
            id TEXT PRIMARY KEY,
            device_id TEXT NOT NULL,
            hour_start DATETIME NOT NULL,
            avg_temp REAL NOT NULL,
            min_temp REAL NOT NULL,
            max_temp REAL NOT NULL,
            sample_count INTEGER NOT NULL,
            avg_humidity REAL,
            UNIQUE(device_id, hour_start)
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_hourly_temp_device_time 
        ON hourly_temperatures(device_id, hour_start)
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS device_temp_configs (
            id TEXT PRIMARY KEY,
            device_id TEXT NOT NULL UNIQUE,
            min_temp REAL NOT NULL,
            max_temp REAL NOT NULL,
            created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS temperature_alerts (
            id TEXT PRIMARY KEY,
            device_id TEXT NOT NULL,
            alert_type TEXT NOT NULL,
            temperature REAL NOT NULL,
            threshold REAL NOT NULL,
            deviation REAL NOT NULL,
            start_time DATETIME NOT NULL,
            end_time DATETIME,
            duration_seconds INTEGER,
            is_resolved INTEGER NOT NULL DEFAULT 0,
            created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_alerts_device_time 
        ON temperature_alerts(device_id, start_time)
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_alerts_unresolved 
        ON temperature_alerts(device_id, is_resolved)
        "#,
    )
    .execute(&pool)
    .await?;

    Ok(pool)
}

pub async fn insert_temperature_record(
    pool: &SqlitePool,
    report: &TemperatureReport,
) -> Result<TemperatureRecord> {
    let id = Uuid::new_v4().to_string();
    let timestamp = Utc::now();
    let now = timestamp;
    let client_timestamp = report.client_timestamp;

    sqlx::query(
        r#"
        INSERT INTO temperature_records (id, device_id, temperature, humidity, timestamp, client_timestamp, created_at)
        VALUES (?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&id)
    .bind(&report.device_id)
    .bind(report.temperature)
    .bind(report.humidity)
    .bind(timestamp)
    .bind(client_timestamp)
    .bind(now)
    .execute(pool)
    .await?;

    Ok(TemperatureRecord {
        id,
        device_id: report.device_id.clone(),
        temperature: report.temperature,
        humidity: report.humidity,
        timestamp,
        client_timestamp,
        created_at: now,
    })
}

pub async fn get_records_in_hour(
    pool: &SqlitePool,
    device_id: &str,
    hour_start: DateTime<Utc>,
) -> Result<Vec<TemperatureRecord>> {
    let hour_end = hour_start + Duration::hours(1);

    let records = sqlx::query_as::<_, TemperatureRecord>(
        r#"
        SELECT id, device_id, temperature, humidity, timestamp, client_timestamp, created_at
        FROM temperature_records
        WHERE device_id = ? AND timestamp >= ? AND timestamp < ?
        ORDER BY timestamp ASC
        "#,
    )
    .bind(device_id)
    .bind(hour_start)
    .bind(hour_end)
    .fetch_all(pool)
    .await?;

    Ok(records)
}

pub async fn upsert_hourly_temperature(
    pool: &SqlitePool,
    hourly: &HourlyTemperature,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO hourly_temperatures 
            (id, device_id, hour_start, avg_temp, min_temp, max_temp, sample_count, avg_humidity)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(device_id, hour_start) DO UPDATE SET
            avg_temp = excluded.avg_temp,
            min_temp = excluded.min_temp,
            max_temp = excluded.max_temp,
            sample_count = excluded.sample_count,
            avg_humidity = excluded.avg_humidity
        "#,
    )
    .bind(&hourly.id)
    .bind(&hourly.device_id)
    .bind(hourly.hour_start)
    .bind(hourly.avg_temp)
    .bind(hourly.min_temp)
    .bind(hourly.max_temp)
    .bind(hourly.sample_count)
    .bind(hourly.avg_humidity)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn get_hourly_temperatures(
    pool: &SqlitePool,
    device_id: Option<&str>,
    start_time: Option<DateTime<Utc>>,
    end_time: Option<DateTime<Utc>>,
) -> Result<Vec<HourlyTemperature>> {
    let mut query = String::from(
        "SELECT id, device_id, hour_start, avg_temp, min_temp, max_temp, sample_count, avg_humidity 
         FROM hourly_temperatures WHERE 1=1",
    );

    if device_id.is_some() {
        query.push_str(" AND device_id = ?");
    }
    if start_time.is_some() {
        query.push_str(" AND hour_start >= ?");
    }
    if end_time.is_some() {
        query.push_str(" AND hour_start < ?");
    }
    query.push_str(" ORDER BY hour_start DESC");

    let mut q = sqlx::query_as::<_, HourlyTemperature>(&query);

    if let Some(did) = device_id {
        q = q.bind(did);
    }
    if let Some(st) = start_time {
        q = q.bind(st);
    }
    if let Some(et) = end_time {
        q = q.bind(et);
    }

    let results = q.fetch_all(pool).await?;
    Ok(results)
}

pub async fn get_all_device_ids(pool: &SqlitePool) -> Result<Vec<String>> {
    let devices: Vec<(String,)> = sqlx::query_as(
        "SELECT DISTINCT device_id FROM temperature_records ORDER BY device_id",
    )
    .fetch_all(pool)
    .await?;

    Ok(devices.into_iter().map(|(d,)| d).collect())
}

pub async fn upsert_device_config(
    pool: &SqlitePool,
    req: &DeviceTempConfigRequest,
) -> Result<DeviceTempConfig> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now();

    sqlx::query(
        r#"
        INSERT INTO device_temp_configs (id, device_id, min_temp, max_temp, created_at, updated_at)
        VALUES (?, ?, ?, ?, ?, ?)
        ON CONFLICT(device_id) DO UPDATE SET
            min_temp = excluded.min_temp,
            max_temp = excluded.max_temp,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(&id)
    .bind(&req.device_id)
    .bind(req.min_temp)
    .bind(req.max_temp)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await?;

    get_device_config(pool, &req.device_id).await?
        .ok_or_else(|| anyhow::anyhow!("设备配置创建失败"))
}

pub async fn get_device_config(
    pool: &SqlitePool,
    device_id: &str,
) -> Result<Option<DeviceTempConfig>> {
    let config = sqlx::query_as::<_, DeviceTempConfig>(
        "SELECT id, device_id, min_temp, max_temp, created_at, updated_at 
         FROM device_temp_configs WHERE device_id = ?",
    )
    .bind(device_id)
    .fetch_optional(pool)
    .await?;

    Ok(config)
}

pub async fn get_all_device_configs(pool: &SqlitePool) -> Result<Vec<DeviceTempConfig>> {
    let configs = sqlx::query_as::<_, DeviceTempConfig>(
        "SELECT id, device_id, min_temp, max_temp, created_at, updated_at 
         FROM device_temp_configs ORDER BY device_id",
    )
    .fetch_all(pool)
    .await?;

    Ok(configs)
}

pub async fn create_alert(
    pool: &SqlitePool,
    device_id: &str,
    alert_type: &str,
    temperature: f64,
    threshold: f64,
    deviation: f64,
    start_time: DateTime<Utc>,
) -> Result<TemperatureAlert> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now();

    sqlx::query(
        r#"
        INSERT INTO temperature_alerts 
            (id, device_id, alert_type, temperature, threshold, deviation, start_time, is_resolved, created_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, 0, ?)
        "#,
    )
    .bind(&id)
    .bind(device_id)
    .bind(alert_type)
    .bind(temperature)
    .bind(threshold)
    .bind(deviation)
    .bind(start_time)
    .bind(now)
    .execute(pool)
    .await?;

    let alert = sqlx::query_as::<_, TemperatureAlert>(
        r#"
        SELECT id, device_id, alert_type, temperature, threshold, deviation, 
               start_time, end_time, duration_seconds, is_resolved, created_at
        FROM temperature_alerts WHERE id = ?
        "#,
    )
    .bind(&id)
    .fetch_one(pool)
    .await?;

    Ok(alert)
}

pub async fn get_unresolved_alert(
    pool: &SqlitePool,
    device_id: &str,
    alert_type: &str,
) -> Result<Option<TemperatureAlert>> {
    let alert = sqlx::query_as::<_, TemperatureAlert>(
        r#"
        SELECT id, device_id, alert_type, temperature, threshold, deviation,
               start_time, end_time, duration_seconds, is_resolved, created_at
        FROM temperature_alerts 
        WHERE device_id = ? AND alert_type = ? AND is_resolved = 0
        ORDER BY start_time DESC LIMIT 1
        "#,
    )
    .bind(device_id)
    .bind(alert_type)
    .fetch_optional(pool)
    .await?;

    Ok(alert)
}

pub async fn resolve_alert(
    pool: &SqlitePool,
    alert_id: &str,
    end_time: DateTime<Utc>,
) -> Result<()> {
    let alert = sqlx::query_as::<_, TemperatureAlert>(
        "SELECT id, device_id, alert_type, temperature, threshold, deviation,
                start_time, end_time, duration_seconds, is_resolved, created_at
         FROM temperature_alerts WHERE id = ?",
    )
    .bind(alert_id)
    .fetch_optional(pool)
    .await?;

    if let Some(alert) = alert {
        let duration = (end_time - alert.start_time).num_seconds();

        sqlx::query(
            r#"
            UPDATE temperature_alerts 
            SET end_time = ?, duration_seconds = ?, is_resolved = 1
            WHERE id = ?
            "#,
        )
        .bind(end_time)
        .bind(duration)
        .bind(alert_id)
        .execute(pool)
        .await?;
    }

    Ok(())
}

pub async fn update_alert_temperature(
    pool: &SqlitePool,
    alert_id: &str,
    temperature: f64,
    deviation: f64,
) -> Result<()> {
    sqlx::query(
        "UPDATE temperature_alerts SET temperature = ?, deviation = ? WHERE id = ?",
    )
    .bind(temperature)
    .bind(deviation)
    .bind(alert_id)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn get_alerts(
    pool: &SqlitePool,
    device_id: Option<&str>,
    start_time: Option<DateTime<Utc>>,
    end_time: Option<DateTime<Utc>>,
    resolved: Option<bool>,
) -> Result<Vec<TemperatureAlert>> {
    let mut query = String::from(
        r#"
        SELECT id, device_id, alert_type, temperature, threshold, deviation,
               start_time, end_time, duration_seconds, is_resolved, created_at
        FROM temperature_alerts WHERE 1=1
        "#,
    );

    if device_id.is_some() {
        query.push_str(" AND device_id = ?");
    }
    if start_time.is_some() {
        query.push_str(" AND start_time >= ?");
    }
    if end_time.is_some() {
        query.push_str(" AND start_time < ?");
    }
    if let Some(r) = resolved {
        if r {
            query.push_str(" AND is_resolved = 1");
        } else {
            query.push_str(" AND is_resolved = 0");
        }
    }
    query.push_str(" ORDER BY start_time DESC");

    let mut q = sqlx::query_as::<_, TemperatureAlert>(&query);

    if let Some(did) = device_id {
        q = q.bind(did);
    }
    if let Some(st) = start_time {
        q = q.bind(st);
    }
    if let Some(et) = end_time {
        q = q.bind(et);
    }

    let results = q.fetch_all(pool).await?;
    Ok(results)
}
