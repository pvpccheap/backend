use actix_web::{delete, get, post, put, web, HttpRequest, HttpResponse};
use chrono::{Datelike, Local, NaiveTime};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::config::Config;
use crate::db::models::{Device, Rule};
use crate::error::{AppError, AppResult};
use crate::services::pvpc::PvpcClient;
use crate::services::scheduler::calculate_optimal_hours;

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schedule_info: Option<ScheduleGenerationInfo>,
}

#[derive(Debug, Serialize)]
pub struct ScheduleGenerationInfo {
    pub schedules_created: usize,
    pub message: String,
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
            schedule_info: None,
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
    pvpc: web::Data<PvpcClient>,
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

    // Generar schedules per la nova regla
    tracing::info!("Generant schedules per la nova regla '{}'...", rule.name);
    let db_rule = Rule {
        id: rule.id,
        device_id: rule.device_id,
        name: rule.name.clone(),
        max_hours: rule.max_hours,
        time_window_start: rule.time_window_start,
        time_window_end: rule.time_window_end,
        min_continuous_hours: rule.min_continuous_hours,
        days_of_week: rule.days_of_week,
        is_enabled: rule.is_enabled,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };

    let schedule_info = match regenerate_schedules_for_rule(pool.get_ref(), &pvpc, &db_rule).await {
        Ok(info) => {
            tracing::info!("Creats {} schedules per la nova regla '{}': {}", info.schedules_created, rule.name, info.message);
            Some(info)
        }
        Err(e) => {
            tracing::error!("Error generant schedules per la nova regla '{}': {}", rule.name, e);
            None
        }
    };

    let mut response = RuleResponse::from(rule);
    response.schedule_info = schedule_info;

    Ok(HttpResponse::Created().json(response))
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
    pvpc: web::Data<PvpcClient>,
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

    // Regenerar schedules si la regla ha canviat
    let db_rule = Rule {
        id: updated.id,
        device_id: updated.device_id,
        name: updated.name.clone(),
        max_hours: updated.max_hours,
        time_window_start: updated.time_window_start,
        time_window_end: updated.time_window_end,
        min_continuous_hours: updated.min_continuous_hours,
        days_of_week: updated.days_of_week,
        is_enabled: updated.is_enabled,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };

    let schedule_info = if updated.is_enabled {
        // Si està habilitada, regenerar schedules
        tracing::info!("Regenerant schedules per la regla '{}'...", updated.name);
        match regenerate_schedules_for_rule(pool.get_ref(), &pvpc, &db_rule).await {
            Ok(info) => {
                tracing::info!("Regenerats {} schedules per la regla '{}': {}", info.schedules_created, updated.name, info.message);
                Some(info)
            }
            Err(e) => {
                tracing::error!("Error regenerant schedules per la regla '{}': {}", updated.name, e);
                None
            }
        }
    } else {
        // Si s'ha desactivat, cancel·lar schedules pendents
        tracing::info!("Cancel·lant schedules per la regla desactivada '{}'...", updated.name);
        let cancelled = cancel_pending_schedules_for_rule(pool.get_ref(), rule_id).await.unwrap_or(0);
        Some(ScheduleGenerationInfo {
            schedules_created: 0,
            message: format!("Regla desactivada. {} schedules pendents cancel·lats.", cancelled),
        })
    };

    let mut response = RuleResponse::from(updated);
    response.schedule_info = schedule_info;

