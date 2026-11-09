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
3. **Import books** (button, or automatic on the first Launch): indexes the
   `books_*.jsonl.zst` where they are — no copy — into
   `~/Library/Application Support/slotlab/cache/` (~30 s and ~5 % of the books'
   size for an eight-mode game, once per build: redone automatically when the
   math changes, even in the middle of an import).
4. Launch the game's frontend (`pnpm --filter <game> run dev -- --port 5174`)
   and **Launch**.

## Tabs

- **Games** — game registry, RTP/hit/max win per mode, reload, and **what
  changed in the last build**: every load with a new fingerprint is recorded,
  so a `run.py prod` that moved an RTP by a thousandth or halved a hit rate
  leaves a trace instead of passing silently.
- **Play** — multi-viewport (9 presets + custom, incl. the two aspect ratios a
  review has refused a game over, and a **Review pack** button for the four it
  always opens), currency (fiat + XGC/XSC), social mode, language, balance,
  sticky **force (mode, event id)**, bookmarks, live spin feed (SSE), reset
  sessions, replay deep-links, plus **QA scenarios**: fault injection in the
  emulated RGS — leave the next round open (unpaid), walk the balance down to
  the edge of a spin, arm an RGS error / HTTP 500 / a call that never answers
  on any endpoint, add latency, or launch against a broken `rgs_url`. Four of
  the studio's refusals are about states that cannot be reached by playing
  normally; here they are one click.
- **Analyze** — organised around *populations*: a whole mode, its spins
  without bonus, all its bonuses, each bonus tier (number of trigger symbols,
  whatever criteria opened it), each variant of a tier (tier × criteria) and
  each criteria. Every population gets the same block, weighted by the
  published LUT: frequency, RTP share, **median next to the average**,
  percentiles P5…P99.9 (each replayable), min/max, std and CV, share paying
  0x / at least the cost, share under 10x/20x/50x/100x (+ any threshold you
  type), value carried by the top 1 %/10 %, retriggers (rate, mean, win with
  vs without), free spins awarded/played, the bonus's own win
  (`freeGameWins`), and sample size (books, effective outcomes). Views:
  **Overview** (findings computed from the numbers — « half of the 4-scatter
  bonuses pay under 9.2x while the average is 96.5x » —, every mode, tail
  risk), **Explore** (tree + the distribution curve and bands of one
  population, its breakdown, the same tier in every mode), **Compare** (any
  populations of any modes: overlaid curves, bands, metric matrix), and the
  **17 Mnemoo checks + 4 in-house checks** in **Compliance**
  (0.1x granularity, most-likely ≥ 1/1000, integer cost, star tier).
  Two more lenses: **Win tiers** — the celebration ladder measured on the
  published LUT mode by mode (read from the game's own `winLevelThresholds.ts`
  when it has one): how often each plaque shows up, how many SECONDS it costs
  the average spin, and a field to try another ladder before touching the game;
  **Fleet** — the base mode of every registered game side by side, the only
  place that says whether an RTP, a hit rate or a ceiling is normal here;
  **Market** — every mode against the modes of the same kind of the slots the
  market plays (base games of a close max win, double chances of the SAME
  cost, bonus buys and all-or-nothing buys of a close price; a slot counts
  once, through its closest mode, and the games Stake files as burst games —
  `scripts/market/burst_games.js` — are left out): each figure —
  hit rate, the win ladder, max win odds, 6 000 spins without 500x, median
  return of a buy, share refunding nothing, volatility… — as the peers'
  spread (p10 · middle half p25–p75 · p90), their median, and where the mode
  falls, with the most played peers listed.
- **Muted findings** — a rule that makes no sense for a given slot can be
  muted per game, in one mode or in every mode (« Mute » under an opened
  finding in Analyze → Overview, Preflight, CrowdSim). Muted findings leave
  every counter and stay listed, folded, under « N muted findings » with an
  **Unmute** button. Kept on the server (`<app dir>/muted/<game>.json`,
  `GET/POST/PUT/DELETE /api/ui/games/{name}/muted`), so the Mac, the remote UI
  and the CLI report see the same list. Blocking findings (Stake compliance,
  publish checks, star limits, gaps, most likely book, bonus ladder, the
  checklist items that already cost a refusal) cannot be muted.
