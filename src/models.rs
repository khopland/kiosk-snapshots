use std::time::Duration;

use anyhow::Result;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use url::Url;
use uuid::Uuid;

use crate::error::AppError;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScrollAnchor {
    Custom,
    Top,
    Middle,
    Bottom,
}

impl ScrollAnchor {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Custom => "custom",
            Self::Top => "top",
            Self::Middle => "middle",
            Self::Bottom => "bottom",
        }
    }
}

impl std::str::FromStr for ScrollAnchor {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "custom" => Ok(Self::Custom),
            "top" => Ok(Self::Top),
            "middle" => Ok(Self::Middle),
            "bottom" => Ok(Self::Bottom),
            _ => Err(AppError::bad_request(
                "scroll_anchor must be custom, top, middle, or bottom",
            )),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Screen {
    pub id: Uuid,
    pub name: String,
    pub url: String,
    pub enabled: bool,
    pub interval_seconds: i64,
    pub settle_delay_ms: i64,
    pub navigation_timeout_ms: i64,
    pub viewport_width: i64,
    pub viewport_height: i64,
    pub zoom_factor: f64,
    pub scroll_anchor: String,
    pub scroll_x: i64,
    pub scroll_y: i64,
    pub post_scroll_delay_ms: i64,
    pub full_page: bool,
    pub last_capture_at: Option<DateTime<Utc>>,
    pub next_capture_at: DateTime<Utc>,
    pub last_status: Option<String>,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct CaptureRecord {
    pub id: Uuid,
    pub screen_id: Uuid,
    pub captured_at: DateTime<Utc>,
    pub status: String,
    pub file_path: Option<String>,
    pub file_size_bytes: Option<i64>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub duration_ms: i64,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateScreenRequest {
    pub name: String,
    pub url: String,
    pub enabled: bool,
    pub interval_seconds: u64,
    pub settle_delay_ms: u64,
    pub navigation_timeout_ms: u64,
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub zoom_factor: f64,
    pub scroll_anchor: ScrollAnchor,
    pub scroll_x: i64,
    pub scroll_y: i64,
    pub post_scroll_delay_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UpdateScreenRequest {
    pub name: Option<String>,
    pub url: Option<String>,
    pub enabled: Option<bool>,
    pub interval_seconds: Option<u64>,
    pub settle_delay_ms: Option<u64>,
    pub navigation_timeout_ms: Option<u64>,
    pub viewport_width: Option<u32>,
    pub viewport_height: Option<u32>,
    pub zoom_factor: Option<f64>,
    pub scroll_anchor: Option<ScrollAnchor>,
    pub scroll_x: Option<i64>,
    pub scroll_y: Option<i64>,
    pub post_scroll_delay_ms: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct CaptureWrite {
    pub status: CaptureStatus,
    pub captured_at: DateTime<Utc>,
    pub file_path: Option<String>,
    pub file_size_bytes: Option<i64>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub duration_ms: i64,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureStatus {
    Success,
    Failed,
}

impl CaptureStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failed => "failed",
        }
    }
}

impl Screen {
    pub fn settle_duration(&self) -> Duration {
        Duration::from_millis(self.settle_delay_ms.max(0) as u64)
    }

    pub fn navigation_timeout(&self) -> Duration {
        Duration::from_millis(self.navigation_timeout_ms.max(1) as u64)
    }

    pub fn post_scroll_delay(&self) -> Duration {
        Duration::from_millis(self.post_scroll_delay_ms.max(0) as u64)
    }

    pub fn scroll_anchor(&self) -> ScrollAnchor {
        self.scroll_anchor.parse().unwrap_or(ScrollAnchor::Custom)
    }
}

impl CreateScreenRequest {
    pub fn validate(self) -> Result<Self, AppError> {
        validate_screen_input(
            &self.name,
            &self.url,
            self.interval_seconds,
            self.settle_delay_ms,
            self.navigation_timeout_ms,
            self.viewport_width,
            self.viewport_height,
            self.zoom_factor,
            self.scroll_anchor,
            self.scroll_x,
            self.scroll_y,
            self.post_scroll_delay_ms,
        )?;
        Ok(self)
    }
}

impl UpdateScreenRequest {
    pub fn apply(self, current: &Screen) -> Result<CreateScreenRequest, AppError> {
        let merged = CreateScreenRequest {
            name: self.name.unwrap_or_else(|| current.name.clone()),
            url: self.url.unwrap_or_else(|| current.url.clone()),
            enabled: self.enabled.unwrap_or(current.enabled),
            interval_seconds: self
                .interval_seconds
                .unwrap_or(current.interval_seconds.max(1) as u64),
            settle_delay_ms: self
                .settle_delay_ms
                .unwrap_or(current.settle_delay_ms.max(0) as u64),
            navigation_timeout_ms: self
                .navigation_timeout_ms
                .unwrap_or(current.navigation_timeout_ms.max(1) as u64),
            viewport_width: self
                .viewport_width
                .unwrap_or(current.viewport_width.max(1) as u32),
            viewport_height: self
                .viewport_height
                .unwrap_or(current.viewport_height.max(1) as u32),
            zoom_factor: self.zoom_factor.unwrap_or(current.zoom_factor),
            scroll_anchor: self
                .scroll_anchor
                .unwrap_or_else(|| current.scroll_anchor()),
            scroll_x: self.scroll_x.unwrap_or(current.scroll_x),
            scroll_y: self.scroll_y.unwrap_or(current.scroll_y),
            post_scroll_delay_ms: self
                .post_scroll_delay_ms
                .unwrap_or(current.post_scroll_delay_ms.max(0) as u64),
        };
        merged.validate()
    }
}

fn validate_screen_input(
    name: &str,
    url: &str,
    interval_seconds: u64,
    settle_delay_ms: u64,
    navigation_timeout_ms: u64,
    viewport_width: u32,
    viewport_height: u32,
    zoom_factor: f64,
    scroll_anchor: ScrollAnchor,
    scroll_x: i64,
    scroll_y: i64,
    post_scroll_delay_ms: u64,
) -> Result<(), AppError> {
    if name.trim().is_empty() {
        return Err(AppError::bad_request("name must not be empty"));
    }
    let parsed =
        Url::parse(url).map_err(|err| AppError::bad_request(format!("invalid url: {err}")))?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err(AppError::bad_request("url must use http or https"));
    }
    if interval_seconds == 0 {
        return Err(AppError::bad_request("interval_seconds must be > 0"));
    }
    if navigation_timeout_ms == 0 {
        return Err(AppError::bad_request("navigation_timeout_ms must be > 0"));
    }
    if viewport_width == 0 || viewport_height == 0 {
        return Err(AppError::bad_request("viewport size must be > 0"));
    }
    if !(0.1..=5.0).contains(&zoom_factor) {
        return Err(AppError::bad_request(
            "zoom_factor must be between 0.1 and 5.0",
        ));
    }
    let _ = scroll_anchor;
    if scroll_x.unsigned_abs() > 100_000 || scroll_y.unsigned_abs() > 100_000 {
        return Err(AppError::bad_request(
            "scroll values are unrealistically large",
        ));
    }
    if settle_delay_ms > 60 * 60 * 1000 {
        return Err(AppError::bad_request(
            "settle_delay_ms is unrealistically large",
        ));
    }
    if post_scroll_delay_ms > 60 * 60 * 1000 {
        return Err(AppError::bad_request(
            "post_scroll_delay_ms is unrealistically large",
        ));
    }
    Ok(())
}

pub fn next_run_from(now: DateTime<Utc>, interval_seconds: i64) -> DateTime<Utc> {
    now + ChronoDuration::seconds(interval_seconds.max(1))
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::{CreateScreenRequest, ScrollAnchor, next_run_from};

    #[test]
    fn next_run_moves_forward() {
        let now = Utc::now();
        let next = next_run_from(now, 15);
        assert!(next > now);
    }

    #[test]
    fn create_request_validates_url() {
        let request = CreateScreenRequest {
            name: "A".into(),
            url: "ftp://example.com".into(),
            enabled: true,
            interval_seconds: 60,
            settle_delay_ms: 5000,
            navigation_timeout_ms: 10000,
            viewport_width: 1920,
            viewport_height: 1080,
            zoom_factor: 1.0,
            scroll_anchor: ScrollAnchor::Custom,
            scroll_x: 0,
            scroll_y: 0,
            post_scroll_delay_ms: 500,
        };
        assert!(request.validate().is_err());
    }
}
