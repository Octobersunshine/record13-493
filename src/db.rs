use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use sqlx::{SqlitePool, sqlite::SqlitePoolOptions};
use uuid::Uuid;

use crate::models::{HourlyTemperature, TemperatureRecord, TemperatureReport};

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
