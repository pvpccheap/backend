use std::sync::Arc;
use std::time::{Duration, Instant};

use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use reqwest::Client;
use serde::Deserialize;
use tokio::sync::RwLock;

use crate::api::auth::GoogleIdTokenClaims;
use crate::error::{AppError, AppResult};

const GOOGLE_CERTS_URL: &str = "https://www.googleapis.com/oauth2/v3/certs";
const GOOGLE_ISSUERS: &[&str] = &["accounts.google.com", "https://accounts.google.com"];
const CERTS_CACHE_DURATION: Duration = Duration::from_secs(3600); // 1 hora

/// Claus públiques de Google en format JWK
#[derive(Debug, Deserialize)]
struct GoogleCerts {
    keys: Vec<GoogleJwk>,
}

#[derive(Debug, Clone, Deserialize)]
struct GoogleJwk {
    kid: String,
    n: String,  // RSA modulus
    e: String,  // RSA exponent
    #[serde(default)]
    alg: Option<String>,
}

/// Claims del token ID de Google (format intern)
#[derive(Debug, Deserialize)]
struct GoogleTokenClaims {
    sub: String,
    email: String,
    #[serde(default)]
    email_verified: Option<bool>,
    name: Option<String>,
    picture: Option<String>,
    aud: String,
    iss: String,
    exp: i64,
    iat: i64,
}

/// Cache de claus de Google
struct CertsCache {
    certs: Vec<GoogleJwk>,
    fetched_at: Instant,
}

/// Servei d'autenticació de Google
#[derive(Clone)]
pub struct GoogleAuthService {
    client: Client,
    cache: Arc<RwLock<Option<CertsCache>>>,
}

impl GoogleAuthService {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            cache: Arc::new(RwLock::new(None)),
        }
    }

    /// Verifica un token ID de Google
    pub async fn verify_id_token(
        &self,
        token: &str,
        expected_client_id: &str,
    ) -> AppResult<GoogleIdTokenClaims> {
        // Obtenir les claus públiques de Google (amb cache)
        let certs = self.get_google_certs().await?;

        // Extreure el kid del header del token
        let header = jsonwebtoken::decode_header(token)
            .map_err(|_| AppError::Unauthorized("Invalid token header".to_string()))?;

        let kid = header
            .kid
            .ok_or_else(|| AppError::Unauthorized("Token missing kid".to_string()))?;

        // Trobar la clau corresponent
        let jwk = certs
            .iter()
            .find(|k| k.kid == kid)
            .ok_or_else(|| AppError::Unauthorized("Unknown signing key".to_string()))?;

        // Crear la clau de decodificació
        let decoding_key = DecodingKey::from_rsa_components(&jwk.n, &jwk.e)
            .map_err(|_| AppError::Unauthorized("Invalid key format".to_string()))?;

        // Configurar validació
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_audience(&[expected_client_id]);
        validation.set_issuer(GOOGLE_ISSUERS);
        validation.validate_exp = true;

        // Decodificar i validar
        let token_data = decode::<GoogleTokenClaims>(token, &decoding_key, &validation)
            .map_err(|e| {
                tracing::warn!("Google token validation failed: {:?}", e);
                AppError::Unauthorized("Invalid Google token".to_string())
            })?;

        let claims = token_data.claims;

        // Verificar que l'email està verificat (opcional però recomanat)
        if claims.email_verified == Some(false) {
            return Err(AppError::Unauthorized("Email not verified".to_string()));
        }

        Ok(GoogleIdTokenClaims {
            sub: claims.sub,
            email: claims.email,
            name: claims.name,
            picture: claims.picture,
        })
    }

    /// Obté les claus públiques de Google (amb cache)
    async fn get_google_certs(&self) -> AppResult<Vec<GoogleJwk>> {
        // Comprovar cache
        {
            let cache = self.cache.read().await;
            if let Some(ref cached) = *cache {
                if cached.fetched_at.elapsed() < CERTS_CACHE_DURATION {
                    return Ok(cached.certs.clone());
                }
            }
        }

        // Obtenir noves claus
        let certs = self.fetch_google_certs().await?;

        // Actualitzar cache
        {
            let mut cache = self.cache.write().await;
            *cache = Some(CertsCache {
                certs: certs.clone(),
                fetched_at: Instant::now(),
            });
        }

        Ok(certs)
    }

    /// Descarrega les claus públiques de Google
    async fn fetch_google_certs(&self) -> AppResult<Vec<GoogleJwk>> {
        let response = self
            .client
            .get(GOOGLE_CERTS_URL)
            .send()
            .await
            .map_err(|e| {
                tracing::error!("Failed to fetch Google certs: {:?}", e);
                AppError::ExternalApi("Failed to fetch Google certificates".to_string())
            })?;

        if !response.status().is_success() {
            return Err(AppError::ExternalApi(format!(
                "Google certs API returned {}",
                response.status()
            )));
        }

        let certs: GoogleCerts = response.json().await.map_err(|e| {
            tracing::error!("Failed to parse Google certs: {:?}", e);
            AppError::ExternalApi("Failed to parse Google certificates".to_string())
        })?;

        Ok(certs.keys)
    }
}
