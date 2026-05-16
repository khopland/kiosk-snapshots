use std::str::FromStr;

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{SqlitePool, query, query_as};
use uuid::Uuid;

use crate::config::AppConfig;
use crate::error::AppError;
use crate::models::{CaptureRecord, CaptureWrite, CreateScreenRequest, Screen, next_run_from};

#[derive(Debug, Clone)]
pub struct Database {
    pool: SqlitePool,
}

impl Database {
    pub async fn connect(config: &AppConfig) -> Result<Self> {
        let options =
            SqliteConnectOptions::from_str(&format!("sqlite://{}", config.db_path.display()))?
                .create_if_missing(true)
                .foreign_keys(true)
                .journal_mode(SqliteJournalMode::Wal)
                .synchronous(SqliteSynchronous::Normal);

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await?;

        sqlx::migrate!("./migrations").run(&pool).await?;

        Ok(Self { pool })
    }

    pub async fn list_screens(&self) -> Result<Vec<Screen>> {
        query_as::<_, Screen>("SELECT * FROM screens ORDER BY created_at DESC")
            .fetch_all(&self.pool)
            .await
            .context("list screens")
    }

    pub async fn list_enabled_screens(&self) -> Result<Vec<Screen>> {
        query_as::<_, Screen>("SELECT * FROM screens WHERE enabled = 1 ORDER BY name ASC")
            .fetch_all(&self.pool)
            .await
            .context("list enabled screens")
    }

    pub async fn due_screens(&self, now: DateTime<Utc>, limit: i64) -> Result<Vec<Screen>> {
        query_as::<_, Screen>(
            "SELECT * FROM screens
             WHERE enabled = 1 AND next_capture_at <= ?
             ORDER BY next_capture_at ASC
             LIMIT ?",
        )
        .bind(now)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .context("load due screens")
    }

