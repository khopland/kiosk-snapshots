use std::env;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tokio::fs;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub bind_addr: SocketAddr,
    pub data_dir: PathBuf,
    pub db_path: PathBuf,
    pub captures_dir: PathBuf,
    pub scheduler_tick_seconds: u64,
    pub capture_concurrency: usize,
    pub default_retention_days: u32,
    pub display_rotation_ms: u64,
    pub chromium_executable: Option<PathBuf>,
}

impl AppConfig {
    pub fn from_env() -> Result<Self> {
        let bind_addr = env::var("APP_BIND")
            .unwrap_or_else(|_| "0.0.0.0:3000".to_string())
            .parse()
            .context("APP_BIND must be a valid socket address")?;

        let data_dir = env::var("APP_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("./data"));
        let db_path = data_dir.join("db").join("app.db");
        let captures_dir = data_dir.join("captures");

        let scheduler_tick_seconds = parse_u64("APP_SCHEDULER_TICK_SECONDS", 1)?;
        let capture_concurrency = parse_usize("APP_CAPTURE_CONCURRENCY", 1)?;
        let default_retention_days = parse_u32("APP_RETENTION_DAYS", 14)?;
        let display_rotation_ms = parse_u64("APP_DISPLAY_ROTATION_MS", 10_000)?;
        let chromium_executable = env::var("APP_CHROMIUM_BIN")
            .ok()
            .map(PathBuf::from)
            .or_else(detect_browser_executable);

        let config = Self {
            bind_addr,
            data_dir,
            db_path,
            captures_dir,
            scheduler_tick_seconds,
            capture_concurrency,
            default_retention_days,
            display_rotation_ms,
            chromium_executable,
        };
        config.validate()?;
        Ok(config)
    }

    pub async fn ensure_dirs(&self) -> Result<()> {
        fs::create_dir_all(&self.data_dir).await?;
        fs::create_dir_all(self.db_dir()).await?;
        fs::create_dir_all(&self.captures_dir).await?;
        Ok(())
    }

    pub fn db_dir(&self) -> &Path {
        self.db_path
            .parent()
            .expect("db path is always nested under a directory")
    }

    fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            self.scheduler_tick_seconds > 0,
            "scheduler tick must be > 0"
        );
        anyhow::ensure!(
            self.capture_concurrency > 0,
            "capture concurrency must be > 0"
        );
        anyhow::ensure!(
            self.default_retention_days > 0,
            "retention days must be > 0"
        );
        anyhow::ensure!(self.display_rotation_ms > 0, "display rotation must be > 0");
        Ok(())
    }
}

fn parse_u64(name: &str, default: u64) -> Result<u64> {
    env::var(name)
        .map(|value| {
            value
                .parse::<u64>()
                .with_context(|| format!("{name} must be an unsigned integer"))
        })
        .unwrap_or(Ok(default))
}

fn parse_u32(name: &str, default: u32) -> Result<u32> {
    env::var(name)
        .map(|value| {
            value
                .parse::<u32>()
                .with_context(|| format!("{name} must be an unsigned integer"))
        })
        .unwrap_or(Ok(default))
}

fn parse_usize(name: &str, default: usize) -> Result<usize> {
    env::var(name)
        .map(|value| {
            value
                .parse::<usize>()
                .with_context(|| format!("{name} must be an unsigned integer"))
        })
        .unwrap_or(Ok(default))
}

fn detect_browser_executable() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let home = env::var_os("HOME").map(PathBuf::from);
        let mut candidates = vec![
            PathBuf::from("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
            PathBuf::from("/Applications/Chromium.app/Contents/MacOS/Chromium"),
            PathBuf::from("/Applications/Brave Browser.app/Contents/MacOS/Brave Browser"),
            PathBuf::from("/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge"),
        ];

        if let Some(home) = home {
            candidates.extend([
                home.join("Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
                home.join("Applications/Chromium.app/Contents/MacOS/Chromium"),
                home.join("Applications/Brave Browser.app/Contents/MacOS/Brave Browser"),
                home.join("Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge"),
            ]);
        }

        return candidates.into_iter().find(|path| path.exists());
    }

    #[cfg(target_os = "linux")]
    {
        let candidates = [
            "/usr/bin/chromium",
            "/usr/bin/chromium-browser",
            "/usr/bin/google-chrome",
            "/snap/bin/chromium",
            "/usr/bin/microsoft-edge",
        ];
        return candidates
            .into_iter()
            .map(PathBuf::from)
            .find(|path| path.exists());
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::AppConfig;

    #[tokio::test]
    async fn config_builds_with_defaults() {
        let config = AppConfig::from_env().expect("config");
        assert!(config.capture_concurrency >= 1);
        assert!(config.default_retention_days >= 1);
        config.ensure_dirs().await.expect("dirs");
    }
}
