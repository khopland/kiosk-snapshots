use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use tokio::sync::Semaphore;

use crate::capture::CaptureService;
use crate::config::AppConfig;
use crate::db::Database;

#[derive(Debug, Clone)]
pub struct AppState {
    pub config: AppConfig,
    pub db: Database,
    pub capture_service: CaptureService,
    pub browser_health: BrowserHealth,
    pub capture_slots: Arc<Semaphore>,
}

impl AppState {
    pub fn new(
        config: AppConfig,
        db: Database,
        capture_service: CaptureService,
        browser_health: BrowserHealth,
    ) -> Self {
        let capture_slots = Arc::new(Semaphore::new(config.capture_concurrency));
        Self {
            config,
            db,
            capture_service,
            browser_health,
            capture_slots,
        }
    }
}

#[derive(Debug, Default)]
pub struct BrowserHealthInner {
    consecutive_failures: AtomicUsize,
}

#[derive(Debug, Clone, Default)]
pub struct BrowserHealth {
    inner: Arc<BrowserHealthInner>,
}

impl BrowserHealth {
    pub fn record_success(&self) {
        self.inner.consecutive_failures.store(0, Ordering::SeqCst);
    }

    pub fn record_failure(&self) {
        self.inner
            .consecutive_failures
            .fetch_add(1, Ordering::SeqCst);
    }

    pub fn is_degraded(&self) -> bool {
        self.inner.consecutive_failures.load(Ordering::SeqCst) >= 3
    }

    pub fn consecutive_failures(&self) -> usize {
        self.inner.consecutive_failures.load(Ordering::SeqCst)
    }
}
