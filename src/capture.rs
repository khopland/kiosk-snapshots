use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Context, Result};
use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;
use chromiumoxide::handler::viewport::Viewport;
use chromiumoxide::page::ScreenshotParams;
use chrono::Utc;
use futures_util::StreamExt;
use tokio::fs;
use tracing::{info, warn};

use crate::config::AppConfig;
use crate::models::{CaptureStatus, CaptureWrite, Screen};
use crate::state::BrowserHealth;

#[derive(Debug, Clone)]
pub struct CaptureService {
    config: AppConfig,
    browser_health: BrowserHealth,
}

impl CaptureService {
    pub fn new(config: AppConfig, browser_health: BrowserHealth) -> Self {
        Self {
            config,
            browser_health,
        }
    }

    pub async fn capture(&self, screen: &Screen) -> CaptureWrite {
        match self.capture_inner(screen).await {
            Ok(capture) => {
                self.browser_health.record_success();
                capture
            }
            Err(error) => {
                self.browser_health.record_failure();
                warn!(screen_id = %screen.id, error = %error, "capture failed");
                CaptureWrite {
                    status: CaptureStatus::Failed,
                    captured_at: Utc::now(),
                    file_path: None,
                    file_size_bytes: None,
                    width: Some(screen.viewport_width),
                    height: Some(screen.viewport_height),
                    duration_ms: 0,
                    error_message: Some(error.to_string()),
                }
            }
        }
    }

    async fn capture_inner(&self, screen: &Screen) -> Result<CaptureWrite> {
        let started = Instant::now();
        let screen_dir = self.config.captures_dir.join(screen.id.to_string());
        let history_dir = screen_dir.join("history");
        fs::create_dir_all(&history_dir).await?;

        let browser_config = self.browser_config(screen)?;
        let (mut browser, mut handler) = Browser::launch(browser_config).await?;
        let handler_task = tokio::spawn(async move {
            while let Some(message) = handler.next().await {
                if let Err(error) = message {
                    warn!(error = %error, "chromium handler error");
                }
            }
        });

        let page = browser.new_page("about:blank").await?;

        tokio::time::timeout(screen.navigation_timeout(), async {
            page.goto(screen.url.as_str()).await?;
            page.wait_for_navigation().await?;
            Ok::<(), anyhow::Error>(())
        })
        .await
        .context("navigation timed out")??;

        tokio::time::sleep(screen.settle_duration()).await;
        self.apply_view_transforms(&page, screen).await?;
        tokio::time::sleep(screen.post_scroll_delay()).await;

        let bytes = page
            .screenshot(
                ScreenshotParams::builder()
                    .format(CaptureScreenshotFormat::Png)
                    .full_page(false)
                    .capture_beyond_viewport(false)
                    .build(),
            )
            .await?;

        let captured_at = Utc::now();
        let capture_id = uuid::Uuid::now_v7();
        let relative_history = format!(
            "{}/history/{}-{}.png",
            screen.id,
            captured_at.format("%Y%m%dT%H%M%SZ"),
            capture_id
        );
        let history_path = self.config.captures_dir.join(&relative_history);
        let latest_relative = format!("{}/latest.png", screen.id);
        let latest_path = self.config.captures_dir.join(&latest_relative);

        fs::write(&history_path, &bytes).await?;
        fs::write(&latest_path, &bytes).await?;

        let _ = browser.close().await;
        handler_task.abort();

        let duration_ms = started.elapsed().as_millis() as i64;
        info!(screen_id = %screen.id, duration_ms, "capture saved");

        Ok(CaptureWrite {
            status: CaptureStatus::Success,
            captured_at,
            file_path: Some(relative_history),
            file_size_bytes: Some(bytes.len() as i64),
            width: Some(screen.viewport_width),
            height: Some(screen.viewport_height),
            duration_ms,
            error_message: None,
        })
    }

    fn browser_config(&self, screen: &Screen) -> Result<BrowserConfig> {
        let viewport = Viewport {
            width: screen.viewport_width.max(1) as u32,
            height: screen.viewport_height.max(1) as u32,
            device_scale_factor: Some(1.0),
            emulating_mobile: false,
            is_landscape: screen.viewport_width >= screen.viewport_height,
            has_touch: false,
        };

        let mut builder = BrowserConfig::builder()
            .with_head()
            .no_sandbox()
            .disable_https_first()
            .viewport(viewport)
            .args([
                "--disable-gpu",
                "--disable-dev-shm-usage",
                "--hide-scrollbars",
                "--disable-background-timer-throttling",
                "--disable-renderer-backgrounding",
            ]);

        if let Some(path) = &self.config.chromium_executable {
            builder = builder.chrome_executable(path);
        }

        builder
            .build()
            .map_err(|error| anyhow::anyhow!("build chromium config: {error}"))
    }

