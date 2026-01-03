mod api;
mod background_tasks;
mod config;
mod db;
mod error;
mod services;

use std::sync::Arc;

use actix_cors::Cors;
use actix_web::{middleware, web, App, HttpServer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::config::Config;
use crate::services::google::GoogleAuthService;
use crate::services::pvpc::PvpcClient;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Carregar .env si existeix
    dotenvy::dotenv().ok();

    // Configurar logging
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "info,sqlx=warn".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Carregar configuració
    let config = Config::from_env().expect("Failed to load configuration");
    let server_addr = config.server_addr();

    tracing::info!("Starting server at http://{}", server_addr);

    // Crear pool de base de dades
    let pool = db::create_pool(&config.database_url)
        .await
        .expect("Failed to create database pool");

    // Executar migracions
    db::run_migrations(&pool)
        .await
        .expect("Failed to run migrations");

    tracing::info!("Database migrations completed");

    // Crear client HTTP compartit
    let http_client = reqwest::Client::new();

    // Crear client PVPC
    let pvpc_client = PvpcClient::new();

    // Crear servei d'autenticació de Google
    let google_auth = GoogleAuthService::new(http_client);

    // Encapsular amb Arc per compartir entre threads
    let config = Arc::new(config);
    let pool_arc = Arc::new(pool.clone());
    let pvpc_arc = Arc::new(pvpc_client.clone());

    // Iniciar background tasks (scheduler diari)
    background_tasks::start_background_tasks(pool_arc, pvpc_arc);
    tracing::info!("Background tasks started");

    // Iniciar servidor
    HttpServer::new(move || {
        let mut cors = Cors::default()
            .allowed_methods(vec!["GET", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"])
            .allowed_headers(vec![
                actix_web::http::header::AUTHORIZATION,
                actix_web::http::header::CONTENT_TYPE,
                actix_web::http::header::ACCEPT,
            ])
            .max_age(3600);

        // Configurar orígens permesos
        for origin in &config.allowed_origins {
            cors = cors.allowed_origin(origin);
        }

        App::new()
            .wrap(middleware::Logger::default())
            .wrap(tracing_actix_web::TracingLogger::default())
            .wrap(cors)
            .app_data(web::Data::new(pool.clone()))
            .app_data(web::Data::from(config.clone()))
            .app_data(web::Data::new(pvpc_client.clone()))
            .app_data(web::Data::new(google_auth.clone()))
            .configure(api::configure)
            .route("/health", web::get().to(health_check))
    })
    .bind(&server_addr)?
    .run()
    .await
}

async fn health_check() -> &'static str {
    "OK"
}