- **Search** — book search by min/max win, criteria
  (basegame/freegame/wincap…), event type present (freeSpinTrigger…),
  number of retriggers; trigger stats per mode; each hit can be forced or
  bookmarked.
- **Scan** — every book of a mode read once, WEIGHTED BY THE LUT: the staging
  invariants nothing else sees. Anticipation fired vs. deserved (early / late /
  missing / cannot-trigger, against the real rule — the reel that follows the
  one completing trigger − 1, intra-column on the last reel), scatters on the
  visible window (padding excluded, which is where a naive scan goes wrong),
  bonus tiers and their monotonicity, re-triggers, wild moves off the grid, the
  share of rounds carrying each event, and the PACING in staging seconds
  (mean / p50 / p95). The trigger size and the padding are derived from the
  books; a mode that anticipates on every spin is flagged as such instead of
  being reported as broken. Kept per build fingerprint, on disk.
- **Preflight** — the pre-upload checklist, run against the files: ~28 text
  rules over the game's sources, the shared kit and the BUILD (renderer,
  bet-level guards, amount formatting, restricted terminology, social sweep,
  `--ui-*` tokens, reduced motion, hygiene) plus the checks only this process
  can make — ghost assets (`key=` vs `assets.ts`), version sync, book events
  the frontend never names, orphan files in publish_files, oversized book
  lines, and `verify_publish`: the LUT payout against the book payout, by id,
  over every row. Each finding carries its file, its line, and the section of
  the studio doc that justifies it.
- **Challenge** — Stake challenge calculator from the mode's published LUT:
  hit rate of any target multiplier, hunter-EV matrix (targets × awards,
  verdict-colored: dead/thin/sweet/generous/giveaway), operator break-even
  (turnover & spins = award ÷ house edge), per-award suggested target
  (hardest still ≥ ×1.5 EV) and a single-challenge simulator (lone-claim
  net, expected grind). Round multiplier = win ÷ total stake, so bonus
  buys dilute themselves out.
