pub mod auth;
pub mod devices;
pub mod prices;
pub mod rules;
pub mod schedule;

use actix_web::web;

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api")
            .configure(auth::configure)
            .configure(devices::configure)
            .configure(rules::configure)
            .configure(prices::configure)
            .configure(schedule::configure),
    );
}
