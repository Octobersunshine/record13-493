use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use sqlx::SqlitePool;
use tracing::warn;
use uuid::Uuid;

use crate::db::{
    create_alert, get_alerts, get_device_config, get_hourly_temperatures, get_records_in_hour,
    get_unresolved_alert, insert_temperature_record, resolve_alert, update_alert_temperature,
    upsert_device_config, upsert_hourly_temperature,
};
use crate::models::{
    AlertStatistics, AlertType, DeviceTempConfig, DeviceTempConfigRequest, HourlyTemperature,
    TemperatureAlert, TemperatureRecord, TemperatureReport,
};

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

    check_temperature_alert(pool, &record).await?;

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

        check_temperature_alert(pool, &record).await?;

        records.push(record);
    }

    for (device_id, hour_start) in &hours_to_update {
        aggregate_hour(pool, device_id, *hour_start).await?;
    }

    Ok(records)
}

async fn check_temperature_alert(
    pool: &SqlitePool,
    record: &TemperatureRecord,
) -> Result<()> {
    let config = get_device_config(pool, &record.device_id).await?;
    let config = match config {
        Some(c) => c,
        None => return Ok(()),
    };

    let is_overheat = record.temperature > config.max_temp;
    let is_undercool = record.temperature < config.min_temp;

    let unresolved_overheat = get_unresolved_alert(pool, &record.device_id, "overheat").await?;
    let unresolved_undercool = get_unresolved_alert(pool, &record.device_id, "undercool").await?;

    if is_overheat {
        let deviation = record.temperature - config.max_temp;

        if let Some(alert) = unresolved_overheat {
            let max_deviation = if deviation > alert.deviation {
                deviation
            } else {
                alert.deviation
            };
            let max_temp = if record.temperature > alert.temperature {
                record.temperature
            } else {
                alert.temperature
            };
            update_alert_temperature(pool, &alert.id, max_temp, max_deviation).await?;
        } else {
            if let Some(under_alert) = unresolved_undercool {
                resolve_alert(pool, &under_alert.id, record.timestamp).await?;
                warn!(
                    "设备 {} 低温告警解除，持续 {} 秒",
                    record.device_id,
                    (record.timestamp - under_alert.start_time).num_seconds()
                );
            }

            create_alert(
                pool,
                &record.device_id,
                "overheat",
                record.temperature,
                config.max_temp,
                deviation,
                record.timestamp,
            )
            .await?;
            warn!(
                "设备 {} 超温告警！温度: {}°C, 阈值: {}°C, 偏差: {}°C",
                record.device_id, record.temperature, config.max_temp, deviation
            );
        }
    } else if is_undercool {
        let deviation = config.min_temp - record.temperature;

        if let Some(alert) = unresolved_undercool {
            let max_deviation = if deviation > alert.deviation {
                deviation
            } else {
                alert.deviation
            };
            let min_temp = if record.temperature < alert.temperature {
                record.temperature
            } else {
                alert.temperature
            };
            update_alert_temperature(pool, &alert.id, min_temp, max_deviation).await?;
        } else {
            if let Some(over_alert) = unresolved_overheat {
                resolve_alert(pool, &over_alert.id, record.timestamp).await?;
                warn!(
                    "设备 {} 超温告警解除，持续 {} 秒",
                    record.device_id,
                    (record.timestamp - over_alert.start_time).num_seconds()
                );
            }

            create_alert(
                pool,
                &record.device_id,
                "undercool",
                record.temperature,
                config.min_temp,
                deviation,
                record.timestamp,
            )
            .await?;
            warn!(
                "设备 {} 低温告警！温度: {}°C, 阈值: {}°C, 偏差: {}°C",
                record.device_id, record.temperature, config.min_temp, deviation
            );
        }
    } else {
        if let Some(over_alert) = unresolved_overheat {
            resolve_alert(pool, &over_alert.id, record.timestamp).await?;
            warn!(
                "设备 {} 超温告警解除，持续 {} 秒",
                record.device_id,
                (record.timestamp - over_alert.start_time).num_seconds()
            );
        }
        if let Some(under_alert) = unresolved_undercool {
            resolve_alert(pool, &under_alert.id, record.timestamp).await?;
            warn!(
                "设备 {} 低温告警解除，持续 {} 秒",
                record.device_id,
                (record.timestamp - under_alert.start_time).num_seconds()
            );
        }
    }

    Ok(())
}

