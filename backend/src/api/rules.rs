use actix_web::{delete, get, post, put, web, HttpRequest, HttpResponse};
use chrono::NaiveTime;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::config::Config;
use crate::db::models::Device;
use crate::error::{AppError, AppResult};

use super::auth::extract_user_from_request;

#[derive(Debug, Deserialize)]
pub struct CreateRuleRequest {
    pub device_id: Uuid,
    pub name: String,
    pub max_hours: i32,
    pub time_window_start: Option<NaiveTime>,
    pub time_window_end: Option<NaiveTime>,
    pub min_continuous_hours: Option<i32>,
    pub days_of_week: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateRuleRequest {
    pub name: Option<String>,
    pub max_hours: Option<i32>,
    pub time_window_start: Option<NaiveTime>,
    pub time_window_end: Option<NaiveTime>,
    pub min_continuous_hours: Option<i32>,
    pub days_of_week: Option<i32>,
    pub is_enabled: Option<bool>,
}

/// Struct per queries amb JOIN
#[derive(Debug, FromRow)]
struct RuleWithDevice {
    id: Uuid,
    device_id: Uuid,
    name: String,
    max_hours: i32,
    time_window_start: Option<NaiveTime>,
    time_window_end: Option<NaiveTime>,
    min_continuous_hours: i32,
    days_of_week: i32,
    is_enabled: bool,
    device_name: String,
}

#[derive(Debug, Serialize)]
pub struct RuleResponse {
    pub id: Uuid,
    pub device_id: Uuid,
    pub device_name: String,
    pub name: String,
    pub max_hours: i32,
    pub time_window_start: Option<NaiveTime>,
    pub time_window_end: Option<NaiveTime>,
    pub min_continuous_hours: i32,
    pub days_of_week: i32,
    pub is_enabled: bool,
}

impl From<RuleWithDevice> for RuleResponse {
    fn from(r: RuleWithDevice) -> Self {
        Self {
            id: r.id,
            device_id: r.device_id,
            device_name: r.device_name,
            name: r.name,
            max_hours: r.max_hours,
            time_window_start: r.time_window_start,
            time_window_end: r.time_window_end,
            min_continuous_hours: r.min_continuous_hours,
            days_of_week: r.days_of_week,
            is_enabled: r.is_enabled,
        }
    }
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(list_rules)
        .service(create_rule)
        .service(get_rule)
        .service(update_rule)
        .service(delete_rule);
}

/// GET /api/rules
#[get("/rules")]
async fn list_rules(
    pool: web::Data<PgPool>,
    config: web::Data<Config>,
    req: HttpRequest,
) -> AppResult<HttpResponse> {
    let user = extract_user_from_request(&req, &pool, &config.jwt_secret).await?;

    let rules = sqlx::query_as::<_, RuleWithDevice>(
        r#"
        SELECT r.id, r.device_id, r.name, r.max_hours, r.time_window_start,
               r.time_window_end, r.min_continuous_hours, r.days_of_week, r.is_enabled,
               d.name as device_name
        FROM rules r
        JOIN devices d ON r.device_id = d.id
        WHERE d.user_id = $1
        ORDER BY r.name
        "#
    )
    .bind(user.id)
    .fetch_all(pool.get_ref())
    .await?;

    let response: Vec<RuleResponse> = rules.into_iter().map(Into::into).collect();
    Ok(HttpResponse::Ok().json(response))
}

/// POST /api/rules
#[post("/rules")]
async fn create_rule(
    pool: web::Data<PgPool>,
    config: web::Data<Config>,
    req: HttpRequest,
    body: web::Json<CreateRuleRequest>,
) -> AppResult<HttpResponse> {
    let user = extract_user_from_request(&req, &pool, &config.jwt_secret).await?;

    // Verificar que el dispositiu pertany a l'usuari
    let device = sqlx::query_as::<_, Device>(
        "SELECT * FROM devices WHERE id = $1 AND user_id = $2"
    )
    .bind(body.device_id)
    .bind(user.id)
    .fetch_optional(pool.get_ref())
    .await?
    .ok_or_else(|| AppError::NotFound("Device not found".to_string()))?;

    // Validacions
    if body.max_hours < 1 || body.max_hours > 24 {
        return Err(AppError::BadRequest("max_hours must be between 1 and 24".to_string()));
    }

    let min_continuous = body.min_continuous_hours.unwrap_or(1);
    if min_continuous < 1 || min_continuous > body.max_hours {
        return Err(AppError::BadRequest(
            "min_continuous_hours must be between 1 and max_hours".to_string()
        ));
    }

    let rule = sqlx::query_as::<_, RuleWithDevice>(
        r#"
        WITH inserted AS (
            INSERT INTO rules (device_id, name, max_hours, time_window_start, time_window_end, min_continuous_hours, days_of_week)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            RETURNING *
        )
        SELECT i.id, i.device_id, i.name, i.max_hours, i.time_window_start,
               i.time_window_end, i.min_continuous_hours, i.days_of_week, i.is_enabled,
               $8::text as device_name
        FROM inserted i
        "#
    )
    .bind(body.device_id)
    .bind(&body.name)
    .bind(body.max_hours)
    .bind(body.time_window_start)
    .bind(body.time_window_end)
    .bind(min_continuous)
    .bind(body.days_of_week.unwrap_or(127))
    .bind(&device.name)
    .fetch_one(pool.get_ref())
    .await?;

    Ok(HttpResponse::Created().json(RuleResponse::from(rule)))
}

/// GET /api/rules/{id}
#[get("/rules/{id}")]
async fn get_rule(
    pool: web::Data<PgPool>,
    config: web::Data<Config>,
    req: HttpRequest,
    path: web::Path<Uuid>,
) -> AppResult<HttpResponse> {
    let user = extract_user_from_request(&req, &pool, &config.jwt_secret).await?;
    let rule_id = path.into_inner();

    let rule = sqlx::query_as::<_, RuleWithDevice>(
        r#"
        SELECT r.id, r.device_id, r.name, r.max_hours, r.time_window_start,
               r.time_window_end, r.min_continuous_hours, r.days_of_week, r.is_enabled,
               d.name as device_name
        FROM rules r
        JOIN devices d ON r.device_id = d.id
        WHERE r.id = $1 AND d.user_id = $2
        "#
    )
    .bind(rule_id)
    .bind(user.id)
    .fetch_optional(pool.get_ref())
    .await?
    .ok_or_else(|| AppError::NotFound("Rule not found".to_string()))?;

    Ok(HttpResponse::Ok().json(RuleResponse::from(rule)))
}

/// PUT /api/rules/{id}
#[put("/rules/{id}")]
async fn update_rule(
    pool: web::Data<PgPool>,
    config: web::Data<Config>,
    req: HttpRequest,
    path: web::Path<Uuid>,
    body: web::Json<UpdateRuleRequest>,
) -> AppResult<HttpResponse> {
    let user = extract_user_from_request(&req, &pool, &config.jwt_secret).await?;
    let rule_id = path.into_inner();

    // Verificar que la regla pertany a un dispositiu de l'usuari
    let existing = sqlx::query_as::<_, RuleWithDevice>(
        r#"
        SELECT r.id, r.device_id, r.name, r.max_hours, r.time_window_start,
               r.time_window_end, r.min_continuous_hours, r.days_of_week, r.is_enabled,
               d.name as device_name
        FROM rules r
        JOIN devices d ON r.device_id = d.id
        WHERE r.id = $1 AND d.user_id = $2
        "#
    )
    .bind(rule_id)
    .bind(user.id)
    .fetch_optional(pool.get_ref())
    .await?
    .ok_or_else(|| AppError::NotFound("Rule not found".to_string()))?;

    // Aplicar actualitzacions
    let new_name = body.name.as_ref().unwrap_or(&existing.name);
    let new_max_hours = body.max_hours.unwrap_or(existing.max_hours);
    let new_time_window_start = body.time_window_start.or(existing.time_window_start);
    let new_time_window_end = body.time_window_end.or(existing.time_window_end);
    let new_min_continuous = body.min_continuous_hours.unwrap_or(existing.min_continuous_hours);
    let new_days_of_week = body.days_of_week.unwrap_or(existing.days_of_week);
    let new_is_enabled = body.is_enabled.unwrap_or(existing.is_enabled);

    let updated = sqlx::query_as::<_, RuleWithDevice>(
        r#"
        WITH updated AS (
            UPDATE rules
            SET name = $1, max_hours = $2, time_window_start = $3, time_window_end = $4,
                min_continuous_hours = $5, days_of_week = $6, is_enabled = $7, updated_at = NOW()
            WHERE id = $8
            RETURNING *
        )
        SELECT u.id, u.device_id, u.name, u.max_hours, u.time_window_start,
               u.time_window_end, u.min_continuous_hours, u.days_of_week, u.is_enabled,
               $9::text as device_name
        FROM updated u
        "#
    )
    .bind(new_name)
    .bind(new_max_hours)
    .bind(new_time_window_start)
    .bind(new_time_window_end)
    .bind(new_min_continuous)
    .bind(new_days_of_week)
    .bind(new_is_enabled)
    .bind(rule_id)
    .bind(&existing.device_name)
    .fetch_one(pool.get_ref())
    .await?;

    Ok(HttpResponse::Ok().json(RuleResponse::from(updated)))
}

/// DELETE /api/rules/{id}
#[delete("/rules/{id}")]
async fn delete_rule(
    pool: web::Data<PgPool>,
    config: web::Data<Config>,
    req: HttpRequest,
    path: web::Path<Uuid>,
) -> AppResult<HttpResponse> {
    let user = extract_user_from_request(&req, &pool, &config.jwt_secret).await?;
    let rule_id = path.into_inner();

    // Verificar que la regla pertany a un dispositiu de l'usuari i eliminar
    let result = sqlx::query(
        r#"
        DELETE FROM rules
        WHERE id = $1 AND device_id IN (
            SELECT id FROM devices WHERE user_id = $2
        )
        "#
    )
    .bind(rule_id)
    .bind(user.id)
    .execute(pool.get_ref())
    .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Rule not found".to_string()));
    }

    Ok(HttpResponse::NoContent().finish())
}
