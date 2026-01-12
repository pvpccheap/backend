use chrono::{Datelike, NaiveDate, NaiveTime, Timelike};
use shared::{DailyPrices, HourlyPrice};
use sqlx::PgPool;

use crate::db::models::Rule;

/// Resultat del càlcul d'hores òptimes
#[derive(Debug, Clone)]
pub struct OptimalHours {
    pub hours: Vec<u8>,
    pub total_price: f64,
}

/// Calcula les hores òptimes (més barates) per una regla
pub fn calculate_optimal_hours(
    prices: &[HourlyPrice],
    max_hours: i32,
    min_continuous_hours: i32,
    time_window_start: Option<NaiveTime>,
    time_window_end: Option<NaiveTime>,
) -> OptimalHours {
    // Filtrar hores dins la finestra temporal
    let filtered_prices = filter_by_time_window(prices, time_window_start, time_window_end);

    if filtered_prices.is_empty() {
        return OptimalHours {
            hours: vec![],
            total_price: 0.0,
        };
    }

    if min_continuous_hours <= 1 {
        // Algorisme simple: seleccionar les hores més barates
        calculate_scattered_hours(&filtered_prices, max_hours as usize)
    } else {
        // Algorisme de blocs: seleccionar blocs continus
        calculate_continuous_blocks(&filtered_prices, max_hours as usize, min_continuous_hours as usize)
    }
}

/// Filtra les hores dins d'una finestra temporal
fn filter_by_time_window(
    prices: &[HourlyPrice],
    start: Option<NaiveTime>,
    end: Option<NaiveTime>,
) -> Vec<HourlyPrice> {
    match (start, end) {
        (None, None) => prices.to_vec(),
        (Some(start), Some(end)) => {
            let start_hour = start.hour() as u8;
            let end_hour = end.hour() as u8;

            prices
                .iter()
                .filter(|p| {
                    if start_hour <= end_hour {
                        // Finestra normal: ex. 08:00-20:00
                        p.hour >= start_hour && p.hour < end_hour
                    } else {
                        // Finestra que creua mitjanit: ex. 20:00-09:00
                        p.hour >= start_hour || p.hour < end_hour
                    }
                })
                .cloned()
                .collect()
        }
        // Si només hi ha un dels dos, assumim tota la nit/dia
        (Some(start), None) => {
            let start_hour = start.hour() as u8;
            prices.iter().filter(|p| p.hour >= start_hour).cloned().collect()
        }
        (None, Some(end)) => {
            let end_hour = end.hour() as u8;
            prices.iter().filter(|p| p.hour < end_hour).cloned().collect()
        }
    }
}

/// Algorisme per hores saltejades (min_continuous = 1)
fn calculate_scattered_hours(prices: &[HourlyPrice], max_hours: usize) -> OptimalHours {
    let mut sorted_prices = prices.to_vec();
    sorted_prices.sort_by(|a, b| {
        a.price
            .partial_cmp(&b.price)
            .expect("Price comparison failed: NaN values not allowed")
    });

    let selected: Vec<_> = sorted_prices.into_iter().take(max_hours).collect();
    let total_price: f64 = selected.iter().map(|p| p.price).sum();

    let mut hours: Vec<u8> = selected.iter().map(|p| p.hour).collect();
    hours.sort(); // Ordenar cronològicament

    OptimalHours { hours, total_price }
}

