use chrono::{Datelike, Local, NaiveTime, Timelike};
use shared::DailyPrices;
use sqlx::PgPool;
use std::sync::Arc;
use tokio::time::{interval, Duration};

use crate::db::models::Rule;
use crate::services::pvpc::PvpcClient;
use crate::services::scheduler::calculate_optimal_hours;

/// Hora a la qual es generen els schedules de demà (20:30)
const SCHEDULE_GENERATION_HOUR: u32 = 20;
const SCHEDULE_GENERATION_MINUTE: u32 = 30;

/// Interval de reintent si falla (30 minuts)
const RETRY_INTERVAL_MINUTES: u64 = 30;

/// Interval de comprovació (cada minut)
const CHECK_INTERVAL_SECONDS: u64 = 60;

/// Inicia les tasques en background
pub fn start_background_tasks(pool: Arc<PgPool>, pvpc_client: Arc<PvpcClient>) {
    let pool_clone = pool.clone();
    let pvpc_clone = pvpc_client.clone();
    let pool_for_cleanup = pool.clone();

    // Tasca 1: Generació de schedules
    tokio::spawn(async move {
        // Primer, comprovar si falten schedules d'avui
        check_and_generate_today_schedules(&pool_clone, &pvpc_clone).await;

        // Després, iniciar el scheduler diari
        run_daily_scheduler(pool_clone, pvpc_clone).await;
    });

    // Tasca 2: Marcar accions pendents expirades com a 'missed'
    tokio::spawn(async move {
        run_expired_actions_checker(pool_for_cleanup).await;
    });
}

/// Comprova si hi ha schedules per avui i demà, si no, els genera
async fn check_and_generate_today_schedules(pool: &PgPool, pvpc: &PvpcClient) {
    let now = Local::now();
    let today = now.date_naive();
    let tomorrow = today + chrono::Duration::days(1);

    // === Generar schedules per AVUI ===
    let existing_today: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM scheduled_actions WHERE scheduled_date = $1"
    )
    .bind(today)
    .fetch_one(pool)
    .await
    .unwrap_or(0);

    if existing_today > 0 {
        tracing::info!(
            "Ja existeixen {} schedules per avui ({}), no cal generar-ne",
            existing_today,
            today
        );
    } else {
        tracing::info!("No hi ha schedules per avui ({}), intentant generar-los...", today);
        match generate_schedules_for_date(pool, pvpc, today).await {
            Ok(count) => {
                tracing::info!("Generats {} schedules per avui ({})", count, today);
            }
            Err(e) => {
                tracing::warn!(
                    "No s'han pogut generar schedules per avui: {}. Es reintentarà més tard.",
                    e
                );
            }
        }
    }

    // === Generar schedules per DEMÀ (si ja són passades les 20:30) ===
    let is_after_schedule_time = now.hour() > SCHEDULE_GENERATION_HOUR
        || (now.hour() == SCHEDULE_GENERATION_HOUR && now.minute() >= SCHEDULE_GENERATION_MINUTE);

    if is_after_schedule_time {
        let existing_tomorrow: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM scheduled_actions WHERE scheduled_date = $1"
        )
        .bind(tomorrow)
        .fetch_one(pool)
        .await
        .unwrap_or(0);

        if existing_tomorrow > 0 {
            tracing::info!(
                "Ja existeixen {} schedules per demà ({}), no cal generar-ne",
                existing_tomorrow,
                tomorrow
            );
        } else {
            tracing::info!(
                "Són passades les {}:{:02} i no hi ha schedules per demà ({}), intentant generar-los...",
                SCHEDULE_GENERATION_HOUR,
                SCHEDULE_GENERATION_MINUTE,
                tomorrow
            );
            match generate_schedules_for_date(pool, pvpc, tomorrow).await {
                Ok(count) => {
                    tracing::info!("Generats {} schedules per demà ({})", count, tomorrow);
                }
                Err(e) => {
                    tracing::warn!(
                        "No s'han pogut generar schedules per demà: {}. Es reintentarà més tard.",
                        e
                    );
                }
            }
        }
    }
}

/// Scheduler que s'executa cada dia a les 20:30
async fn run_daily_scheduler(pool: Arc<PgPool>, pvpc: Arc<PvpcClient>) {
    let mut check_interval = interval(Duration::from_secs(CHECK_INTERVAL_SECONDS));
    let mut last_generation_date: Option<chrono::NaiveDate> = None;
    let mut retry_pending = false;
    let mut last_retry: Option<chrono::DateTime<Local>> = None;

    loop {
        check_interval.tick().await;

        let now = Local::now();
        let today = now.date_naive();
        let tomorrow = today + chrono::Duration::days(1);

        // Comprovar si és hora de generar schedules (20:30)
        let is_schedule_time = now.hour() == SCHEDULE_GENERATION_HOUR
            && now.minute() >= SCHEDULE_GENERATION_MINUTE
            && now.minute() < SCHEDULE_GENERATION_MINUTE + 1;

        // Comprovar si ja hem generat per demà avui
        let already_generated_today = last_generation_date == Some(tomorrow);

        // Comprovar si cal reintentar
        let should_retry = retry_pending && last_retry.map_or(true, |last| {
            now.signed_duration_since(last).num_minutes() >= RETRY_INTERVAL_MINUTES as i64
        });

        if (is_schedule_time && !already_generated_today) || should_retry {
            tracing::info!(
                "Generant schedules per demà ({})...",
                tomorrow
            );

            match generate_schedules_for_date(&pool, &pvpc, tomorrow).await {
                Ok(count) => {
                    tracing::info!(
                        "Generats {} schedules per demà ({})",
                        count,
                        tomorrow
                    );
                    last_generation_date = Some(tomorrow);
                    retry_pending = false;
                    last_retry = None;
                }
                Err(e) => {
                    tracing::error!(
                        "Error generant schedules per demà: {}. Es reintentarà en {} minuts.",
                        e,
                        RETRY_INTERVAL_MINUTES
                    );
                    retry_pending = true;
                    last_retry = Some(now);
                }
            }
        }
    }
}