pub async fn set_device_temp_config(
    pool: &SqlitePool,
    req: &DeviceTempConfigRequest,
) -> Result<DeviceTempConfig> {
    let config = upsert_device_config(pool, req).await?;
    Ok(config)
}

pub async fn get_device_temp_config(
    pool: &SqlitePool,
    device_id: &str,
) -> Result<Option<DeviceTempConfig>> {
    let config = get_device_config(pool, device_id).await?;
    Ok(config)
}

pub async fn get_alert_list(
    pool: &SqlitePool,
    device_id: Option<&str>,
    start_time: Option<DateTime<Utc>>,
    end_time: Option<DateTime<Utc>>,
    resolved: Option<bool>,
) -> Result<Vec<TemperatureAlert>> {
    let alerts = get_alerts(pool, device_id, start_time, end_time, resolved).await?;
    Ok(alerts)
}

pub async fn calculate_alert_statistics(
    pool: &SqlitePool,
    device_id: &str,
    start_time: Option<DateTime<Utc>>,
    end_time: Option<DateTime<Utc>>,
) -> Result<AlertStatistics> {
    let end = end_time.unwrap_or_else(Utc::now);
    let start = start_time.unwrap_or_else(|| end - Duration::days(30));

    let alerts = get_alerts(pool, Some(device_id), Some(start), Some(end), Some(true)).await?;

    let mut total_alerts = 0i64;
    let mut overheat_alerts = 0i64;
    let mut undercool_alerts = 0i64;
    let mut total_overheat_duration = 0i64;
    let mut total_undercool_duration = 0i64;
    let mut max_overheat_temp: Option<f64> = None;
    let mut min_undercool_temp: Option<f64> = None;
    let mut overheat_deviations: Vec<f64> = Vec::new();
    let mut undercool_deviations: Vec<f64> = Vec::new();

    for alert in &alerts {
        total_alerts += 1;
        let duration = alert.duration_seconds.unwrap_or(0);

        if alert.alert_type == "overheat" {
            overheat_alerts += 1;
            total_overheat_duration += duration;
            overheat_deviations.push(alert.deviation);

            max_overheat_temp = Some(match max_overheat_temp {
                Some(current) => current.max(alert.temperature),
                None => alert.temperature,
            });
        } else if alert.alert_type == "undercool" {
            undercool_alerts += 1;
            total_undercool_duration += duration;
            undercool_deviations.push(alert.deviation);

            min_undercool_temp = Some(match min_undercool_temp {
                Some(current) => current.min(alert.temperature),
                None => alert.temperature,
            });
        }
    }

    let unresolved = get_alerts(pool, Some(device_id), Some(start), Some(end), Some(false)).await?;
    for alert in &unresolved {
        total_alerts += 1;
        let duration = (end - alert.start_time).num_seconds();

        if alert.alert_type == "overheat" {
            overheat_alerts += 1;
            total_overheat_duration += duration;
            overheat_deviations.push(alert.deviation);
            max_overheat_temp = Some(match max_overheat_temp {
                Some(current) => current.max(alert.temperature),
                None => alert.temperature,
            });
        } else if alert.alert_type == "undercool" {
            undercool_alerts += 1;
            total_undercool_duration += duration;
            undercool_deviations.push(alert.deviation);
            min_undercool_temp = Some(match min_undercool_temp {
                Some(current) => current.min(alert.temperature),
                None => alert.temperature,
            });
        }
    }

    let avg_overheat_deviation = if overheat_deviations.is_empty() {
        None
    } else {
        let sum: f64 = overheat_deviations.iter().sum();
        Some(sum / overheat_deviations.len() as f64)
    };

    let avg_undercool_deviation = if undercool_deviations.is_empty() {
        None
    } else {
        let sum: f64 = undercool_deviations.iter().sum();
        Some(sum / undercool_deviations.len() as f64)
    };

    Ok(AlertStatistics {
        device_id: device_id.to_string(),
        total_alerts,
        overheat_alerts,
        undercool_alerts,
        total_overheat_duration_seconds: total_overheat_duration,
        total_undercool_duration_seconds: total_undercool_duration,
        total_alert_duration_seconds: total_overheat_duration + total_undercool_duration,
        max_overheat_temp,
        min_undercool_temp,
        avg_overheat_deviation,
        avg_undercool_deviation,
        start_time: start,
        end_time: end,
    })
}