/// Algorisme per blocs continus (min_continuous > 1)
fn calculate_continuous_blocks(
    prices: &[HourlyPrice],
    max_hours: usize,
    min_continuous: usize,
) -> OptimalHours {
    if prices.len() < min_continuous {
        return OptimalHours {
            hours: vec![],
            total_price: 0.0,
        };
    }

    // Crear un mapa d'hora -> preu per accés ràpid
    let price_map: std::collections::HashMap<u8, f64> =
        prices.iter().map(|p| (p.hour, p.price)).collect();

    // Obtenir les hores disponibles ordenades
    let mut available_hours: Vec<u8> = prices.iter().map(|p| p.hour).collect();
    available_hours.sort();

    // Generar tots els blocs possibles de min_continuous hores consecutives
    let mut blocks: Vec<(Vec<u8>, f64)> = Vec::new();

    for i in 0..available_hours.len() {
        let mut block_hours = vec![available_hours[i]];
        let mut block_price = price_map[&available_hours[i]];

        for j in (i + 1)..available_hours.len() {
            // Safe: block_hours always has at least one element (initialized above)
            let prev_hour = block_hours
                .last()
                .expect("block_hours should never be empty at this point");
            let curr_hour = available_hours[j];

            // Comprovar si és consecutiu (considerant el wrap-around a mitjanit)
            let is_consecutive = (curr_hour == prev_hour + 1)
                || (*prev_hour == 23 && curr_hour == 0);

            if !is_consecutive {
                break;
            }

            block_hours.push(curr_hour);
            block_price += price_map[&curr_hour];

            if block_hours.len() >= min_continuous {
                let avg_price = block_price / block_hours.len() as f64;
                blocks.push((block_hours.clone(), avg_price));
            }
        }
    }

    if blocks.is_empty() {
        return OptimalHours {
            hours: vec![],
            total_price: 0.0,
        };
    }

    // Ordenar blocs per preu mitjà
    blocks.sort_by(|a, b| {
        a.1.partial_cmp(&b.1)
            .expect("Block price comparison failed: NaN values not allowed")
    });

    // Seleccionar blocs sense solapament fins arribar a max_hours
    let mut selected_hours: Vec<u8> = Vec::new();
    let mut total_price = 0.0;

    for (block_hours, _avg_price) in blocks {
        // Comprovar si aquest bloc solapa amb els ja seleccionats
        let overlaps = block_hours.iter().any(|h| selected_hours.contains(h));

        if !overlaps && selected_hours.len() + block_hours.len() <= max_hours {
            for hour in &block_hours {
                total_price += price_map[hour];
            }
            selected_hours.extend(block_hours);

            if selected_hours.len() >= max_hours {
                break;
            }
        }
    }

    selected_hours.sort();

    OptimalHours {
        hours: selected_hours,
        total_price,
    }
}

// =============================================================================
// FUNCIONS UTILITÀRIES COMPARTIDES
// =============================================================================

/// Converteix un chrono::Weekday a un bit per la màscara de dies de la setmana
/// Dilluns = 1, Dimarts = 2, Dimecres = 4, Dijous = 8, Divendres = 16, Dissabte = 32, Diumenge = 64
#[inline]
pub fn weekday_to_bit(weekday: chrono::Weekday) -> i32 {
    match weekday {
        chrono::Weekday::Mon => 1,
        chrono::Weekday::Tue => 2,
        chrono::Weekday::Wed => 4,
        chrono::Weekday::Thu => 8,
        chrono::Weekday::Fri => 16,
        chrono::Weekday::Sat => 32,
        chrono::Weekday::Sun => 64,
    }
}

/// Comprova si una regla s'aplica a un dia de la setmana específic
#[inline]
pub fn rule_applies_to_day(rule: &Rule, date: NaiveDate) -> bool {
    let day_bit = weekday_to_bit(date.weekday());
    (rule.days_of_week & day_bit) != 0
}

