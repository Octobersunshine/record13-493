use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::db::{get_hourly_temperatures, get_records_in_hour, insert_temperature_record, upsert_hourly_temperature};
use crate::models::{HourlyTemperature, TemperatureRecord, TemperatureReport};

fn floor_to_hour(dt: DateTime<Utc>) -> DateTime<Utc> {
    dt.with_minute(0).unwrap()
      .with_second(0).unwrap()
      .with_nanosecond(0).unwrap()
}

pub async fn report_temperature(
    pool: &SqlitePool,
    report: &TemperatureReport,
) -> Result<TemperatureRecord> {
    let record = insert_temperature_record(pool, report).await?;

    let hour_start = floor_to_hour(record.timestamp);
    aggregate_hour(pool, &record.device_id, hour_start).await?;

    Ok(record)
}

async fn aggregate_hour(
    pool: &SqlitePool,
    device_id: &str,
    hour_start: DateTime<Utc>,
) -> Result<()> {
    let records = get_records_in_hour(pool, device_id, hour_start).await?;

    if records.is_empty() {
        return Ok(());
    }

    let mut min_temp = f64::INFINITY;
    let mut max_temp = f64::NEG_INFINITY;
    let mut sum_temp = 0.0;
    let mut sum_humidity = 0.0;
    let mut humidity_count = 0;

    for record in &records {
        if record.temperature < min_temp {
            min_temp = record.temperature;
        }
        if record.temperature > max_temp {
            max_temp = record.temperature;
        }
        sum_temp += record.temperature;

        if let Some(hum) = record.humidity {
            sum_humidity += hum;
            humidity_count += 1;
        }
    }

    let avg_temp = sum_temp / records.len() as f64;
    let avg_humidity = if humidity_count > 0 {
        Some(sum_humidity / humidity_count as f64)
    } else {
        None
    };

    let hourly = HourlyTemperature {
        id: Uuid::new_v4().to_string(),
        device_id: device_id.to_string(),
        hour_start,
        avg_temp,
        min_temp,
        max_temp,
        sample_count: records.len() as i64,
        avg_humidity,
    };

    upsert_hourly_temperature(pool, &hourly).await?;

    Ok(())
}

pub async fn get_hourly_data(
    pool: &SqlitePool,
    device_id: Option<&str>,
    start_time: Option<DateTime<Utc>>,
    end_time: Option<DateTime<Utc>>,
) -> Result<Vec<HourlyTemperature>> {
    let mut end = end_time;
    if end.is_none() {
        end = Some(Utc::now() + Duration::hours(1));
    }

    let result = get_hourly_temperatures(pool, device_id, start_time, end).await?;
    Ok(result)
}

pub async fn batch_report_temperatures(
    pool: &SqlitePool,
    reports: &[TemperatureReport],
) -> Result<Vec<TemperatureRecord>> {
    let mut records = Vec::new();
    let mut hours_to_update: Vec<(String, DateTime<Utc>)> = Vec::new();

    for report in reports {
        let record = insert_temperature_record(pool, report).await?;
        let hour_start = floor_to_hour(record.timestamp);

        let key = (record.device_id.clone(), hour_start);
        if !hours_to_update.contains(&key) {
            hours_to_update.push(key);
        }

        records.push(record);
    }

    for (device_id, hour_start) in &hours_to_update {
        aggregate_hour(pool, device_id, *hour_start).await?;
    }

    Ok(records)
}