- **CrowdSim** — seeded Monte-Carlo cohorts (PoP, ruin, drawdown, streaks,
  p5/p50/p95 balance curves) + RTP convergence up to 50M spins. **Player
  feel** reads the frustrations of a mode, exact on the LUT and counted on the
  cohort's own sessions. A base game: bonus **droughts** (1 000 spins with one
  bonus or none), bonus **letdowns** (3+ bonuses in 300 spins, all under 20x
  the stake), **dry spells** (longest run without a win), **broke before the
  bonus** (a 100-stake deposit lost before the first bonus — estimated on
  12 000 deposits) and **big wins out of reach** (6 000 spins without 500x).
  A bought or costly round: **purchases that refund nothing** (under 25 % of
  the price; all-or-nothing modes are read, not flagged). Every definition is
  editable; the median ÷ average bonus sits next to its market band. Each
  frustration past its line gets a **fix**: the LUT re-weighted (books
  untouched, RTP kept), paid two or three ways — the base game, the big wins,
  the bonus, the pops or the middle of a buy — each read as before → after
  (hit rate, median bonus, wins ≥ 500x, refunds, max win, RTP budget per line,
  and any other frustration it pushes past its line), the gentlest one
  recommended, and any of them playable as a new cohort next to the original.
  **Market**: each base mode is also read against the slots the market plays
  with a close max win, and each double chance against the double chances of
  the same cost — a 3x ante against the slots that have a 3x mode, the closest
  max wins first (every mode of the slots the market plays NOW: the most
  played slots of the last 30 days, from the top down until they make 90 % of
  the slot turnover, instant games left out, from the certified lookup tables
  StakeCruncher serves). SlotLab rebuilds this reference every day while it
  runs (`market_refresh.rs`: today's ranking, only the new lookup tables are
  downloaded, cache in `~/Library/Application Support/slotlab/market/`) and
  swaps it in live; the Market tab shows the day of the ranking and an
  « Update now » button; `crates/slotlab-core/data/market.json` (refreshed by
  `scripts/market/fetch_market.py`, same rule and cache) is only the offline
  fallback built in:
  hit rate, wins ≥ 100/500/1 000/5 000x the stake,
  max win odds, 6 000 spins without 500x/1 000x, volatility, as p25 / median /
  p75. The **one fix for everything** then starts from the peers' median
  (every rung of the ladder and the max win's odds on it, the RTP kept) and
  takes each frustration's best way only as far as the market's middle half
  allows.
- **Review** — "Event IDs" block (average/high/max win per mode) ready to
  paste for the Stake submission, and a **client report**: the same math in
  whole sentences (how often it pays, how much, how long before something big,
  where the ceiling is), Markdown, ready to send.
- **Assets** — what the player downloads to start and what the textures cost
  in video memory, as a RANGE (mutually exclusive atlas scales and audio
  formats), plus a **contact sheet** of every image the build ships: the one
  check no rule replaces.
- **Share** — the game's static build + emulated RGS behind a Cloudflare
  quick tunnel (`*.trycloudflare.com`): a playable link to send to a friend,
  with nothing to install on their end. Prerequisite: `brew install cloudflared`.
  **Pinned shares** (API only) live beside the tab's one, each on its own
  port behind a stable name (named tunnel, reverse proxy):
  `POST /api/ui/share/start {"game", "buildDir", "balance", "port": 3603,
  "publicHost": "x.example.com", "tunnel": false, "console": false}` — the
  visitor's `rgs_url` uses `publicHost`; `tunnel: false` skips the quick
  tunnel. `GET /api/ui/share/status` lists them under `pinned`;
  `POST /api/ui/share/stop {"port": 3603}` stops one. A share's RGS answers
  for the shared game only (404 otherwise). Shares die with SlotLab: restart
  a pinned one from a watchdog (e.g. a systemd user unit polling `status`).

## Architecture

```
crates/slotlab-core   # pure (wasm-able): parse index.json/LUT, stats, checks,
                      # crowdsim, review, celebration tiers, book invariants,
                      # preflight rules — all the formulas and all the rules
crates/slotlab-app    # binary: axum (UI+API, RGS TLS, share), books index
                      # (read in place: zstd resume points + .idx sidecar),
                      # sessions + fault injection, book scan jobs, build
                      # history, rcgen TLS, cloudflared tunnel
frontend/             # Vite + Svelte 5 + Chart.js (SPA, embedded in release)
```

The emulated RGS contract follows the games'
`frontend/packages/rgs-fetcher/src/schema.ts`:
`/wallet/authenticate|balance|play|end-round`, `/bet/event`,
`GET /bet/replay/{game}/{version}/{mode}/{event}`. Amounts in micro-units
(1,000,000 = $1). Modes resolved case-insensitively, float `cost` tolerated
(gotchas inherited from the math-sdk).

## Perf notes

The books are **never** copied nor loaded whole into RAM: SlotLab reads them
in the game's own `books_<MODE>.jsonl.zst`. That file is ONE zstd frame,
which normally means decompressing everything before a book to reach it; the
import decodes it once and, every 64 MiB of books, saves what a zstd decoder
carries from one block to the next (the 2 MiB window, the Huffman and FSE
tables, the three repeat offsets — `crates/slotlab-app/csrc/zstd_resume.c`).
A fresh decoder restored from such a **resume point** reads the original file
from there (`src/books/zran.rs`). The cache of a mode is its search index
(`.idx`, compressed) plus these resume points (`.ckpt`, ~3-5 % of the
`.zst`): Pumpkin Spells went from 8.5 GB of cache to 0.3 GB. Before an index
is written, every book is read back through the resume points and compared
with what the import saw; a source that could not be entered that way would
be copied into independent frames instead (`.zcopy`, never seen so far).

Reaching one book decodes 32 MiB on average: ~15 ms per spin. The import runs
three modes at a time (one core decodes each, all cores parse the JSON):
~30 s for Pumpkin Spells' eight modes (92 GB of JSON), against ~100 s when
every book was re-compressed into blocks. The mechanics of a mode (Analyze →
Mechanics) are no longer computed at import — they were two thirds of its
cost — but the first time that tab opens a mode (~5-15 s), then kept.

A **book scan** (Scan tab) and every other pass over all the books run in
parallel, one task per stretch between two resume points: ~0.7 s to read the
17 GB of a base game. A scan result is kept on disk against the build's
fingerprint, so it is computed once per `run.py prod`.
