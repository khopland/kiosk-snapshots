use std::path::PathBuf;
use std::time::Duration;

use tracing::{error, info, warn};

use crate::state::AppState;

pub fn spawn_workers(state: AppState) {
    let scheduler_state = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(
            scheduler_state.config.scheduler_tick_seconds,
        ));
        loop {
            interval.tick().await;
            let Ok(permit) = scheduler_state.capture_slots.clone().try_acquire_owned() else {
                continue;
            };

            match scheduler_state.db.due_screens(chrono::Utc::now(), 1).await {
                Ok(mut screens) => {
                    if let Some(screen) = screens.pop() {
                        let state = scheduler_state.clone();
                        tokio::spawn(async move {
                            let _permit = permit;
                            let capture = state.capture_service.capture(&screen).await;
                            if let Err(error) = state.db.record_capture(&screen, capture).await {
                                error!(screen_id = %screen.id, error = %error, "failed to persist scheduled capture");
                            }
                        });
                    }
                }
                Err(error) => {
                    warn!(error = %error, "failed to query due screens");
                }
            }
        }
    });

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(24 * 60 * 60));
        interval.tick().await;
        loop {
            interval.tick().await;
            if let Err(error) = prune_history(&state).await {
                warn!(error = %error, "retention cleanup failed");
            }
        }
    });
}

async fn prune_history(state: &AppState) -> anyhow::Result<()> {
    let old_paths = state
        .db
        .old_capture_paths(state.config.default_retention_days)
        .await?;
    let deleted_rows = state
        .db
        .prune_old_captures(state.config.default_retention_days)
        .await?;

    for path in old_paths {
        let full_path: PathBuf = state.config.captures_dir.join(path);
        match tokio::fs::remove_file(&full_path).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                warn!(path = %full_path.display(), error = %error, "failed to remove old capture file")
            }
        }
    }

    if deleted_rows > 0 {
        info!(deleted_rows, "pruned old captures");
    }

    Ok(())
}
