use actix_web::{get, patch, post, web, HttpRequest, HttpResponse};
use chrono::{Datelike, NaiveDate, NaiveTime};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::config::Config;
use crate::db::models::Rule;
use crate::error::{AppError, AppResult};
use crate::services::pvpc::PvpcClient;
use crate::services::scheduler::calculate_optimal_hours;

use super::auth::extract_user_from_request;

#[derive(Debug, Deserialize)]
pub struct CalculateRequest {
    pub rule_id: Uuid,
    pub date: Option<NaiveDate>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateStatusRequest {
    pub status: String, // "executed", "failed", "cancelled"
}

#[derive(Debug, Serialize)]
pub struct CalculateResponse {
    pub rule_id: Uuid,
    pub date: NaiveDate,
    pub optimal_hours: Vec<u8>,
    pub total_price: f64,
}

#[derive(Debug, FromRow)]
struct ScheduledActionRow {
    id: Uuid,
    device_id: Uuid,
    device_name: String,
    google_device_id: String,
    start_time: NaiveTime,
    end_time: NaiveTime,
    status: String,
}

#[derive(Debug, Serialize)]
pub struct ScheduleResponse {
    pub id: Uuid,
    pub device_id: Uuid,
    pub device_name: String,
    pub google_device_id: String,
    pub start_time: String,
    pub end_time: String,
    pub status: String,
}

impl From<ScheduledActionRow> for ScheduleResponse {
    fn from(a: ScheduledActionRow) -> Self {
        Self {
            id: a.id,
            device_id: a.device_id,
            device_name: a.device_name,
            google_device_id: a.google_device_id,
            start_time: a.start_time.to_string(),
            end_time: a.end_time.to_string(),
            status: a.status,
        }
    }
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(get_today_schedule)
        .service(get_schedule_by_date)
        .service(calculate_schedule)
        .service(generate_schedule_now)
        .service(update_schedule_status);
}

/// GET /api/schedule/today
#[get("/schedule/today")]
async fn get_today_schedule(
    pool: web::Data<PgPool>,
    config: web::Data<Config>,
    req: HttpRequest,
) -> AppResult<HttpResponse> {
    let user = extract_user_from_request(&req, &pool, &config.jwt_secret).await?;
    let today = chrono::Local::now().date_naive();

    let actions = get_schedule_for_user_and_date(pool.get_ref(), user.id, today).await?;
    Ok(HttpResponse::Ok().json(actions))
}

/// GET /api/schedule/{date}
#[get("/schedule/{date}")]
async fn get_schedule_by_date(
    pool: web::Data<PgPool>,
    config: web::Data<Config>,
    req: HttpRequest,
    path: web::Path<NaiveDate>,
) -> AppResult<HttpResponse> {
    let user = extract_user_from_request(&req, &pool, &config.jwt_secret).await?;
    let date = path.into_inner();

    let actions = get_schedule_for_user_and_date(pool.get_ref(), user.id, date).await?;
    Ok(HttpResponse::Ok().json(actions))
}

/// POST /api/schedule/generate
/// Força la generació de schedules per avui i demà (si els preus estan disponibles)
#[post("/schedule/generate")]
async fn generate_schedule_now(
    pool: web::Data<PgPool>,
    config: web::Data<Config>,
    pvpc: web::Data<PvpcClient>,
    req: HttpRequest,
) -> AppResult<HttpResponse> {
    let user = extract_user_from_request(&req, &pool, &config.jwt_secret).await?;
    let today = chrono::Local::now().date_naive();
    let tomorrow = today + chrono::Duration::days(1);

    // Obtenir totes les regles actives de l'usuari
    let rules = sqlx::query_as::<_, Rule>(
        r#"
        SELECT r.*
        FROM rules r
        JOIN devices d ON r.device_id = d.id
        WHERE r.is_enabled = true AND d.user_id = $1
        "#
    )
    .bind(user.id)
    .fetch_all(pool.get_ref())
    .await?;

    let mut total_created = 0;
    let mut results = Vec::new();

    // Generar per avui
    if let Ok(prices_today) = pvpc.get_today_prices().await {
        let count = generate_schedules_for_rules(&pool, &rules, &prices_today, today).await?;
        total_created += count;
        results.push(serde_json::json!({
            "date": today.to_string(),
            "count": count
        }));
    }

    // Generar per demà (si els preus estan disponibles)
    if let Ok(prices_tomorrow) = pvpc.get_tomorrow_prices().await {
        if !prices_tomorrow.prices.is_empty() {
            let count = generate_schedules_for_rules(&pool, &rules, &prices_tomorrow, tomorrow).await?;
            total_created += count;
            results.push(serde_json::json!({
                "date": tomorrow.to_string(),
                "count": count
            }));
        }
    }

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "message": format!("Generats {} schedules en total", total_created),
        "total_count": total_created,
        "details": results
    })))
}