    Ok(HttpResponse::Ok().json(response))
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

/// Regenera els schedules per una regla (avui i demà si els preus estan disponibles)
/// Retorna informació sobre els schedules generats
async fn regenerate_schedules_for_rule(
    pool: &PgPool,
    pvpc: &PvpcClient,
    rule: &Rule,
) -> Result<ScheduleGenerationInfo, Box<dyn std::error::Error + Send + Sync>> {
    let now = Local::now();
    let today = now.date_naive();
    let tomorrow = today + chrono::Duration::days(1);
    let current_time = now.time();

    // Primer, eliminar schedules pendents d'aquesta regla (que encara no han passat)
    sqlx::query(
        r#"
        DELETE FROM scheduled_actions
        WHERE rule_id = $1
          AND status = 'pending'
          AND (scheduled_date > $2 OR (scheduled_date = $2 AND start_time > $3))
        "#
    )
    .bind(rule.id)
    .bind(today)
    .bind(current_time)
    .execute(pool)
    .await?;

    let mut created_count = 0;
    let mut today_count = 0;
    let mut tomorrow_count = 0;
    let mut today_available = false;
    let mut tomorrow_available = false;

    // Generar per avui (només hores futures)
    match pvpc.get_today_prices().await {
        Ok(prices) => {
            today_available = !prices.prices.is_empty();
            tracing::info!(
                "Preus d'avui ({}) obtinguts: {} hores",
                today,
                prices.prices.len()
            );
            let count = generate_schedules_for_rule_and_date(pool, rule, &prices, today, Some(current_time)).await?;
            tracing::info!(
                "Generats {} schedules per avui (hores futures després de {})",
                count,
                current_time.format("%H:%M")
            );
            today_count = count;
            created_count += count;
        }
        Err(e) => {
            tracing::warn!("No s'han pogut obtenir els preus d'avui: {:?}", e);
        }
    }

    // Generar per demà (si els preus estan disponibles)
    match pvpc.get_tomorrow_prices().await {
        Ok(prices) => {
            tomorrow_available = !prices.prices.is_empty();
            if tomorrow_available {
                let count = generate_schedules_for_rule_and_date(pool, rule, &prices, tomorrow, None).await?;
                tracing::info!("Generats {} schedules per demà ({})", count, tomorrow);
                tomorrow_count = count;
                created_count += count;
            } else {
                tracing::info!("Preus de demà ({}) encara no disponibles", tomorrow);
            }
        }
        Err(e) => {
            tracing::warn!("No s'han pogut obtenir els preus de demà: {:?}", e);
        }
    }

    tracing::info!(
        "Regenerats {} schedules per la regla '{}' (id: {})",
        created_count,
        rule.name,
        rule.id
    );

    // Generar missatge informatiu
    let message = if created_count > 0 {
        format!(
            "Creats {} schedules ({} per avui, {} per demà)",
            created_count, today_count, tomorrow_count
        )
    } else if today_available && !tomorrow_available {
        "Les hores òptimes d'avui ja han passat. Els schedules de demà es generaran a les 20:30 quan els preus estiguin disponibles.".to_string()
    } else if !today_available && !tomorrow_available {
        "Els preus encara no estan disponibles. Els schedules es generaran automàticament quan els preus estiguin disponibles.".to_string()
    } else {
        "No s'han pogut generar schedules per aquesta regla avui.".to_string()
    };

    Ok(ScheduleGenerationInfo {
        schedules_created: created_count,
        message,
    })
}

/// Genera schedules per una regla i una data específica
async fn generate_schedules_for_rule_and_date(
    pool: &PgPool,
    rule: &Rule,
    prices: &shared::DailyPrices,
    date: chrono::NaiveDate,
    min_time: Option<NaiveTime>,
) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
    // Comprovar si el dia de la setmana està inclòs
    let weekday = date.weekday();
    let day_bit = match weekday {
        chrono::Weekday::Mon => 1,
        chrono::Weekday::Tue => 2,
        chrono::Weekday::Wed => 4,
        chrono::Weekday::Thu => 8,
        chrono::Weekday::Fri => 16,
        chrono::Weekday::Sat => 32,
        chrono::Weekday::Sun => 64,
    };

    if (rule.days_of_week & day_bit) == 0 {
        return Ok(0);
    }

    // Calcular les hores òptimes
    let optimal = calculate_optimal_hours(
        &prices.prices,
        rule.max_hours,
        rule.min_continuous_hours,
        rule.time_window_start,
        rule.time_window_end,
    );

    let mut created_count = 0;

    for hour in &optimal.hours {
        let start_time = NaiveTime::from_hms_opt(*hour as u32, 0, 0).unwrap();

        // Si hi ha min_time, saltar hores que ja han passat
        if let Some(min) = min_time {
            if start_time <= min {
                continue;
            }
        }

        // Per l'hora 23, end_time seria 00:00 que causa problemes de comparació
        // Usem 23:59:59 per evitar que end_time < start_time
        let end_time = if *hour == 23 {
            NaiveTime::from_hms_opt(23, 59, 59).unwrap()
        } else {
            NaiveTime::from_hms_opt(*hour as u32 + 1, 0, 0).unwrap()
        };
        let price = prices.prices.iter()
            .find(|p| p.hour == *hour)
            .map(|p| p.price);

        let result = sqlx::query(
            r#"
            INSERT INTO scheduled_actions (rule_id, scheduled_date, start_time, end_time, price_per_kwh, status)
            VALUES ($1, $2, $3, $4, $5, 'pending')
            ON CONFLICT (rule_id, scheduled_date, start_time) DO NOTHING
            "#
        )
        .bind(rule.id)
        .bind(date)
        .bind(start_time)
        .bind(end_time)
        .bind(price)
        .execute(pool)
        .await?;

        if result.rows_affected() > 0 {
            created_count += 1;
        }
    }

    Ok(created_count)
}

/// Cancel·la els schedules pendents d'una regla (quan es desactiva)
async fn cancel_pending_schedules_for_rule(
    pool: &PgPool,
    rule_id: Uuid,
) -> Result<u64, sqlx::Error> {
    let now = Local::now();
    let today = now.date_naive();
    let current_time = now.time();

    let result = sqlx::query(
        r#"
        UPDATE scheduled_actions
        SET status = 'cancelled'
        WHERE rule_id = $1
          AND status = 'pending'
          AND (scheduled_date > $2 OR (scheduled_date = $2 AND start_time > $3))
        "#
    )
    .bind(rule_id)
    .bind(today)
    .bind(current_time)
    .execute(pool)
    .await?;

    if result.rows_affected() > 0 {
        tracing::info!(
            "Cancel·lats {} schedules pendents per la regla {}",
            result.rows_affected(),
            rule_id
        );
    }

    Ok(result.rows_affected())
}