    async fn apply_view_transforms(
        &self,
        page: &chromiumoxide::Page,
        screen: &Screen,
    ) -> Result<()> {
        let script = format!(
            r#"
                (() => {{
                    const zoom = {zoom};
                    const anchor = "{anchor}";
                    const viewportHeight = window.innerHeight;
                    const scrollHeight = Math.max(
                      document.documentElement ? document.documentElement.scrollHeight : 0,
                      document.body ? document.body.scrollHeight : 0
                    );
                    const html = document.documentElement;
                    const body = document.body;
                    if (html) {{
                        html.style.zoom = String(zoom);
                        html.style.transformOrigin = 'top left';
                    }}
                    if (body) {{
                        body.style.zoom = String(zoom);
                        body.style.transformOrigin = 'top left';
                    }}
                    let y = {scroll_y};
                    if (anchor === "top") {{
                        y = 0;
                    }} else if (anchor === "middle") {{
                        y = Math.max(0, Math.round((scrollHeight - viewportHeight) / 2));
                    }} else if (anchor === "bottom") {{
                        y = Math.max(0, scrollHeight - viewportHeight);
                    }}
                    window.scrollTo({scroll_x}, y);
                    return {{ scrollY: window.scrollY, innerHeight: viewportHeight, scrollHeight }};
                }})()
            "#,
            zoom = screen.zoom_factor,
            anchor = screen.scroll_anchor().as_str(),
            scroll_x = screen.scroll_x,
            scroll_y = screen.scroll_y,
        );

        let _: serde_json::Value = page.evaluate(script).await?.into_value()?;
        Ok(())
    }
}

#[allow(dead_code)]
fn _latest_path_for(screen_dir: PathBuf) -> PathBuf {
    screen_dir.join("latest.png")
}

#[cfg(test)]
mod tests {
    use axum::Router;
    use axum::response::Html;
    use axum::routing::get;
    use chrono::Utc;
    use image::GenericImageView;
    use serial_test::serial;
    use tempfile::tempdir;
    use tokio::net::TcpListener;
    use uuid::Uuid;

    use crate::config::AppConfig;
    use crate::models::ScrollAnchor;

    use super::*;

