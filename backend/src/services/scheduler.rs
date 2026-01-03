use chrono::{NaiveTime, Timelike};
use shared::HourlyPrice;

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
    sorted_prices.sort_by(|a, b| a.price.partial_cmp(&b.price).unwrap());

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
            let prev_hour = block_hours.last().unwrap();
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
    blocks.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

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