/// Genera schedules per una data específica
async fn generate_schedules_for_date(
    pool: &PgPool,
    pvpc: &PvpcClient,
    date: chrono::NaiveDate,
) -> Result<usize, String> {
    let today = Local::now().date_naive();

    // Obtenir els preus per la data
    let prices = if date == today {
        pvpc.get_today_prices().await
    } else if date == today + chrono::Duration::days(1) {
        pvpc.get_tomorrow_prices().await
    } else {
        pvpc.get_prices_for_date(date).await
    };

    let prices = prices.map_err(|e| format!("Error obtenint preus: {:?}", e))?;

    // Utilitzar la funció existent per generar schedules
    // Però primer hem de modificar-la per acceptar una data i preus
    let count = generate_schedule_with_prices(pool, &prices, date)
        .await
        .map_err(|e| format!("Error generant schedules: {:?}", e))?;

    Ok(count)
}

/// Genera schedules per una data amb preus ja obtinguts
async fn generate_schedule_with_prices(
    pool: &PgPool,
    prices: &DailyPrices,
    date: chrono::NaiveDate,
) -> Result<usize, sqlx::Error> {

    // Obtenir totes les regles actives
    let rules = sqlx::query_as::<_, Rule>(
        "SELECT * FROM rules WHERE is_enabled = true"
    )
    .fetch_all(pool)
    .await?;

    let mut created_count = 0;
    let rules_count = rules.len();

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
            continue; // Aquesta regla no s'aplica aquest dia
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
            // end_time és sempre l'hora següent (00:00 per l'hora 23)
            // Quan start_time > end_time, significa que l'acció creua mitjanit
            // L'Android i el backend han de tractar aquest cas especialment
            let end_time = NaiveTime::from_hms_opt(((*hour + 1) % 24) as u32, 0, 0).unwrap();

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

    tracing::info!(
        "Processades {} regles, creats {} scheduled_actions per {}",
        rules_count,
        created_count,
        date
    );

    Ok(created_count)
}

/// Comprova cada minut si hi ha accions pendents que ja han expirat i les marca com 'missed'
async fn run_expired_actions_checker(pool: Arc<PgPool>) {
    let mut check_interval = interval(Duration::from_secs(CHECK_INTERVAL_SECONDS));

    loop {
        check_interval.tick().await;

        if let Err(e) = mark_expired_actions_as_missed(&pool).await {
            tracing::error!("Error marcant accions expirades: {}", e);
        }
    }
}

/// Marca les accions pendents que ja han passat la seva hora end_time com a 'missed'
///
/// Lògica:
/// - Accions normals (ex: 10:00-14:00, start < end): es marquen com missed quan current_time >= end_time
/// - Accions que creuen mitjanit (ex: 23:00-00:00, start > end): NO es marquen com missed el mateix dia,
///   sinó quan el dia següent arriba (scheduled_date < today)
///
/// Això és consistent amb la lògica de l'app Android (ScheduleExecutionWorker.markMissedActionsAsFailed)
async fn mark_expired_actions_as_missed(pool: &PgPool) -> Result<(), sqlx::Error> {
    let now = Local::now();
    let today = now.date_naive();
    let current_time = now.time();

    // Cas 1: Accions normals d'avui (end_time > start_time) que ja han acabat
    // Ex: 10:00-14:00 i ara són les 15:00 → missed
    let result = sqlx::query(
        r#"
        UPDATE scheduled_actions
        SET status = 'missed'
        WHERE status = 'pending'
          AND scheduled_date = $1
          AND end_time > start_time
          AND end_time <= $2
        "#
    )
    .bind(today)
    .bind(current_time)
    .execute(pool)
    .await?;

    if result.rows_affected() > 0 {
        tracing::info!(
            "Marcades {} accions normals com a 'missed' (data: {}, hora actual: {})",
            result.rows_affected(),
            today,
            current_time.format("%H:%M")
        );
    }

    // Cas 2: Accions de dies anteriors que encara estiguin pendents
    // Inclou tant accions normals com les que creuaven mitjanit
    // Ex: Acció de ahir 23:00-00:00 que no es va executar → missed
    let result_old = sqlx::query(
        r#"
        UPDATE scheduled_actions
        SET status = 'missed'
        WHERE status = 'pending'
          AND scheduled_date < $1
        "#
    )
    .bind(today)
    .execute(pool)
    .await?;

    if result_old.rows_affected() > 0 {
        tracing::info!(
            "Marcades {} accions de dies anteriors com a 'missed'",
            result_old.rows_affected()
        );
    }

    Ok(())
}
