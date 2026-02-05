use std::sync::Arc;

use askama::Template;
use axum::{
    extract::State,
    response::{Html, IntoResponse, Json},
    routing::get,
    Router,
};
use rusqlite::Connection;
use std::sync::Mutex;

use crate::data::{get_daily, get_hourly, get_weekly, AggregatedTiming};

pub struct AppState {
    pub db: Mutex<Connection>,
    pub network: String,
}

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate {
    network: String,
}

pub fn create_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/api/hourly", get(api_hourly))
        .route("/api/daily", get(api_daily))
        .route("/api/weekly", get(api_weekly))
        .with_state(state)
}

async fn index(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let template = IndexTemplate {
        network: state.network.clone(),
    };
    Html(template.render().unwrap_or_default())
}

async fn api_hourly(State(state): State<Arc<AppState>>) -> Json<Vec<AggregatedTiming>> {
    let db = state.db.lock().unwrap();
    Json(get_hourly(&db).unwrap_or_default())
}

async fn api_daily(State(state): State<Arc<AppState>>) -> Json<Vec<AggregatedTiming>> {
    let db = state.db.lock().unwrap();
    Json(get_daily(&db).unwrap_or_default())
}

async fn api_weekly(State(state): State<Arc<AppState>>) -> Json<Vec<AggregatedTiming>> {
    let db = state.db.lock().unwrap();
    Json(get_weekly(&db).unwrap_or_default())
}

pub async fn serve(state: Arc<AppState>, port: u16) -> anyhow::Result<()> {
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    log::info!("Web server listening on http://0.0.0.0:{}", port);
    axum::serve(listener, app).await?;
    Ok(())
}
