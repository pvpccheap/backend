use chrono::{NaiveTime, NaiveDate};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Preu d'una hora específica
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HourlyPrice {
    pub hour: u8,
    pub price: f64,  // €/kWh
}

/// Preus PVPC d'un dia complet
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyPrices {
    pub date: NaiveDate,
    pub prices: Vec<HourlyPrice>,
}

/// Tipus de dispositiu
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeviceType {
    Switch,
    Thermostat,
    Light,
    Other(String),
}

/// Dies de la setmana com a bitmask
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct DaysOfWeek(pub u8);

impl DaysOfWeek {
    pub const MONDAY: u8 = 1;
    pub const TUESDAY: u8 = 2;
    pub const WEDNESDAY: u8 = 4;
    pub const THURSDAY: u8 = 8;
    pub const FRIDAY: u8 = 16;
    pub const SATURDAY: u8 = 32;
    pub const SUNDAY: u8 = 64;
    pub const ALL_DAYS: u8 = 127;
    pub const WEEKDAYS: u8 = 31;  // Dilluns a divendres
    pub const WEEKEND: u8 = 96;   // Dissabte i diumenge

    pub fn new(mask: u8) -> Self {
        Self(mask)
    }

    pub fn all() -> Self {
        Self(Self::ALL_DAYS)
    }

    pub fn includes(&self, day: chrono::Weekday) -> bool {
        let bit = match day {
            chrono::Weekday::Mon => Self::MONDAY,
            chrono::Weekday::Tue => Self::TUESDAY,
            chrono::Weekday::Wed => Self::WEDNESDAY,
            chrono::Weekday::Thu => Self::THURSDAY,
            chrono::Weekday::Fri => Self::FRIDAY,
            chrono::Weekday::Sat => Self::SATURDAY,
            chrono::Weekday::Sun => Self::SUNDAY,
        };
        (self.0 & bit) != 0
    }
}

impl Default for DaysOfWeek {
    fn default() -> Self {
        Self::all()
    }
}

/// Estat d'una acció programada
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActionStatus {
    Pending,
    Executed,
    Failed,
    Cancelled,
}

/// DTO per crear una regla
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRuleRequest {
    pub device_id: Uuid,
    pub name: String,
    pub max_hours: i32,
    pub time_window_start: Option<NaiveTime>,
    pub time_window_end: Option<NaiveTime>,
    pub min_continuous_hours: Option<i32>,
    pub days_of_week: Option<u8>,
}

/// DTO per actualitzar una regla
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateRuleRequest {
    pub name: Option<String>,
    pub max_hours: Option<i32>,
    pub time_window_start: Option<NaiveTime>,
    pub time_window_end: Option<NaiveTime>,
    pub min_continuous_hours: Option<i32>,
    pub days_of_week: Option<u8>,
    pub is_enabled: Option<bool>,
}

/// DTO per sincronitzar dispositius des de l'app Android
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncDeviceRequest {
    pub google_device_id: String,
    pub name: String,
    pub device_type: Option<String>,
    pub room: Option<String>,
}

/// Acció programada per enviar a l'app Android
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledActionResponse {
    pub id: Uuid,
    pub device_id: Uuid,
    pub device_name: String,
    pub google_device_id: String,
    pub action: String,  // "on" o "off"
    pub scheduled_time: NaiveTime,
    pub status: ActionStatus,
}