    pub async fn get_screen(&self, id: Uuid) -> Result<Option<Screen>> {
        query_as::<_, Screen>("SELECT * FROM screens WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .context("get screen")
    }

    pub async fn create_screen(&self, input: CreateScreenRequest) -> Result<Screen> {
        let now = Utc::now();
        let id = Uuid::now_v7();
        let next_capture_at = next_run_from(now, input.interval_seconds as i64);

        query(
            "INSERT INTO screens (
                id, name, url, enabled, interval_seconds, settle_delay_ms,
                navigation_timeout_ms, viewport_width, viewport_height,
                zoom_factor, scroll_anchor, scroll_x, scroll_y, post_scroll_delay_ms,
                full_page, last_capture_at, next_capture_at, last_status,
                last_error, created_at, updated_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(&input.name)
        .bind(&input.url)
        .bind(input.enabled)
        .bind(input.interval_seconds as i64)
        .bind(input.settle_delay_ms as i64)
        .bind(input.navigation_timeout_ms as i64)
        .bind(input.viewport_width as i64)
        .bind(input.viewport_height as i64)
        .bind(input.zoom_factor)
        .bind(input.scroll_anchor.as_str())
        .bind(input.scroll_x)
        .bind(input.scroll_y)
        .bind(input.post_scroll_delay_ms as i64)
        .bind(false)
        .bind(Option::<DateTime<Utc>>::None)
        .bind(next_capture_at)
        .bind(Option::<String>::None)
        .bind(Option::<String>::None)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await
        .context("create screen")?;

        self.get_screen(id).await?.context("created screen missing")
    }

    pub async fn update_screen(
        &self,
        id: Uuid,
        input: CreateScreenRequest,
    ) -> Result<Screen, AppError> {
        let now = Utc::now();
        let next_capture_at = next_run_from(now, input.interval_seconds as i64);
        let result = query(
            "UPDATE screens
             SET name = ?, url = ?, enabled = ?, interval_seconds = ?, settle_delay_ms = ?,
                 navigation_timeout_ms = ?, viewport_width = ?, viewport_height = ?,
                 zoom_factor = ?, scroll_anchor = ?, scroll_x = ?, scroll_y = ?, post_scroll_delay_ms = ?,
                 full_page = ?, next_capture_at = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(&input.name)
        .bind(&input.url)
        .bind(input.enabled)
        .bind(input.interval_seconds as i64)
        .bind(input.settle_delay_ms as i64)
        .bind(input.navigation_timeout_ms as i64)
        .bind(input.viewport_width as i64)
        .bind(input.viewport_height as i64)
        .bind(input.zoom_factor)
        .bind(input.scroll_anchor.as_str())
        .bind(input.scroll_x)
        .bind(input.scroll_y)
        .bind(input.post_scroll_delay_ms as i64)
        .bind(false)
        .bind(next_capture_at)
        .bind(now)
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(anyhow::Error::from)
        .map_err(AppError::Internal)?;

        if result.rows_affected() == 0 {
            return Err(AppError::not_found("screen not found"));
        }

        self.get_screen(id)
            .await
            .map_err(AppError::Internal)?
            .ok_or_else(|| AppError::not_found("screen not found"))
    }

    pub async fn delete_screen(&self, id: Uuid) -> Result<bool> {
        let result = query("DELETE FROM screens WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .context("delete screen")?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn list_captures(&self, screen_id: Uuid) -> Result<Vec<CaptureRecord>> {
        query_as::<_, CaptureRecord>(
            "SELECT * FROM captures WHERE screen_id = ? ORDER BY captured_at DESC LIMIT 100",
        )
        .bind(screen_id)
        .fetch_all(&self.pool)
        .await
        .context("list captures")
    }

    pub async fn latest_capture(&self, screen_id: Uuid) -> Result<Option<CaptureRecord>> {
        query_as::<_, CaptureRecord>(
            "SELECT * FROM captures WHERE screen_id = ? ORDER BY captured_at DESC LIMIT 1",
        )
        .bind(screen_id)
        .fetch_optional(&self.pool)
        .await
        .context("latest capture")
    }

    pub async fn record_capture(
        &self,
        screen: &Screen,
        capture: CaptureWrite,
    ) -> Result<CaptureRecord> {
        let capture_id = Uuid::now_v7();
        let next_capture_at = next_run_from(capture.captured_at, screen.interval_seconds);

        let mut tx = self.pool.begin().await?;

        query(
            "INSERT INTO captures (
                id, screen_id, captured_at, status, file_path, file_size_bytes,
                width, height, duration_ms, error_message
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(capture_id)
        .bind(screen.id)
        .bind(capture.captured_at)
        .bind(capture.status.as_str())
        .bind(&capture.file_path)
        .bind(capture.file_size_bytes)
        .bind(capture.width)
        .bind(capture.height)
        .bind(capture.duration_ms)
        .bind(&capture.error_message)
        .execute(&mut *tx)
        .await?;

        query(
            "UPDATE screens
             SET last_capture_at = ?, next_capture_at = ?, last_status = ?, last_error = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(capture.captured_at)
        .bind(next_capture_at)
        .bind(capture.status.as_str())
        .bind(&capture.error_message)
        .bind(capture.captured_at)
        .bind(screen.id)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        query_as::<_, CaptureRecord>("SELECT * FROM captures WHERE id = ?")
            .bind(capture_id)
            .fetch_one(&self.pool)
            .await
            .context("fetch saved capture")
    }

    pub async fn old_capture_paths(&self, retention_days: u32) -> Result<Vec<String>> {
        let cutoff = Utc::now() - Duration::days(retention_days as i64);
        let rows = query_scalar_strings(
            &self.pool,
            "SELECT file_path FROM captures
             WHERE file_path IS NOT NULL AND captured_at < ?",
            cutoff,
        )
        .await?;
        Ok(rows)
    }

    pub async fn prune_old_captures(&self, retention_days: u32) -> Result<u64> {
        let cutoff = Utc::now() - Duration::days(retention_days as i64);
        let result = query("DELETE FROM captures WHERE captured_at < ?")
            .bind(cutoff)
            .execute(&self.pool)
            .await
            .context("prune captures")?;
        Ok(result.rows_affected())
    }
}

async fn query_scalar_strings(
    pool: &SqlitePool,
    statement: &str,
    cutoff: DateTime<Utc>,
) -> Result<Vec<String>> {
    let rows: Vec<(String,)> = sqlx::query_as(statement)
        .bind(cutoff)
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().map(|tuple| tuple.0).collect())
}
