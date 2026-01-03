use actix_web::{get, post, web, HttpRequest, HttpResponse};
use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::config::Config;
use crate::db::models::User;
use crate::error::{AppError, AppResult};
use crate::services::google::GoogleAuthService;

/// JWT Claims per tokens interns de l'aplicació
#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,  // user_id
    pub email: String,
    pub exp: i64,
    pub iat: i64,
}

#[derive(Debug, Deserialize)]
pub struct GoogleLoginRequest {
    pub id_token: String,
}

#[derive(Debug, Serialize)]
pub struct AuthResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: i64,
    pub user: UserResponse,
}

#[derive(Debug, Serialize)]
pub struct UserResponse {
    pub id: Uuid,
    pub email: String,
    pub name: Option<String>,
    pub picture_url: Option<String>,
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(google_login)
        .service(refresh_token)
        .service(get_me);
}

/// POST /api/auth/google
/// Login amb Google ID token
#[post("/auth/google")]
async fn google_login(
    pool: web::Data<PgPool>,
    config: web::Data<Config>,
    google_auth: web::Data<GoogleAuthService>,
    body: web::Json<GoogleLoginRequest>,
) -> AppResult<HttpResponse> {
    // Validar el token de Google amb verificació de signatura
    let google_claims = google_auth
        .verify_id_token(&body.id_token, &config.google_client_id)
        .await?;

    // Buscar o crear usuari
    let user = find_or_create_user(&pool, &google_claims).await?;

    // Generar JWT
    let (token, expires_in) = generate_jwt(&user, &config.jwt_secret)?;

    Ok(HttpResponse::Ok().json(AuthResponse {
        access_token: token,
        token_type: "Bearer".to_string(),
        expires_in,
        user: UserResponse {
            id: user.id,
            email: user.email,
            name: user.name,
            picture_url: user.picture_url,
        },
    }))
}

/// POST /api/auth/refresh
#[post("/auth/refresh")]
async fn refresh_token(
    pool: web::Data<PgPool>,
    config: web::Data<Config>,
    req: HttpRequest,
) -> AppResult<HttpResponse> {
    let user = extract_user_from_request(&req, &pool, &config.jwt_secret).await?;

    let (token, expires_in) = generate_jwt(&user, &config.jwt_secret)?;

    Ok(HttpResponse::Ok().json(AuthResponse {
        access_token: token,
        token_type: "Bearer".to_string(),
        expires_in,
        user: UserResponse {
            id: user.id,
            email: user.email,
            name: user.name,
            picture_url: user.picture_url,
        },
    }))
}

/// GET /api/auth/me
#[get("/auth/me")]
async fn get_me(
    pool: web::Data<PgPool>,
    config: web::Data<Config>,
    req: HttpRequest,
) -> AppResult<HttpResponse> {
    let user = extract_user_from_request(&req, &pool, &config.jwt_secret).await?;

    Ok(HttpResponse::Ok().json(UserResponse {
        id: user.id,
        email: user.email,
        name: user.name,
        picture_url: user.picture_url,
    }))
}

/// Claims validats del token de Google
pub struct GoogleIdTokenClaims {
    pub sub: String,
    pub email: String,
    pub name: Option<String>,
    pub picture: Option<String>,
}

async fn find_or_create_user(pool: &PgPool, claims: &GoogleIdTokenClaims) -> AppResult<User> {
    // Intentar trobar l'usuari existent
    let existing = sqlx::query_as::<_, User>("SELECT * FROM users WHERE google_id = $1")
        .bind(&claims.sub)
        .fetch_optional(pool)
        .await?;

    if existing.is_some() {
        // Actualitzar info si ha canviat
        let updated = sqlx::query_as::<_, User>(
            r#"
            UPDATE users
            SET email = $1, name = $2, picture_url = $3, updated_at = NOW()
            WHERE google_id = $4
            RETURNING *
            "#,
        )
        .bind(&claims.email)
        .bind(&claims.name)
        .bind(&claims.picture)
        .bind(&claims.sub)
        .fetch_one(pool)
        .await?;

        Ok(updated)
    } else {
        // Crear nou usuari
        let new_user = sqlx::query_as::<_, User>(
            r#"
            INSERT INTO users (google_id, email, name, picture_url)
            VALUES ($1, $2, $3, $4)
            RETURNING *
            "#,
        )
        .bind(&claims.sub)
        .bind(&claims.email)
        .bind(&claims.name)
        .bind(&claims.picture)
        .fetch_one(pool)
        .await?;

        Ok(new_user)
    }
}

fn generate_jwt(user: &User, secret: &str) -> AppResult<(String, i64)> {
    let expires_in = 3600 * 24; // 24 hores
    let now = Utc::now();
    let exp = now + Duration::seconds(expires_in);

    let claims = Claims {
        sub: user.id.to_string(),
        email: user.email.clone(),
        iat: now.timestamp(),
        exp: exp.timestamp(),
    };

    // Usar explícitament HS256
    let header = Header::new(Algorithm::HS256);

    let token = encode(&header, &claims, &EncodingKey::from_secret(secret.as_bytes()))?;

    Ok((token, expires_in))
}

pub async fn extract_user_from_request(
    req: &HttpRequest,
    pool: &PgPool,
    jwt_secret: &str,
) -> AppResult<User> {
    let auth_header = req
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| AppError::Unauthorized("Missing Authorization header".to_string()))?;

    let token = auth_header
        .strip_prefix("Bearer ")
        .ok_or_else(|| AppError::Unauthorized("Invalid Authorization format".to_string()))?;

    // Validació estricta: només acceptar HS256
    let mut validation = Validation::new(Algorithm::HS256);
    validation.validate_exp = true;

    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(jwt_secret.as_bytes()),
        &validation,
    )?;

    let user_id: Uuid = token_data
        .claims
        .sub
        .parse()
        .map_err(|_| AppError::Unauthorized("Invalid user ID in token".to_string()))?;

    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::Unauthorized("User not found".to_string()))?;

    Ok(user)
}