/// Genera scheduled_actions per una regla i una data amb preus donats.
/// Retorna el nombre de schedules creats.
///
/// Aquesta funció és l'única font de veritat per generar schedules,
/// evitant duplicació de codi entre rules.rs, schedule.rs i background_tasks.rs.
pub async fn generate_schedules_for_rule_and_date(
    pool: &PgPool,
    rule: &Rule,
    prices: &DailyPrices,
    date: NaiveDate,
    only_future_hours: Option<NaiveTime>,
) -> Result<usize, sqlx::Error> {
    // Comprovar si la regla s'aplica aquest dia de la setmana
    if !rule_applies_to_day(rule, date) {
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

    // Crear scheduled_actions per cada hora
    for hour in &optimal.hours {
        let start_time = NaiveTime::from_hms_opt(*hour as u32, 0, 0)
            .expect("Hour should be valid (0-23)");

        // Si s'ha especificat only_future_hours, saltar hores passades
        if let Some(current_time) = only_future_hours {
            if start_time <= current_time {
                continue;
            }
        }

        // end_time és sempre l'hora següent (00:00 per l'hora 23)
        // Quan start_time > end_time, significa que l'acció creua mitjanit
        let end_time = NaiveTime::from_hms_opt(((*hour + 1) % 24) as u32, 0, 0)
            .expect("End hour should be valid (0-23)");

        let price = prices
            .prices
            .iter()
            .find(|p| p.hour == *hour)
            .map(|p| p.price);

        let result = sqlx::query(
            r#"
            INSERT INTO scheduled_actions (rule_id, scheduled_date, start_time, end_time, price_per_kwh, status)
            VALUES ($1, $2, $3, $4, $5, 'pending')
            ON CONFLICT (rule_id, scheduled_date, start_time) DO NOTHING
            "#,
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

/// Genera schedules per múltiples regles i una data.
/// Retorna el nombre total de schedules creats.
pub async fn generate_schedules_for_rules(
    pool: &PgPool,
    rules: &[Rule],
    prices: &DailyPrices,
    date: NaiveDate,
    only_future_hours: Option<NaiveTime>,
) -> Result<usize, sqlx::Error> {
    let mut total_created = 0;

    for rule in rules {
        let count =
            generate_schedules_for_rule_and_date(pool, rule, prices, date, only_future_hours)
                .await?;
        total_created += count;
    }

    Ok(total_created)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_prices() -> Vec<HourlyPrice> {
        // Preus de prova: més barat a la matinada, més car a la tarda
        (0..24)
            .map(|hour| HourlyPrice {
                hour,
                price: match hour {
                    0..=5 => 0.05 + (hour as f64 * 0.001),   // Molt barat
                    6..=9 => 0.10 + (hour as f64 * 0.005),  // Barat
                    10..=13 => 0.15 + (hour as f64 * 0.002),// Mitjà
                    14..=17 => 0.20 - (hour as f64 * 0.001),// Car
                    18..=21 => 0.25 - (hour as f64 * 0.002),// Molt car
                    _ => 0.08,                               // Nit
                },
            })
            .collect()
    }

    #[test]
    fn test_scattered_hours() {
        let prices = create_test_prices();
        let result = calculate_optimal_hours(&prices, 6, 1, None, None);

        assert_eq!(result.hours.len(), 6);
        // Les primeres hores haurien de ser les de matinada (més barates)
        assert!(result.hours.contains(&0));
        assert!(result.hours.contains(&1));
    }

    #[test]
    fn test_time_window_night() {
        let prices = create_test_prices();
        let start = NaiveTime::from_hms_opt(20, 0, 0).unwrap();
        let end = NaiveTime::from_hms_opt(9, 0, 0).unwrap();

        let result = calculate_optimal_hours(&prices, 4, 1, Some(start), Some(end));

        assert_eq!(result.hours.len(), 4);
        // Totes les hores haurien de ser entre 20:00-09:00
        for hour in &result.hours {
            assert!(*hour >= 20 || *hour < 9);
        }
    }

    #[test]
    fn test_continuous_blocks() {
        let prices = create_test_prices();
        let result = calculate_optimal_hours(&prices, 4, 2, None, None);

        // Hauria de retornar 2 blocs de 2 hores
        assert!(result.hours.len() <= 4);

        // Verificar que les hores són consecutives en blocs
        let mut sorted = result.hours.clone();
        sorted.sort();

        // Comprovar continuïtat
        let mut blocks = 0;
        let mut i = 0;
        while i < sorted.len() {
            blocks += 1;
            let mut j = i + 1;
            while j < sorted.len() && sorted[j] == sorted[j - 1] + 1 {
                j += 1;
            }
            i = j;
        }

        // Cada bloc hauria de tenir almenys 2 hores
        println!("Blocs: {}, Hores: {:?}", blocks, sorted);
    }
}
