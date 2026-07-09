use std::sync::Arc;

use auth::Authenticator;
use axum::Router;
use axum::middleware;
use axum::routing::delete;
use axum::routing::get;
use axum::routing::patch;
use axum::routing::post;
use tower_http::cors::CorsLayer;
use web::with_request_trace;

use super::handlers::authenticate::authenticate;
use super::handlers::create_user::create_user;
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
        .merge(protected_routes)
        // route_layer, not layer: it must wrap already-matched routes so
        // `track_http_metrics` sees the `MatchedPath` extension.
        .route_layer(middleware::from_fn(web::metrics::track_http_metrics));

    with_request_trace(router)
        .layer(CorsLayer::permissive())
        .with_state(state)
}
