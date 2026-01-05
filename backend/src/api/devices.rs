use actix_web::{delete, get, patch, post, web, HttpRequest, HttpResponse};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::config::Config;
use crate::db::models::Device;
use crate::error::{AppError, AppResult};

use super::auth::extract_user_from_request;

#[derive(Debug, Deserialize)]
pub struct SyncDevicesRequest {
    pub devices: Vec<SyncDeviceItem>,
}

#[derive(Debug, Deserialize)]
pub struct SyncDeviceItem {
    pub google_device_id: String,
    pub name: String,
    pub device_type: Option<String>,
    pub room: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateDeviceRequest {
    pub is_active: Option<bool>,
    pub name: Option<String>,
    pub google_device_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DeviceResponse {
    pub id: Uuid,
    pub google_device_id: String,
    pub name: String,
    pub device_type: Option<String>,
    pub room: Option<String>,
    pub is_active: bool,
}

impl From<Device> for DeviceResponse {
    fn from(d: Device) -> Self {
        Self {
            id: d.id,
            google_device_id: d.google_device_id,
            name: d.name,
            device_type: d.device_type,
            room: d.room,
            is_active: d.is_active,
        }
    }
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(list_devices)
        .service(sync_devices)
        .service(update_device)
        .service(delete_device);
}

/// GET /api/devices
#[get("/devices")]
async fn list_devices(
    pool: web::Data<PgPool>,
    config: web::Data<Config>,
    req: HttpRequest,
) -> AppResult<HttpResponse> {
    let user = extract_user_from_request(&req, &pool, &config.jwt_secret).await?;

    let devices = sqlx::query_as::<_, Device>(
        "SELECT * FROM devices WHERE user_id = $1 ORDER BY name"
    )
    .bind(user.id)
    .fetch_all(pool.get_ref())
    .await?;

    let response: Vec<DeviceResponse> = devices.into_iter().map(Into::into).collect();
    Ok(HttpResponse::Ok().json(response))
}

/// POST /api/devices/sync
/// Sincronitza els dispositius des de l'app Android
#[post("/devices/sync")]
async fn sync_devices(
    pool: web::Data<PgPool>,
    config: web::Data<Config>,
    req: HttpRequest,
    body: web::Json<SyncDevicesRequest>,
) -> AppResult<HttpResponse> {
    let user = extract_user_from_request(&req, &pool, &config.jwt_secret).await?;

    let mut synced_devices = Vec::new();

    for device_data in &body.devices {
        // Upsert: insertar o actualitzar si ja existeix
        let device = sqlx::query_as::<_, Device>(
            r#"
            INSERT INTO devices (user_id, google_device_id, name, device_type, room)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (user_id, google_device_id)
            DO UPDATE SET
                name = EXCLUDED.name,
                device_type = EXCLUDED.device_type,
                room = EXCLUDED.room
            RETURNING *
            "#
        )
        .bind(user.id)
        .bind(&device_data.google_device_id)
        .bind(&device_data.name)
        .bind(&device_data.device_type)
        .bind(&device_data.room)
        .fetch_one(pool.get_ref())
        .await?;

        synced_devices.push(DeviceResponse::from(device));
    }

    Ok(HttpResponse::Ok().json(synced_devices))
}

/// PATCH /api/devices/{id}
#[patch("/devices/{id}")]
async fn update_device(
    pool: web::Data<PgPool>,
    config: web::Data<Config>,
    req: HttpRequest,
    path: web::Path<Uuid>,
    body: web::Json<UpdateDeviceRequest>,
) -> AppResult<HttpResponse> {
    let user = extract_user_from_request(&req, &pool, &config.jwt_secret).await?;
    let device_id = path.into_inner();

    // Verificar que el dispositiu pertany a l'usuari
    let existing = sqlx::query_as::<_, Device>(
        "SELECT * FROM devices WHERE id = $1 AND user_id = $2"
    )
    .bind(device_id)
    .bind(user.id)
    .fetch_optional(pool.get_ref())
    .await?
    .ok_or_else(|| AppError::NotFound("Device not found".to_string()))?;

    // Actualitzar només els camps proporcionats
    let new_name = body.name.as_ref().unwrap_or(&existing.name);
    let new_is_active = body.is_active.unwrap_or(existing.is_active);
    let new_google_device_id = body.google_device_id.as_ref().unwrap_or(&existing.google_device_id);

    let updated = sqlx::query_as::<_, Device>(
        r#"
        UPDATE devices
        SET name = $1, is_active = $2, google_device_id = $3
        WHERE id = $4
        RETURNING *
        "#
    )
    .bind(new_name)
    .bind(new_is_active)
    .bind(new_google_device_id)
    .bind(device_id)
    .fetch_one(pool.get_ref())
    .await?;

    Ok(HttpResponse::Ok().json(DeviceResponse::from(updated)))
}

/// DELETE /api/devices/{id}
#[delete("/devices/{id}")]
async fn delete_device(
    pool: web::Data<PgPool>,
    config: web::Data<Config>,
    req: HttpRequest,
    path: web::Path<Uuid>,
) -> AppResult<HttpResponse> {
    let user = extract_user_from_request(&req, &pool, &config.jwt_secret).await?;
    let device_id = path.into_inner();

    let result = sqlx::query(
        "DELETE FROM devices WHERE id = $1 AND user_id = $2"
    )
    .bind(device_id)
    .bind(user.id)
    .execute(pool.get_ref())
    .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Device not found".to_string()));
    }

    Ok(HttpResponse::NoContent().finish())
}