    #[tokio::test]
    #[serial]
    async fn smoke_capture_writes_screenshot_file() -> anyhow::Result<()> {
        let mut config = AppConfig::from_env()?;
        if config.chromium_executable.is_none() {
            eprintln!("Skipping smoke test because no Chromium-based browser was detected");
            return Ok(());
        }

        let temp = tempdir()?;
        config.data_dir = temp.path().join("data");
        config.db_path = config.data_dir.join("db").join("app.db");
        config.captures_dir = config.data_dir.join("captures");
        config.ensure_dirs().await?;

        let app = Router::new().route("/", get(smoke_page));
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("smoke server");
        });

        let screen = Screen {
            id: Uuid::now_v7(),
            name: "Smoke".into(),
            url: format!("http://{addr}/"),
            enabled: true,
            interval_seconds: 60,
            settle_delay_ms: 300,
            navigation_timeout_ms: 5_000,
            viewport_width: 1280,
            viewport_height: 720,
            zoom_factor: 1.0,
            scroll_anchor: ScrollAnchor::Custom.as_str().into(),
            scroll_x: 0,
            scroll_y: 0,
            post_scroll_delay_ms: 100,
            full_page: false,
            last_capture_at: None,
            next_capture_at: Utc::now(),
            last_status: None,
            last_error: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let capture = CaptureService::new(config.clone(), BrowserHealth::default())
            .capture(&screen)
            .await;

        server.abort();

        assert!(matches!(capture.status, CaptureStatus::Success));
        let relative_path = capture
            .file_path
            .as_ref()
            .expect("capture should include file path");
        let history_path = config.captures_dir.join(relative_path);
        let latest_path = config
            .captures_dir
            .join(screen.id.to_string())
            .join("latest.png");

        assert!(history_path.exists(), "history capture missing");
        assert!(latest_path.exists(), "latest capture missing");
        assert!(capture.file_size_bytes.unwrap_or_default() > 0);

        Ok(())
    }

    #[tokio::test]
    #[serial]
    async fn smoke_capture_applies_zoom_and_scroll_anchor() -> anyhow::Result<()> {
        let mut config = AppConfig::from_env()?;
        if config.chromium_executable.is_none() {
            eprintln!(
                "Skipping smoke transform test because no Chromium-based browser was detected"
            );
            return Ok(());
        }

        let temp = tempdir()?;
        config.data_dir = temp.path().join("data");
        config.db_path = config.data_dir.join("db").join("app.db");
        config.captures_dir = config.data_dir.join("captures");
        config.ensure_dirs().await?;

        let app = Router::new().route("/transform", get(transform_page));
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("transform server");
        });

        let base_screen = Screen {
            id: Uuid::now_v7(),
            name: "Transform".into(),
            url: format!("http://{addr}/transform"),
            enabled: true,
            interval_seconds: 60,
            settle_delay_ms: 200,
            navigation_timeout_ms: 5_000,
            viewport_width: 1000,
            viewport_height: 600,
            zoom_factor: 1.0,
            scroll_anchor: ScrollAnchor::Top.as_str().into(),
            scroll_x: 0,
            scroll_y: 0,
            post_scroll_delay_ms: 100,
            full_page: false,
            last_capture_at: None,
            next_capture_at: Utc::now(),
            last_status: None,
            last_error: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let bottom_screen = Screen {
            id: Uuid::now_v7(),
            scroll_anchor: ScrollAnchor::Bottom.as_str().into(),
            ..base_screen.clone()
        };
        let zoomed_screen = Screen {
            id: Uuid::now_v7(),
            zoom_factor: 2.0,
            scroll_anchor: ScrollAnchor::Top.as_str().into(),
            ..base_screen.clone()
        };

        let service = CaptureService::new(config.clone(), BrowserHealth::default());
        let base = service.capture(&base_screen).await;
        let bottom = service.capture(&bottom_screen).await;
        let zoomed = service.capture(&zoomed_screen).await;

        server.abort();

        assert!(matches!(base.status, CaptureStatus::Success));
        assert!(matches!(bottom.status, CaptureStatus::Success));
        assert!(matches!(zoomed.status, CaptureStatus::Success));

        let base_path = config.captures_dir.join(base.file_path.expect("base path"));
        let bottom_path = config
            .captures_dir
            .join(bottom.file_path.expect("bottom path"));
        let zoomed_path = config
            .captures_dir
            .join(zoomed.file_path.expect("zoomed path"));
        let base_image = image::open(base_path)?;
        let bottom_image = image::open(bottom_path)?;
        let zoomed_image = image::open(zoomed_path)?;

        let base_center = base_image
            .get_pixel(base_image.width() / 2, base_image.height() / 2)
            .0;
        let bottom_center = bottom_image
            .get_pixel(bottom_image.width() / 2, bottom_image.height() / 2)
            .0;

        assert!(
            base_center[0] > 180 && base_center[1] < 120,
            "top capture should show red section"
        );
        assert!(
            bottom_center[2] > 180 && bottom_center[0] < 120,
            "bottom capture should show blue section"
        );

        let base_zoom_probe = base_image.get_pixel(780, 100).0;
        let zoomed_probe = zoomed_image.get_pixel(780, 100).0;
        assert!(
            base_zoom_probe[1] > 180,
            "base probe should be inside green block"
        );
        assert!(
            zoomed_probe[1] < 120,
            "zoomed capture should move green block away from probe"
        );

        Ok(())
    }

    async fn smoke_page() -> Html<&'static str> {
        Html(
            r#"<!doctype html>
            <html>
              <head>
                <meta charset="utf-8">
                <style>
                  html, body { margin: 0; width: 100%; height: 100%; font-family: sans-serif; background: #f2f0e8; }
                  .frame { display: grid; place-items: center; width: 100%; height: 100%; }
                  .card { width: 80vw; padding: 48px; border-radius: 24px; background: linear-gradient(135deg, #1f4f46, #e36f3d); color: white; }
                </style>
              </head>
              <body>
                <div class="frame">
                  <div class="card" id="card">loading</div>
                </div>
                <script>
                  window.setTimeout(() => {
                    document.getElementById('card').textContent = 'smoke-test-ready';
                  }, 100);
                </script>
              </body>
            </html>"#,
        )
    }

    async fn transform_page() -> Html<&'static str> {
        Html(
            r#"<!doctype html>
            <html>
              <head>
                <meta charset="utf-8">
                <style>
                  html, body { margin: 0; padding: 0; }
                  body { background: rgb(245, 245, 245); }
                  .band { width: 100%; height: 700px; position: relative; }
                  .top { background: rgb(220, 60, 60); }
                  .mid { background: rgb(230, 215, 120); }
                  .bottom { background: rgb(60, 80, 220); }
                  .probe {
                    position: absolute;
                    top: 80px;
                    left: 720px;
                    width: 180px;
                    height: 180px;
                    background: rgb(40, 220, 60);
                  }
                </style>
              </head>
              <body>
                <section class="band top"><div class="probe"></div></section>
                <section class="band mid"></section>
                <section class="band bottom"></section>
              </body>
            </html>"#,
        )
    }
}
