use actix_web::{get, web, HttpResponse};
use shared::DailyPrices;

use crate::error::AppResult;
use crate::services::pvpc::PvpcClient;

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(get_today_prices)
        .service(get_tomorrow_prices);
}

/// GET /api/prices/today
#[get("/prices/today")]
async fn get_today_prices(pvpc: web::Data<PvpcClient>) -> AppResult<HttpResponse> {
    let prices = pvpc.get_today_prices().await?;
    Ok(HttpResponse::Ok().json(prices))
}

/// GET /api/prices/tomorrow
#[get("/prices/tomorrow")]
async fn get_tomorrow_prices(pvpc: web::Data<PvpcClient>) -> AppResult<HttpResponse> {
    let prices = pvpc.get_tomorrow_prices().await?;
    Ok(HttpResponse::Ok().json(prices))
}

/// Resposta enriquida amb estadístiques
#[derive(serde::Serialize)]
pub struct PricesWithStats {
    #[serde(flatten)]
    pub prices: DailyPrices,
    pub stats: PriceStats,
}

#[derive(serde::Serialize)]
pub struct PriceStats {
    pub min_price: f64,
    pub max_price: f64,
    pub avg_price: f64,
    pub cheapest_hours: Vec<u8>,
    pub most_expensive_hours: Vec<u8>,
}

impl From<DailyPrices> for PricesWithStats {
    fn from(prices: DailyPrices) -> Self {
        let min_price = prices.prices.iter().map(|p| p.price).fold(f64::MAX, f64::min);
        let max_price = prices.prices.iter().map(|p| p.price).fold(f64::MIN, f64::max);
        let avg_price = prices.prices.iter().map(|p| p.price).sum::<f64>() / prices.prices.len() as f64;

        let mut sorted_by_price = prices.prices.clone();
        sorted_by_price.sort_by(|a, b| a.price.partial_cmp(&b.price).unwrap());

        let cheapest_hours: Vec<u8> = sorted_by_price.iter().take(6).map(|p| p.hour).collect();
        let most_expensive_hours: Vec<u8> = sorted_by_price.iter().rev().take(6).map(|p| p.hour).collect();

        PricesWithStats {
            prices,
            stats: PriceStats {
                min_price,
                max_price,
                avg_price,
                cheapest_hours,
                most_expensive_hours,
            },
        }
    }
}
