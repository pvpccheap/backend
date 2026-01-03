use chrono::NaiveDate;
use reqwest::Client;
use serde::Deserialize;
use shared::{DailyPrices, HourlyPrice};

use crate::error::{AppError, AppResult};

/// API oficial de ESIOS (Red Eléctrica de España)
/// Indicador 1001 = PVPC (Precio Voluntario para el Pequeño Consumidor)
/// Documentació: https://api.esios.ree.es/
/// Per obtenir el token, enviar email a consultasios@ree.es
const ESIOS_API_URL: &str = "https://api.esios.ree.es/indicators/1001";

/// GeoID per la península (8741)
const GEO_ID_PENINSULA: i32 = 8741;

/// Resposta de l'API ESIOS
#[derive(Debug, Deserialize)]
struct EsiosResponse {
    indicator: EsiosIndicator,
}

#[derive(Debug, Deserialize)]
struct EsiosIndicator {
    values: Vec<EsiosValue>,
}

#[derive(Debug, Deserialize)]
struct EsiosValue {
    value: f64,
    datetime: String,
    #[serde(default)]
    geo_id: Option<i32>,
}

#[derive(Clone)]
pub struct PvpcClient {
    client: Client,
    token: Option<String>,
}

impl PvpcClient {
    pub fn new() -> Self {
        // Intentar carregar el token des de variable d'entorn
        let token = std::env::var("ESIOS_TOKEN").ok();

        if token.is_none() {
            tracing::warn!(
                "ESIOS_TOKEN no configurat. Per obtenir-lo, envia un email a consultasios@ree.es \
                amb l'assumpte 'Personal token request'"
            );
        }

        Self {
            client: Client::new(),
            token,
        }
    }

    /// Crea un client amb un token específic
    pub fn with_token(token: String) -> Self {
        Self {
            client: Client::new(),
            token: Some(token),
        }
    }

    /// Obté els preus PVPC per avui
    pub async fn get_today_prices(&self) -> AppResult<DailyPrices> {
        let today = chrono::Local::now().date_naive();
        self.fetch_prices_for_date(today).await
    }

    /// Obté els preus PVPC per demà (disponible a partir de ~20:00)
    pub async fn get_tomorrow_prices(&self) -> AppResult<DailyPrices> {
        let tomorrow = chrono::Local::now().date_naive() + chrono::Duration::days(1);
        self.fetch_prices_for_date(tomorrow).await
    }

    /// Obté els preus per una data específica
    pub async fn get_prices_for_date(&self, date: NaiveDate) -> AppResult<DailyPrices> {
        self.fetch_prices_for_date(date).await
    }

    async fn fetch_prices_for_date(&self, date: NaiveDate) -> AppResult<DailyPrices> {
        let token = self.token.as_ref().ok_or_else(|| {
            AppError::ExternalApi(
                "ESIOS_TOKEN no configurat. Necessites un token de l'API de ESIOS.".to_string()
            )
        })?;

        // Construir les dates en format ISO 8601 amb timezone d'Espanya
        // L'API espera dates en format: 2024-01-15T00:00:00+01:00
        let start_date = format!("{}T00:00:00", date);
        let end_date = format!("{}T23:59:59", date);

        let url = format!(
            "{}?start_date={}&end_date={}&geo_ids={}",
            ESIOS_API_URL, start_date, end_date, GEO_ID_PENINSULA
        );

        tracing::debug!("Obtenint preus PVPC de: {}", url);

        let response = self
            .client
            .get(&url)
            .header("Accept", "application/json")
            .header("x-api-key", token)
            .send()
            .await
            .map_err(|e| {
                tracing::error!("Error connectant amb ESIOS: {:?}", e);
                AppError::ExternalApi(format!("Error connectant amb ESIOS: {}", e))
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            tracing::error!("ESIOS API error: {} - {}", status, body);
            return Err(AppError::ExternalApi(format!(
                "ESIOS API returned status {}: {}",
                status, body
            )));
        }

        let data: EsiosResponse = response.json().await.map_err(|e| {
            tracing::error!("Error parsejant resposta ESIOS: {:?}", e);
            AppError::ExternalApi(format!("Error parsejant resposta ESIOS: {}", e))
        })?;

        // Convertir la resposta al nostre format
        let mut prices: Vec<HourlyPrice> = data
            .indicator
            .values
            .into_iter()
            .filter(|v| v.geo_id == Some(GEO_ID_PENINSULA) || v.geo_id.is_none())
            .filter_map(|v| {
                // El datetime ve en format ISO 8601: "2024-01-15T00:00:00.000+01:00"
                // Extreure l'hora
                let hour = extract_hour_from_datetime(&v.datetime)?;
                Some(HourlyPrice {
                    hour,
                    // El preu ve en €/MWh, convertim a €/kWh
                    price: v.value / 1000.0,
                })
            })
            .collect();

        prices.sort_by_key(|p| p.hour);

        // Verificar que tenim les 24 hores
        if prices.len() != 24 {
            tracing::warn!(
                "S'esperaven 24 preus per {}, però s'han obtingut {}",
                date,
                prices.len()
            );
        }

        Ok(DailyPrices { date, prices })
    }
}

/// Extreu l'hora d'un datetime en format ISO 8601
fn extract_hour_from_datetime(datetime: &str) -> Option<u8> {
    // Format esperat: "2024-01-15T14:00:00.000+01:00" o similar
    let time_part = datetime.split('T').nth(1)?;
    let hour_str = time_part.split(':').next()?;
    hour_str.parse().ok()
}

impl Default for PvpcClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_hour() {
        assert_eq!(extract_hour_from_datetime("2024-01-15T00:00:00.000+01:00"), Some(0));
        assert_eq!(extract_hour_from_datetime("2024-01-15T14:00:00.000+01:00"), Some(14));
        assert_eq!(extract_hour_from_datetime("2024-01-15T23:00:00.000+01:00"), Some(23));
    }

    #[tokio::test]
    #[ignore] // Ignorar per defecte ja que necessita token
    async fn test_get_today_prices() {
        let token = std::env::var("ESIOS_TOKEN").expect("ESIOS_TOKEN requerit per aquest test");
        let client = PvpcClient::with_token(token);
        let result = client.get_today_prices().await;

        match result {
            Ok(prices) => {
                assert_eq!(prices.prices.len(), 24);
                for price in &prices.prices {
                    assert!(price.hour < 24);
                    assert!(price.price > 0.0);
                }
            }
            Err(e) => {
                panic!("API call failed: {}", e);
            }
        }
    }
}
