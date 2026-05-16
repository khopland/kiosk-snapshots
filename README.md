# Screenshot Pi

Rust screenshot appliance for dashboards and SPA pages. It schedules captures, stores the latest screenshot plus history, exposes `/admin` for management, and `/display` for TV kiosk mode.

## Run on macOS

### 1. Install a Chromium-based browser

Any of these work:

- Google Chrome
- Chromium
- Brave
- Microsoft Edge

The app will auto-detect the browser in normal macOS app bundle locations. If your browser lives somewhere unusual, set:

```bash
export APP_CHROMIUM_BIN="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
```

### 2. Start the server

```bash
cargo run
```

Then open:

- `http://127.0.0.1:3000/admin`
- `http://127.0.0.1:3000/display`

### 3. Optional local settings

```bash
export APP_BIND=127.0.0.1:3000
export APP_DATA_DIR=./data
export RUST_LOG=info
```

## Run in Docker

```bash
docker compose -f docker/docker-compose.yml up --build
```

Then open `http://127.0.0.1:3000/admin`.

## Raspberry Pi / TV mode

- Run the Rust app in the container
- Run Chromium on the Pi host in kiosk mode against `http://127.0.0.1:3000/display`
- Example service: [docker/kiosk-browser.service](/Users/kristoffer/dev/screenshot-pi/docker/kiosk-browser.service)

## Notes

- First capture requires a configured screen in `/admin`.
- Screenshot files are stored in `./data/captures` locally or `/data/captures` in the container.
- SQLite lives in `./data/db/app.db` locally or `/data/db/app.db` in the container.
