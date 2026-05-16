CREATE TABLE IF NOT EXISTS screens (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    url TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    interval_seconds INTEGER NOT NULL,
    settle_delay_ms INTEGER NOT NULL,
    navigation_timeout_ms INTEGER NOT NULL,
    viewport_width INTEGER NOT NULL,
    viewport_height INTEGER NOT NULL,
    zoom_factor REAL NOT NULL DEFAULT 1.0,
    scroll_anchor TEXT NOT NULL DEFAULT 'custom',
    scroll_x INTEGER NOT NULL DEFAULT 0,
    scroll_y INTEGER NOT NULL DEFAULT 0,
    post_scroll_delay_ms INTEGER NOT NULL DEFAULT 500,
    full_page INTEGER NOT NULL DEFAULT 1,
    last_capture_at TEXT NULL,
    next_capture_at TEXT NOT NULL,
    last_status TEXT NULL,
    last_error TEXT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS captures (
    id TEXT PRIMARY KEY NOT NULL,
    screen_id TEXT NOT NULL,
    captured_at TEXT NOT NULL,
    status TEXT NOT NULL,
    file_path TEXT NULL,
    file_size_bytes INTEGER NULL,
    width INTEGER NULL,
    height INTEGER NULL,
    duration_ms INTEGER NOT NULL,
    error_message TEXT NULL,
    FOREIGN KEY(screen_id) REFERENCES screens(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_screens_due
    ON screens(enabled, next_capture_at);

CREATE INDEX IF NOT EXISTS idx_captures_screen_captured_at
    ON captures(screen_id, captured_at DESC);