/// Funció auxiliar per generar schedules per una llista de regles i una data
async fn generate_schedules_for_rules(
    pool: &PgPool,
    rules: &[Rule],
    prices: &shared::DailyPrices,
    date: NaiveDate,
) -> AppResult<usize> {
    let mut created_count = 0;

    for rule in rules {
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
            continue;
        }

        // Calcular les hores òptimes
        let optimal = calculate_optimal_hours(
            &prices.prices,
            rule.max_hours,
            rule.min_continuous_hours,
            rule.time_window_start,
            rule.time_window_end,
        );

        // Crear scheduled_actions per cada hora
        for hour in &optimal.hours {
            let start_time = NaiveTime::from_hms_opt(*hour as u32, 0, 0).unwrap();
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
    }

    Ok(created_count)
}

/// POST /api/schedule/calculate
/// Calcula les hores òptimes per una regla sense guardar-les
#[post("/schedule/calculate")]
async fn calculate_schedule(
    pool: web::Data<PgPool>,
    config: web::Data<Config>,
    pvpc: web::Data<PvpcClient>,
    req: HttpRequest,
    body: web::Json<CalculateRequest>,
) -> AppResult<HttpResponse> {
    let user = extract_user_from_request(&req, &pool, &config.jwt_secret).await?;

    // Verificar que la regla pertany a l'usuari
    let rule = sqlx::query_as::<_, Rule>(
        r#"
        SELECT r.*
        FROM rules r
        JOIN devices d ON r.device_id = d.id
        WHERE r.id = $1 AND d.user_id = $2
        "#
    )
    .bind(body.rule_id)
    .bind(user.id)
    .fetch_optional(pool.get_ref())
    .await?
    .ok_or_else(|| AppError::NotFound("Rule not found".to_string()))?;

    // Obtenir la data (avui per defecte)
    let date = body.date.unwrap_or_else(|| chrono::Local::now().date_naive());

    // Obtenir els preus
    let prices = pvpc.get_prices_for_date(date).await?;

    // Calcular les hores òptimes
    let optimal = calculate_optimal_hours(
        &prices.prices,
        rule.max_hours,
        rule.min_continuous_hours,
        rule.time_window_start,
        rule.time_window_end,
    );

    Ok(HttpResponse::Ok().json(CalculateResponse {
        rule_id: rule.id,
        date,
        optimal_hours: optimal.hours,
        total_price: optimal.total_price,
    }))
}

async fn get_schedule_for_user_and_date(
    pool: &PgPool,
    user_id: Uuid,
    date: NaiveDate,
) -> AppResult<Vec<ScheduleResponse>> {
    let actions = sqlx::query_as::<_, ScheduledActionRow>(
        r#"
        SELECT
            sa.id, sa.start_time, sa.end_time, sa.status,
            d.id as device_id, d.name as device_name, d.google_device_id
        FROM scheduled_actions sa
        JOIN rules r ON sa.rule_id = r.id
        JOIN devices d ON r.device_id = d.id
        WHERE d.user_id = $1 AND sa.scheduled_date = $2
        ORDER BY sa.start_time
        "#
    )
    .bind(user_id)
    .bind(date)
    .fetch_all(pool)
    .await?;

    let response: Vec<ScheduleResponse> = actions.into_iter().map(Into::into).collect();
    Ok(response)
}

/// PATCH /api/schedule/{id}/status
/// Actualitza l'estat d'una acció programada (executed, failed, cancelled)
#[patch("/schedule/{id}/status")]
async fn update_schedule_status(
    pool: web::Data<PgPool>,
    config: web::Data<Config>,
    req: HttpRequest,
    path: web::Path<Uuid>,
    body: web::Json<UpdateStatusRequest>,
) -> AppResult<HttpResponse> {
    let user = extract_user_from_request(&req, &pool, &config.jwt_secret).await?;
    let schedule_id = path.into_inner();

    // Validar que l'status és vàlid
    let valid_statuses = ["executed", "failed", "cancelled", "pending"];
    if !valid_statuses.contains(&body.status.as_str()) {
        return Err(AppError::BadRequest(format!(
            "Invalid status '{}'. Valid values: {:?}",
            body.status, valid_statuses
        )));
    }

    // Verificar que l'acció pertany a l'usuari
    let result = sqlx::query(
        r#"
        UPDATE scheduled_actions sa
        SET status = $1, executed_at = CASE WHEN $1 = 'executed' THEN NOW() ELSE executed_at END
        FROM rules r
        JOIN devices d ON r.device_id = d.id
        WHERE sa.id = $2 AND sa.rule_id = r.id AND d.user_id = $3
        "#
    )
    .bind(&body.status)
    .bind(schedule_id)
    .bind(user.id)
    .execute(pool.get_ref())
    .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Scheduled action not found".to_string()));
    }

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "id": schedule_id,
        "status": body.status,
        "message": "Status updated successfully"
    })))
}

