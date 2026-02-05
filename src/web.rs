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

use crate::data::{get_timings, TxTiming};

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
        .route("/api/timings", get(api_timings))
        .with_state(state)
}

async fn index(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let template = IndexTemplate {
        network: state.network.clone(),
    };
    Html(template.render().unwrap_or_default())
}

async fn api_timings(State(state): State<Arc<AppState>>) -> Json<Vec<TxTiming>> {
    let db = state.db.lock().unwrap();
    let timings = get_timings(&db, Some(100)).unwrap_or_default();
    Json(timings)
}

pub async fn serve(state: Arc<AppState>, port: u16) -> anyhow::Result<()> {
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    log::info!("Web server listening on http://0.0.0.0:{}", port);
    axum::serve(listener, app).await?;
    Ok(())
}
