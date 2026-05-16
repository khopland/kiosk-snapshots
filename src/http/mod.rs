use askama::Template;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::Serialize;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use tracing::warn;
use uuid::Uuid;

use crate::error::AppError;
use crate::models::{CaptureRecord, CreateScreenRequest, Screen, UpdateScreenRequest};
use crate::state::AppState;

pub fn router(state: AppState) -> Router {
    let captures_dir = state.config.captures_dir.clone();

    Router::new()
        .route("/", get(root))
        .route("/healthz", get(healthz))
        .route("/admin", get(admin_page))
        .route("/display", get(display_page))
        .route(
            "/api/screens",
            get(api_list_screens).post(api_create_screen),
        )
        .route(
            "/api/screens/{id}",
            get(api_get_screen)
                .patch(api_update_screen)
                .delete(api_delete_screen),
        )
        .route("/api/screens/{id}/run", post(api_run_screen))
        .route("/api/screens/{id}/captures", get(api_list_captures))
        .route("/api/screens/{id}/latest", get(api_latest_capture))
        .nest_service("/captures", ServeDir::new(captures_dir))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

async fn root() -> Redirect {
    Redirect::to("/admin")
}

async fn healthz(State(state): State<AppState>) -> impl IntoResponse {
    let degraded = state.browser_health.is_degraded();
    let status = if degraded {
        StatusCode::SERVICE_UNAVAILABLE
    } else {
        StatusCode::OK
    };
    let body = Json(serde_json::json!({
        "status": if degraded { "degraded" } else { "ok" },
        "consecutive_browser_failures": state.browser_health.consecutive_failures(),
    }));
    (status, body)
}

async fn admin_page(State(state): State<AppState>) -> Result<Html<String>, AppError> {
    let screens = state.db.list_screens().await.map_err(AppError::Internal)?;
    let cards = build_screen_cards(&screens, &state).await?;
    render(AdminTemplate {
        screens: cards,
        retention_days: state.config.default_retention_days,
        display_rotation_ms: state.config.display_rotation_ms,
    })
}

async fn display_page(State(state): State<AppState>) -> Result<Html<String>, AppError> {
    let screens = state
        .db
        .list_enabled_screens()
        .await
        .map_err(AppError::Internal)?;
    let cards = build_screen_cards(&screens, &state).await?;
    render(DisplayTemplate {
        screens: cards,
        rotation_ms: state.config.display_rotation_ms,
    })
}

async fn api_list_screens(State(state): State<AppState>) -> Result<Json<Vec<Screen>>, AppError> {
    let screens = state.db.list_screens().await.map_err(AppError::Internal)?;
    Ok(Json(screens))
}

async fn api_create_screen(
    State(state): State<AppState>,
    Json(payload): Json<CreateScreenRequest>,
) -> Result<(StatusCode, Json<Screen>), AppError> {
    let payload = payload.validate()?;
    let screen = state
        .db
        .create_screen(payload)
        .await
        .map_err(AppError::Internal)?;
    Ok((StatusCode::CREATED, Json(screen)))
}

async fn api_get_screen(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Screen>, AppError> {
    let screen = state
        .db
        .get_screen(id)
        .await
        .map_err(AppError::Internal)?
        .ok_or_else(|| AppError::not_found("screen not found"))?;
    Ok(Json(screen))
}

async fn api_update_screen(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(payload): Json<UpdateScreenRequest>,
) -> Result<Json<Screen>, AppError> {
    let current = state
        .db
        .get_screen(id)
        .await
        .map_err(AppError::Internal)?
        .ok_or_else(|| AppError::not_found("screen not found"))?;
    let merged = payload.apply(&current)?;
    let screen = state.db.update_screen(id, merged).await?;
    Ok(Json(screen))
}

async fn api_delete_screen(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    if !state
        .db
        .delete_screen(id)
        .await
        .map_err(AppError::Internal)?
    {
        return Err(AppError::not_found("screen not found"));
    }

    let screen_dir = state.config.captures_dir.join(id.to_string());
    if let Err(error) = tokio::fs::remove_dir_all(&screen_dir).await {
        if error.kind() != std::io::ErrorKind::NotFound {
            warn!(screen_id = %id, error = %error, "failed to remove capture directory");
        }
    }

    Ok(StatusCode::NO_CONTENT)
}

async fn api_run_screen(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<CaptureRecord>, AppError> {
    let screen = state
        .db
        .get_screen(id)
        .await
        .map_err(AppError::Internal)?
        .ok_or_else(|| AppError::not_found("screen not found"))?;

    let _permit = state
        .capture_slots
        .clone()
        .acquire_owned()
        .await
        .map_err(|err| AppError::Internal(err.into()))?;

    let capture = state.capture_service.capture(&screen).await;
    let saved = state
        .db
        .record_capture(&screen, capture)
        .await
        .map_err(AppError::Internal)?;
    Ok(Json(saved))
}

async fn api_list_captures(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<CaptureRecord>>, AppError> {
    let captures = state
        .db
        .list_captures(id)
        .await
        .map_err(AppError::Internal)?;
    Ok(Json(captures))
}

async fn api_latest_capture(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<CaptureRecord>, AppError> {
    let capture = state
        .db
        .latest_capture(id)
        .await
        .map_err(AppError::Internal)?
        .ok_or_else(|| AppError::not_found("capture not found"))?;
    Ok(Json(capture))
}

async fn build_screen_cards(
    screens: &[Screen],
    state: &AppState,
) -> Result<Vec<ScreenCard>, AppError> {
    let mut cards = Vec::with_capacity(screens.len());
    for screen in screens {
        let latest = state
            .db
            .latest_capture(screen.id)
            .await
            .map_err(AppError::Internal)?;
        cards.push(ScreenCard {
            id: screen.id,
            name: screen.name.clone(),
            url: screen.url.clone(),
            enabled: screen.enabled,
            interval_seconds: screen.interval_seconds,
            settle_delay_ms: screen.settle_delay_ms,
            navigation_timeout_ms: screen.navigation_timeout_ms,
            viewport_width: screen.viewport_width,
            viewport_height: screen.viewport_height,
            zoom_factor: screen.zoom_factor,
            scroll_anchor: screen.scroll_anchor.clone(),
            scroll_x: screen.scroll_x,
            scroll_y: screen.scroll_y,
            post_scroll_delay_ms: screen.post_scroll_delay_ms,
            last_capture_at: screen.last_capture_at.map(format_dt),
            next_capture_at: format_dt(screen.next_capture_at),
            last_status: screen
                .last_status
                .clone()
                .unwrap_or_else(|| "never-run".to_string()),
            last_error: screen.last_error.clone(),
            latest_image_url: latest
                .as_ref()
                .and_then(|capture| capture.file_path.as_ref())
                .map(|path| format!("/captures/{path}")),
            latest_capture_path: latest.and_then(|capture| capture.file_path),
        });
    }
    Ok(cards)
}

fn format_dt(value: DateTime<Utc>) -> String {
    value.format("%Y-%m-%d %H:%M:%S UTC").to_string()
}

fn render<T: Template>(template: T) -> Result<Html<String>, AppError> {
    let html = template
        .render()
        .map_err(|err| AppError::Internal(err.into()))?;
    Ok(Html(html))
}

#[derive(Debug, Clone, Serialize)]
struct ScreenCard {
    id: Uuid,
    name: String,
    url: String,
    enabled: bool,
    interval_seconds: i64,
    settle_delay_ms: i64,
    navigation_timeout_ms: i64,
    viewport_width: i64,
    viewport_height: i64,
    zoom_factor: f64,
    scroll_anchor: String,
    scroll_x: i64,
    scroll_y: i64,
    post_scroll_delay_ms: i64,
    last_capture_at: Option<String>,
    next_capture_at: String,
    last_status: String,
    last_error: Option<String>,
    latest_image_url: Option<String>,
    latest_capture_path: Option<String>,
}

#[derive(Template)]
#[template(path = "admin.html")]
struct AdminTemplate {
    screens: Vec<ScreenCard>,
    retention_days: u32,
    display_rotation_ms: u64,
}

#[derive(Template)]
#[template(path = "display.html")]
struct DisplayTemplate {
    screens: Vec<ScreenCard>,
    rotation_ms: u64,
}
