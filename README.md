# SlotLab

Unified dev/QA tool for Stake Engine slots — replaces **stake-dev-tool**
(local RGS + test view) and **Mnemoo tools** (math analysis) with a single app,
plus the play↔analysis link and public-tunnel sharing.

## Desktop app (recommended for non-devs)

SlotLab installs like a regular macOS app: download the `.dmg` from the
GitHub **Releases** page, open it, drag **SlotLab** into Applications,
double-click. A first-run guide walks through installing the local
certificate and adding the first game.

The app (`crates/slotlab-desktop`, Tauri 2) starts the server in-process
(same ports :3600/:3601) and shows the UI in a native window. If a `slotlab`
CLI instance is already running, the app attaches to it instead of starting
a second one.

```bash
# Local build (the UI is embedded at compile time → build it first):
pnpm --dir frontend install && pnpm --dir frontend build
cd crates/slotlab-desktop && pnpm dlx @tauri-apps/cli build
# → target/release/bundle/macos/SlotLab.app (and the .dmg without --bundles app)
```

**Public release**: push a `v*` tag → `.github/workflows/release.yml` builds
a **universal** binary (Apple Silicon + Intel), signs and notarizes it with
the Developer ID certificate (same secrets as the SSP pipeline, listed at
the top of the workflow) and publishes the `.dmg` as a draft GitHub release.
⚠️ The workflow is only active once `slotlab/` is the root of its own
repository (GitHub only reads `.github/workflows` at the repo root).

## Run (CLI / dev)

```bash
# Full binary (embedded UI):
cargo build --release --features embed-ui
./target/release/slotlab            # → http://localhost:3600

# Dev (hot-reload UI):
cargo run                            # backend :3600 + RGS :3601
cd frontend && pnpm dev              # UI on :5173 (proxy /api → :3600)
```

Ports: `:3600` UI + API (HTTP) · `:3601` emulated RGS (HTTPS, mandatory — the
games hardcode `https://`) · `:3602` share server (started on demand,
exposed via cloudflared).

## First-time setup

1. Open the UI → **Games** tab → add a game: name (RGS slug) + path
   `…/math/games/<id>/library/publish_files/` + frontend URL (Vite dev or
   served build).
2. **Play** tab → **Install CA** banner (once, macOS password): installs the
   "SlotLab Local CA" into the login keychain so browsers accept
   `https://localhost:3601`.
3. **Import books** (button, or automatic on the first Launch): re-encodes the
   `books_*.jsonl.zst` into indexed zstd blocks in
   `~/Library/Application Support/slotlab/cache/` (~1-3 min for a big game,
   once only — automatically invalidated if the math changes).
4. Launch the game's frontend (`pnpm --filter <game> run dev -- --port 5174`)
   and **Launch**.

## Tabs

- **Games** — game registry, RTP/hit/max win per mode, reload.
- **Play** — multi-viewport (7 presets + custom), currency (fiat + XGC/XSC),
  social mode, language, balance, sticky **force (mode, event id)**, bookmarks,
  live spin feed (SSE), reset sessions, replay deep-links.
- **Analyze** — full stats per mode (RTP, hit, break-even, CV volatility
  and std/cost, CVaR@0.1%, ETL 40x/10kx, P≥5k/10k, weighted median…),
  1-2-5 bucket distribution with RTP contribution, top payouts with
  "▶ Play this book", and the **17 Mnemoo checks + 4 in-house checks**
  (0.1x granularity, most-likely ≥ 1/1000, integer cost, star tier).
- **Search** — book search by min/max win, criteria
  (basegame/freegame/wincap…), event type present (freeSpinTrigger…),
  number of retriggers; trigger stats per mode; each hit can be forced or
  bookmarked.
- **CrowdSim** — seeded Monte-Carlo cohorts (PoP, ruin, drawdown, streaks,
  p5/p50/p95 balance curves) + RTP convergence up to 50M spins.
- **Review** — "Event IDs" block (average/high/max win per mode) ready to
  paste for the Stake submission.
- **Share** — the game's static build + emulated RGS behind a Cloudflare
  quick tunnel (`*.trycloudflare.com`): a playable link to send to a friend,
  with nothing to install on their end. Prerequisite: `brew install cloudflared`.

## Architecture

```
crates/slotlab-core   # pure (wasm-able): parse index.json/LUT, stats, checks,
                      # crowdsim, review — all the formulas
crates/slotlab-app    # binary: axum (UI+API, RGS TLS, share), books loader
                      # (2 MiB zstd blocks + .idx sidecar + mmap + double LRU),
                      # sessions, rcgen TLS, cloudflared tunnel
frontend/             # Vite + Svelte 5 + Chart.js (SPA, embedded in release)
```

The emulated RGS contract follows the games'
`frontend/packages/rgs-fetcher/src/schema.ts`:
`/wallet/authenticate|balance|play|end-round`, `/bet/event`,
`GET /bet/replay/{game}/{version}/{mode}/{event}`. Amounts in micro-units
(1,000,000 = $1). Modes resolved case-insensitively, float `cost` tolerated
(gotchas inherited from the math-sdk).

## Perf notes

The books are **never** loaded whole into RAM: the one-shot import splits
the stream into 2 MiB zstd frames (`.blocks`) + an index
sim_id → (block, offset) and search metadata (`.idx`). At read time:
mmap + LRU of 128 decompressed blocks (~256 MiB) + LRU of parsed books.
The full Tachyon (16 GB decompressed) runs with ~300 MB of RSS.
