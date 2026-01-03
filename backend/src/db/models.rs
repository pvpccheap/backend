use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct User {
    pub id: Uuid,
    pub google_id: String,
    pub email: String,
    pub name: Option<String>,
    pub picture_url: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Device {
    pub id: Uuid,
    pub user_id: Uuid,
    pub google_device_id: String,
    pub name: String,
    pub device_type: Option<String>,
    pub room: Option<String>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Rule {
    pub id: Uuid,
    pub device_id: Uuid,
    pub name: String,
    pub max_hours: i32,
    pub time_window_start: Option<NaiveTime>,
    pub time_window_end: Option<NaiveTime>,
    pub min_continuous_hours: i32,
    pub days_of_week: i32,
    pub is_enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct ScheduledAction {
    pub id: Uuid,
    pub rule_id: Uuid,
    pub scheduled_date: NaiveDate,
    pub start_time: NaiveTime,
    pub end_time: NaiveTime,
    pub price_per_kwh: Option<f64>,
    pub status: String,
    pub executed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// Vista que uneix scheduled_action amb device info
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct ScheduledActionWithDevice {
    pub id: Uuid,
    pub rule_id: Uuid,
    pub scheduled_date: NaiveDate,
    pub start_time: NaiveTime,
    pub end_time: NaiveTime,
    pub status: String,
    pub device_id: Uuid,
    pub device_name: String,
    pub google_device_id: String,
}
