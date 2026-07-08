use std::sync::Arc;

use auth::Authenticator;
use axum::middleware;
use axum::routing::delete;
use axum::routing::get;
use axum::routing::patch;
use axum::routing::post;
use axum::Router;
use tower_http::cors::CorsLayer;
use web::with_request_trace;

use super::handlers::authenticate::authenticate;
use super::handlers::create_user::create_user;
use super::handlers::health::health;
use super::handlers::delete_user::delete_user;
use super::handlers::get_user::get_user;
use super::handlers::update_user::update_user;
use crate::domain::user::service::Service as UserService;
use crate::outbound::argon2::PasswordHasher;
use crate::outbound::kafka::EventProducer;
use crate::outbound::postgres::UserRepository;

#[derive(Clone)]
pub struct AppState {
    pub user_service: Arc<UserService<UserRepository, EventProducer, PasswordHasher>>,
    pub authenticator: Arc<Authenticator>,
    pub jwt_expiration_hours: i64,
}

pub fn create_router(
    user_service: Arc<UserService<UserRepository, EventProducer, PasswordHasher>>,
    authenticator: Arc<Authenticator>,
    jwt_expiration_hours: i64,
) -> Router {
    let state = AppState {
        user_service,
        authenticator,
        jwt_expiration_hours,
    };

    let public_routes = Router::new()
        .route("/health", get(health))
        .route("/api/auth/login", post(authenticate))
        .route("/api/users", post(create_user));

    let protected_routes = Router::new()
        .route("/api/users/{user_id}", get(get_user))
        .route("/api/users/{user_id}", patch(update_user))
        .route("/api/users/{user_id}", delete(delete_user))
        .route_layer(middleware::from_fn_with_state(
            state.authenticator.clone(),
            web::authenticate,
        ));

    let router = Router::new()
        .merge(public_routes)
        .merge(protected_routes);

    with_request_trace(router)
        .layer(CorsLayer::permissive())
        .with_state(state)
}
