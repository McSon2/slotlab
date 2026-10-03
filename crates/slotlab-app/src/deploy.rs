//! One-click publishing of a game to the always-on hub.
//!
//! It runs HERE, on the machine that has the files: a web page served by
//! the hub could never read `~/Documents/.../tachyon`, and pushing 2 GB of
//! books through Cloudflare would hit the 100 MB body limit anyway. So the
//! button lives in SlotLab, the transfer goes over the LAN, and the hub is
//! only told to reload once the files have landed.
//!
//! External commands rather than HTTP crates: rsync and ssh are needed
//! regardless, and `curl` keeps the binary free of a second TLS stack —
//! this same binary is what runs on the homelab.

use crate::persist;
use crate::state::AppState;
use anyhow::{bail, Context, Result};
use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct DeployConfig {
    /// SSH target that can write the hub's data directory.
    pub ssh_target: String,
    /// Data directory as seen from that SSH target.
    pub remote_dir: String,
    /// Hub control API, reachable over the LAN.
    pub hub_url: String,
    pub admin_token: String,
    pub cf_token: String,
    pub cf_zone_id: String,
    pub cf_account_id: String,
    pub cf_tunnel_id: String,
    /// Zone the sub-domains are created in.
    pub base_domain: String,
    /// Hub as seen from the tunnel, e.g. `http://192.168.1.200:3610`.
    pub tunnel_target: String,

    // Hub rebuild — used when this SlotLab writes a book-index format the
    // running hub can't read. Empty = the defaults of `HubBuild::resolve`.
    /// SlotLab sources on this Mac (default: the checkout this app was built from).
    pub hub_source_dir: String,
    /// Where the sources go, as seen from `ssh_target`.
    pub hub_remote_src: String,
    /// Proxmox container running Docker (`pct exec`); "-" = Docker on `ssh_target` itself.
    pub hub_ct: String,
    /// The same sources as seen from Docker (inside the container).
    pub hub_build_dir: String,
    /// Compose stack of the hub.
    pub hub_stack_dir: String,
    pub hub_image: String,

    /// Containers frozen (`docker pause`) while a deployment runs, e.g.
    /// `tiktok-bot` — comma- or space-separated.
    pub pause_during_deploy: String,
}

/// Effective hub-rebuild settings (config, or this homelab's layout).
struct HubBuild {
    source_dir: PathBuf,
    remote_src: String,
    ct: Option<String>,
    build_dir: String,
    stack_dir: String,
    image: String,
}

impl HubBuild {
    fn resolve(cfg: &DeployConfig) -> Result<Self> {
        let or = |v: &str, d: &str| {
            if v.trim().is_empty() { d.to_string() } else { v.trim().to_string() }
        };
        // The checkout this binary was compiled from — right on the Mac that
        // builds SlotLab, absent on a machine that installed a release.
        let built_from = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let source_dir = if cfg.hub_source_dir.trim().is_empty() {
            built_from.canonicalize().unwrap_or(built_from)
        } else {
            PathBuf::from(cfg.hub_source_dir.trim())
        };
        if !source_dir.join("crates/slotlab-app/Cargo.toml").is_file() {
            bail!(
                "SlotLab sources not found in {} — set \"Hub source folder\" in the deployment settings",
                source_dir.display()
            );
        }
        // Sources sit next to the games folder: …/slotlab/games → …/slotlab/src.
        let default_remote_src = Path::new(cfg.remote_dir.trim_end_matches('/'))
            .parent()
            .map(|p| p.join("src").to_string_lossy().to_string())
            .unwrap_or_default();
        Ok(Self {
            source_dir,
            remote_src: or(&cfg.hub_remote_src, &default_remote_src),
            ct: docker_ct(cfg),
            build_dir: or(&cfg.hub_build_dir, "/mnt/storage/slotlab/src"),
            stack_dir: or(&cfg.hub_stack_dir, "/opt/stacks/slotlab-share"),
            image: or(&cfg.hub_image, "slotlab-share:latest"),
        })
    }

    /// A command run where Docker lives (through `pct exec` on Proxmox).
    fn docker_cmd(&self, cmd: &str) -> String {
        match &self.ct {
            Some(ct) => format!("pct exec {ct} -- sh -c {}", shell_quote(cmd)),
            None => cmd.to_string(),
        }
    }
}

/// Proxmox container running Docker, `None` when Docker is on `ssh_target`.
fn docker_ct(cfg: &DeployConfig) -> Option<String> {
    let ct = cfg.hub_ct.trim();
    let ct = if ct.is_empty() { "102" } else { ct };
    (ct != "-").then(|| ct.to_string())
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

fn config_path() -> PathBuf {
    persist::app_dir().join("deploy.json")
}

pub fn load_config() -> DeployConfig {
    std::fs::read(config_path())
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save_config(c: &DeployConfig) -> Result<()> {
    let path = config_path();
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    std::fs::write(&path, serde_json::to_vec_pretty(c)?)?;
    // Holds a Cloudflare token that can edit the whole zone's DNS.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

// ───────────────────────────────── Job ─────────────────────────────────

#[derive(Clone, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum StepState {
    Pending,
    Running,
    Done,
    Failed,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    pub label: String,
    pub state: StepState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DeployJob {
    pub running: bool,
    pub game: String,
    pub steps: Vec<Step>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Default)]
pub struct DeployManager {
    job: Mutex<DeployJob>,
}

impl DeployManager {
    fn start(&self, game: &str, labels: &[&str]) -> bool {
        let mut j = self.job.lock().unwrap();
        if j.running {
            return false;
        }
        *j = DeployJob {
            running: true,
            game: game.to_string(),
            steps: labels
                .iter()
                .map(|l| Step {
                    label: l.to_string(),
                    state: StepState::Pending,
                    detail: None,
                })
                .collect(),
            url: None,
            error: None,
        };
        true
    }

    fn step(&self, i: usize, state: StepState, detail: Option<String>) {
        let mut j = self.job.lock().unwrap();
        if let Some(s) = j.steps.get_mut(i) {
            s.state = state;
            if detail.is_some() {
                s.detail = detail;
            }
        }
    }

    fn finish(&self, url: Option<String>, error: Option<String>) {
        let mut j = self.job.lock().unwrap();
        j.running = false;
        j.url = url;
        j.error = error;
    }

    pub fn status(&self) -> DeployJob {
        self.job.lock().unwrap().clone()
    }
}

// ─────────────────────────────── Routes ───────────────────────────────

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/ui/deploy/config", get(get_config).post(set_config))
        .route("/api/ui/deploy/start", post(start))
        .route("/api/ui/deploy/status", get(status))
        .route("/api/ui/deploy/published", get(published))
}

/// Secrets never travel back to the UI: the page only needs to know they
/// are set, so it can show "configuré" instead of an empty box.
fn masked(c: &DeployConfig) -> Value {
    let mask = |s: &String| if s.is_empty() { "" } else { "••••••••" }.to_string();
    json!({
        "sshTarget": c.ssh_target, "remoteDir": c.remote_dir, "hubUrl": c.hub_url,
        "adminToken": mask(&c.admin_token), "cfToken": mask(&c.cf_token),
        "cfZoneId": c.cf_zone_id, "cfAccountId": c.cf_account_id,
        "cfTunnelId": c.cf_tunnel_id, "baseDomain": c.base_domain,
        "tunnelTarget": c.tunnel_target,
        "hubSourceDir": c.hub_source_dir, "hubRemoteSrc": c.hub_remote_src,
        "hubCt": c.hub_ct, "hubBuildDir": c.hub_build_dir,
        "hubStackDir": c.hub_stack_dir, "hubImage": c.hub_image,
        "pauseDuringDeploy": c.pause_during_deploy,
        "configured": !c.ssh_target.is_empty() && !c.hub_url.is_empty()
            && !c.admin_token.is_empty(),
        // Compared by the page with the hub's `idxVersion` to announce a rebuild.
        "localIdxVersion": crate::books::format::IDX_VERSION,
    })
}

async fn get_config() -> Json<Value> {
    Json(masked(&load_config()))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct ConfigPatch {
    ssh_target: String,
    remote_dir: String,
    hub_url: String,
    admin_token: String,
    cf_token: String,
    cf_zone_id: String,
    cf_account_id: String,
    cf_tunnel_id: String,
    base_domain: String,
    tunnel_target: String,
    hub_source_dir: String,
    hub_remote_src: String,
    hub_ct: String,
    hub_build_dir: String,
    hub_stack_dir: String,
    hub_image: String,
    pause_during_deploy: String,
}

impl Default for ConfigPatch {
    fn default() -> Self {
        Self {
            ssh_target: String::new(),
            remote_dir: String::new(),
            hub_url: String::new(),
            admin_token: String::new(),
            cf_token: String::new(),
            cf_zone_id: String::new(),
            cf_account_id: String::new(),
            cf_tunnel_id: String::new(),
            base_domain: String::new(),
            tunnel_target: String::new(),
            hub_source_dir: String::new(),
            hub_remote_src: String::new(),
            hub_ct: String::new(),
            hub_build_dir: String::new(),
            hub_stack_dir: String::new(),
            hub_image: String::new(),
            pause_during_deploy: String::new(),
        }
    }
}

async fn set_config(Json(p): Json<ConfigPatch>) -> Json<Value> {
    let mut c = load_config();
    c.ssh_target = p.ssh_target;
    c.remote_dir = p.remote_dir;
    c.hub_url = p.hub_url;
    c.cf_zone_id = p.cf_zone_id;
    c.cf_account_id = p.cf_account_id;
    c.cf_tunnel_id = p.cf_tunnel_id;
    c.base_domain = p.base_domain;
    c.tunnel_target = p.tunnel_target;
    c.hub_source_dir = p.hub_source_dir;
    c.hub_remote_src = p.hub_remote_src;
    c.hub_ct = p.hub_ct;
    c.hub_build_dir = p.hub_build_dir;
    c.hub_stack_dir = p.hub_stack_dir;
    c.hub_image = p.hub_image;
    c.pause_during_deploy = p.pause_during_deploy;
    // An untouched secret arrives masked: keep the stored one.
    if !p.admin_token.is_empty() && !p.admin_token.starts_with('•') {
        c.admin_token = p.admin_token;
    }
    if !p.cf_token.is_empty() && !p.cf_token.starts_with('•') {
        c.cf_token = p.cf_token;
    }
    match save_config(&c) {
        Ok(()) => Json(masked(&c)),
        Err(e) => Json(json!({ "error": format!("{e:#}") })),
    }
}

async fn status(State(state): State<AppState>) -> Json<DeployJob> {
    Json(state.deploy.status())
}

/// Games currently served by the hub, so the tab can say "mise à jour"
/// rather than "publication" and pre-fill the existing settings.
async fn published() -> Json<Value> {
    let c = load_config();
    if c.hub_url.is_empty() || c.admin_token.is_empty() {
        return Json(json!({ "games": [] }));
    }
    match hub_get(&c, "/api/hub/games") {
        Ok(v) => Json(v),
        Err(e) => Json(json!({ "error": format!("{e:#}"), "games": [] })),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StartReq {
    game: String,
    /// Sub-domain only: `skullperch` for skullperch.mcberry.fr.
    subdomain: String,
    #[serde(default = "default_balance")]
    balance: u64,
    #[serde(default = "default_currency")]
    currency: String,
    #[serde(default = "default_language")]
    language: String,
    #[serde(default)]
    social: bool,
    #[serde(default)]
    access_code: String,
    /// Console de démonstration dans la page du jeu.
    #[serde(default)]
    console: bool,
}

fn default_balance() -> u64 {
    100_000
}
fn default_currency() -> String {
    "USD".into()
}
fn default_language() -> String {
    "en".into()
}

const STEPS: [&str; 8] = [
    "Checks",
    "Hub version",
    "Math (index.json + LUT)",
    "Frontend build",
    "Books",
    "Register on the hub",
    "Cloudflare subdomain",
    "Link check",
];

async fn start(State(state): State<AppState>, Json(req): Json<StartReq>) -> Json<Value> {
    if !state.deploy.start(&req.game, &STEPS) {
        return Json(json!({ "error": "a deployment is already running" }));
    }
    let st = state.clone();
    tokio::task::spawn_blocking(move || {
        match run(&st, &req) {
            Ok(url) => st.deploy.finish(Some(url), None),
            Err(e) => {
                let msg = format!("{e:#}");
                // Mark the step that was still running as failed.
                let idx = {
                    let j = st.deploy.status();
                    j.steps
                        .iter()
                        .position(|s| matches!(s.state, StepState::Running))
                };
                if let Some(i) = idx {
                    st.deploy.step(i, StepState::Failed, Some(msg.clone()));
                }
                st.deploy.finish(None, Some(msg));
            }
        }
    });
    Json(json!({ "ok": true }))
}

// ───────────────────────────── Deployment ─────────────────────────────

fn run(state: &AppState, req: &StartReq) -> Result<String> {
    let cfg = load_config();
    let d = &state.deploy;

    // 1 — checks
    d.step(0, StepState::Running, None);
    for (label, v) in [
        ("SSH target", &cfg.ssh_target),
        ("remote folder", &cfg.remote_dir),
        ("hub URL", &cfg.hub_url),
        ("hub token", &cfg.admin_token),
    ] {
        if v.trim().is_empty() {
            bail!("incomplete settings: {label} missing (Publish → Homelab → Deployment settings)");
        }
    }
    let sub = req.subdomain.trim().trim_matches('.').to_ascii_lowercase();
    if sub.is_empty() || !sub.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        bail!("invalid subdomain: \"{}\"", req.subdomain);
    }
    let game = state
        .registry
        .get(&req.game)
        .with_context(|| format!("unknown game: {}", req.game))?;
    let build_dir = game
        .entry
        .build_dir
        .clone()
        .context("no frontend build found — run `pnpm --filter <game> build`, then Reload in Games")?;
    if !build_dir.join("index.html").is_file() {
        bail!(
            "no index.html in {} — the build was never produced",
            build_dir.display()
        );
    }
    if !state.books.is_imported(&game) {
        bail!("books not imported (or stale) for {} — Games → Import books. Publishing now would ship books that no longer match the LUTs.", req.game);
    }
    // Games the hub serves right now — read before a possible rebuild, which
    // restarts it and makes the ones with an old-format `.idx` unloadable.
    let hub_games = hub_get(&cfg, "/api/hub/games")?;
    let cache_dir = state.books.cache_dir_for(&game);
    let host = if cfg.base_domain.trim().is_empty() {
        sub.clone()
    } else {
        format!("{sub}.{}", cfg.base_domain.trim())
    };
    let remote = format!("{}/{}", cfg.remote_dir.trim_end_matches('/'), req.game);
    // Held until `run` returns, whatever the outcome: dropping it resumes them.
    let paused = Paused::pause(&cfg)?;
    d.step(
        0,
        StepState::Done,
        Some(if paused.names.is_empty() {
            format!("→ {host}")
        } else {
            format!("→ {host} · paused until the end: {}", paused.names.join(", "))
        }),
    );

    // 2 — the `.idx` files are sent as they are: the hub must read this
    // SlotLab's format, or every spin answers "books not imported". A hub
    // older than the `idxVersion` field only reads version 1.
    d.step(1, StepState::Running, None);
    let hub_idx = hub_games.get("idxVersion").and_then(Value::as_u64).unwrap_or(1);
    let local_idx = crate::books::format::IDX_VERSION as u64;
    if hub_idx == local_idx {
        d.step(1, StepState::Done, Some(format!("up to date (index v{local_idx})")));
    } else {
        let detail = rebuild_hub(state, &cfg, &req.game, &hub_games, hub_idx, local_idx)?;
        d.step(1, StepState::Done, Some(detail));
    }

    // 2/3/4 — files, over the LAN
    ssh(&cfg, &format!("mkdir -p {remote}/publish {remote}/cache {remote}/build"))?;
    d.step(2, StepState::Running, None);
    let n = rsync(
        &game.entry.publish_dir,
        &format!("{}:{remote}/publish/", cfg.ssh_target),
        &["--exclude", "*.zst"],
    )?;
    d.step(2, StepState::Done, Some(n));
    d.step(3, StepState::Running, None);
    let n = rsync(
        &build_dir,
        &format!("{}:{remote}/build/", cfg.ssh_target),
        &["--delete"],
    )?;
    d.step(3, StepState::Done, Some(n));
    d.step(4, StepState::Running, None);
    let n = rsync(&cache_dir, &format!("{}:{remote}/cache/", cfg.ssh_target), &[])?;
    d.step(4, StepState::Done, Some(n));

    // 5 — the hub picks it up without restarting: sessions in flight survive
    d.step(5, StepState::Running, None);
    let manifest = json!({
        "name": req.game,
        "host": host,
        "balance": req.balance,
        "currency": req.currency,
        "language": req.language,
        "social": req.social,
        "accessCode": req.access_code.trim(),
        "console": req.console,
        "publishedAt": iso_now(),
    });
    let res = hub_post(&cfg, "/api/hub/publish", &manifest)?;
    if res.get("ok").and_then(Value::as_bool) != Some(true) {
        let errs = res
            .get("errors")
            .map(|e| e.to_string())
            .unwrap_or_else(|| res.to_string());
        bail!("the hub rejected the game: {errs}");
    }
    d.step(5, StepState::Done, None);

    // 6 — Cloudflare: ingress rule then CNAME. One without the other and
    // the hostname simply does not answer.
    d.step(6, StepState::Running, None);
    if cfg.cf_token.is_empty() {
        d.step(6,
            StepState::Done,
            Some("skipped: no Cloudflare token configured".into()),
        );
    } else {
        let detail = cloudflare_route(&cfg, &sub, &host)?;
        d.step(6, StepState::Done, Some(detail));
    }

    // 7 — proof
    d.step(7, StepState::Running, None);
    let url = format!("https://{host}/play");
    let mut last = String::new();
    for _ in 0..20 {
        match curl_code(&format!("https://{host}/healthz")) {
            Ok(200) => {
                d.step(7, StepState::Done, None);
                return Ok(url);
            }
            Ok(code) => last = format!("HTTP {code}"),
            Err(e) => last = format!("{e:#}"),
        }
        std::thread::sleep(std::time::Duration::from_secs(3));
    }

    // A brand-new sub-domain is exactly the case where THIS machine's
    // resolver still holds the negative answer it cached before the record
    // existed. So ask the hub directly, over the LAN, whether it serves
    // that hostname: if it does, the deployment worked and only the local
    // DNS is behind — that is a warning, not a failure.
    let served = Command::new("curl")
        .args(["-s", "-o", "/dev/null", "-w", "%{http_code}", "--max-time", "10"])
        .arg("-H")
        .arg(format!("Host: {host}"))
        .arg(format!("{}/play", cfg.hub_url.trim_end_matches('/')))
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    if served.starts_with('2') || served.starts_with('3') {
        d.step(7,
            StepState::Done,
            Some(format!(
                "the hub serves {host}, but this Mac does not resolve it yet \
                 ({last}) — give it a few minutes, or run `sudo dscacheutil -flushcache`"
            )),
        );
        return Ok(url);
    }
    bail!("{host} does not answer ({last}), and the hub does not serve it either (HTTP {served})")
}

/// Containers frozen for the length of a deployment. On the homelab, a
/// video render and the hub taking in 2 GB of books don't fit together in
/// the CT's 3 GB: everything ends up swapping, Mosquitto included, and the
/// home automation goes down with it. Paused rather than stopped, so the
/// render resumes exactly where it was.
///
/// The resume commands are written to a script ON THE SERVER, run by
/// `Drop` and, failing that, by a watchdog an hour later — a Mac that
/// sleeps or quits mid-deployment must not leave the bot frozen for good.
struct Paused<'a> {
    cfg: &'a DeployConfig,
    names: Vec<String>,
}

const RESUME_SCRIPT: &str = "/run/slotlab-deploy-resume.sh";
const RESUME_WATCHDOG_SECS: u32 = 3600;

impl<'a> Paused<'a> {
    fn pause(cfg: &'a DeployConfig) -> Result<Self> {
        let names: Vec<&str> = cfg
            .pause_during_deploy
            .split([',', ' '])
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .collect();
        if names.is_empty() {
            return Ok(Self { cfg, names: Vec::new() });
        }
        if let Some(bad) = names
            .iter()
            .find(|n| !n.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c)))
        {
            bail!("invalid container name to pause: \"{bad}\"");
        }
        let docker = match docker_ct(cfg) {
            Some(ct) => format!("pct exec {ct} -- docker"),
            None => "docker".into(),
        };
        let f = RESUME_SCRIPT;
        // Only the RUNNING ones: a container stopped or paused by hand is
        // left exactly as it is, and never resumed by us.
        let script = format!(
            r#"{resume}
for n in {names}; do
  if [ "$({docker} inspect -f '{{{{.State.Status}}}}' "$n" 2>/dev/null)" = running ] && {docker} pause "$n" >/dev/null; then
    echo "{docker} unpause $n" >> {f}; echo "$n"
  fi
done
if [ -f {f} ]; then
  echo 'rm -f "$0" "$0.pid"' >> {f}
  nohup sh -c 'sleep {secs}; [ -f {f} ] && sh {f}' >/dev/null 2>&1 </dev/null &
  echo $! > {f}.pid
fi"#,
            resume = resume_script(),
            names = names.join(" "),
            secs = RESUME_WATCHDOG_SECS,
        );
        let out = ssh_out(cfg, &script).context("pausing containers before the upload")?;
        Ok(Self {
            cfg,
            names: out.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect(),
        })
    }
}

impl Drop for Paused<'_> {
    fn drop(&mut self) {
        if !self.names.is_empty() {
            let _ = ssh(self.cfg, &resume_script());
        }
    }
}

/// Resumes what a deployment paused, and stops its watchdog. Also run
/// before pausing: a previous deployment that never resumed is settled first.
fn resume_script() -> String {
    let f = RESUME_SCRIPT;
    format!("if [ -f {f} ]; then [ -f {f}.pid ] && kill \"$(cat {f}.pid)\" 2>/dev/null; sh {f}; fi")
}

/// Brings the hub to this SlotLab's book-index format: sources → image →
/// recreated container, then the other published games get their books
/// re-sent (the local caches were re-imported in the new format; the ones
/// on the hub are still the old one and would no longer load). The restart
/// cuts the sessions in flight — unavoidable, the old binary can't read v4.
fn rebuild_hub(
    state: &AppState,
    cfg: &DeployConfig,
    publishing: &str,
    hub_games: &Value,
    hub_idx: u64,
    local_idx: u64,
) -> Result<String> {
    let d = &state.deploy;
    let hb = HubBuild::resolve(cfg)?;
    let say = |msg: String| d.step(1, StepState::Running, Some(msg));
    say(format!(
        "the hub reads index v{hub_idx}, this SlotLab writes v{local_idx} — updating the hub (~5 min, it restarts)"
    ));

    ssh(cfg, &format!("mkdir -p {}", shell_quote(&hb.remote_src)))?;
    // Same excludes as scripts/deploy-share.sh; the server keeps its own Dockerfile.
    rsync(
        &hb.source_dir,
        &format!("{}:{}/", cfg.ssh_target, hb.remote_src.trim_end_matches('/')),
        &[
            "--exclude", "target", "--exclude", "node_modules", "--exclude", ".git",
            "--exclude", "dist", "--exclude", "Dockerfile",
        ],
    )?;

    let build = hb.docker_cmd(&format!(
        "docker build --progress=plain -t {} {}",
        shell_quote(&hb.image),
        shell_quote(&hb.build_dir)
    ));
    ssh_streaming(cfg, &build, |line| {
        // BuildKit plain output: "#12 41.3    Compiling serde v1.0.219".
        if let Some(i) = line.find("Compiling ") {
            say(format!("building the hub image — {}", line[i..].trim()));
        } else if line.contains("exporting to image") {
            say("building the hub image — exporting".into());
        }
    })
    .context("hub image build failed")?;

    say("restarting the hub on the new image…".into());
    // `up -d` alone is a no-op when the tag didn't change: the container would
    // keep running the old image.
    ssh(
        cfg,
        &hb.docker_cmd(&format!(
            "cd {} && docker compose up -d --force-recreate",
            shell_quote(&hb.stack_dir)
        )),
    )?;

    let health = format!("{}/healthz", cfg.hub_url.trim_end_matches('/'));
    let mut up = false;
    for _ in 0..45 {
        if matches!(curl_code(&health), Ok(200)) {
            up = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
    if !up {
        bail!("the hub did not come back on {health} after the rebuild");
    }
    let now = hub_get(cfg, "/api/hub/games")?
        .get("idxVersion")
        .and_then(Value::as_u64)
        .unwrap_or(1);
    if now != local_idx {
        bail!("the rebuilt hub still reads index v{now} (expected v{local_idx}) — are {} the current sources?", hb.source_dir.display());
    }

    // Other published games: re-send their books in the new format.
    let names: Vec<String> = hub_games
        .get("games")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|g| g.get("name").and_then(Value::as_str))
                .filter(|n| *n != publishing)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let mut resent = Vec::new();
    let mut missing = Vec::new();
    for name in names {
        match state.registry.get(&name) {
            Some(g) if state.books.is_imported(&g) => {
                say(format!("re-sending the books of {name} in the new format…"));
                let remote = format!("{}/{}/cache/", cfg.remote_dir.trim_end_matches('/'), name);
                rsync(&state.books.cache_dir_for(&g), &format!("{}:{remote}", cfg.ssh_target), &[])?;
                resent.push(name);
            }
            _ => missing.push(name),
        }
    }
    if !resent.is_empty() {
        hub_post(cfg, "/api/hub/reload", &json!({}))?;
    }

    let mut detail = format!("hub rebuilt (index v{local_idx})");
    if !resent.is_empty() {
        detail.push_str(&format!(" · books re-sent: {}", resent.join(", ")));
    }
    if !missing.is_empty() {
        detail.push_str(&format!(
            " · not on this Mac (re-publish them from where they live): {}",
            missing.join(", ")
        ));
    }
    Ok(detail)
}

/// `ssh` for long commands: every output line goes to `on_line` as it
/// arrives, and the tail of the output is the error when it fails.
fn ssh_streaming(cfg: &DeployConfig, command: &str, mut on_line: impl FnMut(&str)) -> Result<()> {
    use std::io::{BufRead, BufReader};
    use std::process::Stdio;
    let mut child = Command::new("ssh")
        .args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=10", "-o", "ServerAliveInterval=30"])
        .arg(&cfg.ssh_target)
        .arg(format!("{command} 2>&1"))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("could not start ssh")?;
    let mut tail: std::collections::VecDeque<String> = std::collections::VecDeque::new();
    if let Some(out) = child.stdout.take() {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            on_line(&line);
            tail.push_back(line);
            if tail.len() > 12 {
                tail.pop_front();
            }
        }
    }
    let status = child.wait().context("ssh did not finish")?;
    if !status.success() {
        let mut err = String::new();
        if let Some(mut e) = child.stderr.take() {
            use std::io::Read;
            let _ = e.read_to_string(&mut err);
        }
        let tail: Vec<String> = tail.into_iter().collect();
        bail!("{}{}", err.trim(), if tail.is_empty() { String::new() } else { format!("\n{}", tail.join("\n")) });
    }
    Ok(())
}

fn iso_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    time::OffsetDateTime::from_unix_timestamp(secs as i64)
        .ok()
        .and_then(|t| {
            t.format(&time::format_description::well_known::Rfc3339)
                .ok()
        })
        .unwrap_or_default()
}

fn ssh(cfg: &DeployConfig, command: &str) -> Result<()> {
    ssh_out(cfg, command).map(|_| ())
}

/// `ssh`, returning what the command printed.
fn ssh_out(cfg: &DeployConfig, command: &str) -> Result<String> {
    let out = Command::new("ssh")
        .args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=10"])
        .arg(&cfg.ssh_target)
        .arg(command)
        .output()
        .context("could not start ssh")?;
    if !out.status.success() {
        bail!(
            "ssh {} : {}",
            cfg.ssh_target,
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Returns a one-line summary of what actually moved.
fn rsync(local: &Path, remote: &str, extra: &[&str]) -> Result<String> {
    let mut src = local.to_string_lossy().to_string();
    if !src.ends_with('/') {
        src.push('/');
    }
    let out = Command::new("rsync")
        .arg("-a")
        .arg("--itemize-changes")
        .args(extra)
        .args(["-e", "ssh -o BatchMode=yes -o ConnectTimeout=10"])
        .arg(&src)
        .arg(remote)
        .output()
        .context("could not start rsync")?;
    if !out.status.success() {
        bail!("rsync : {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    let n = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .count();
    Ok(if n == 0 {
        "already up to date".into()
    } else {
        format!("{n} file(s)")
    })
}

// ─────────────────────────────── HTTP ───────────────────────────────

fn curl_json(args: &[String]) -> Result<Value> {
    // -sS et non -s : sans le second S, une erreur de connexion sort avec un
    // code non nul ET un stderr vide, ce qui donne un message illisible.
    let out = Command::new("curl")
        .args(["-sS", "--max-time", "60"])
        .args(args)
        .output()
        .context("could not start curl")?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let code = out.status.code().unwrap_or(-1);
        bail!(
            "curl failed (code {code}): {}",
            if err.is_empty() {
                "no message. From the installed app this is usually macOS \
                 blocking local network access: System Settings → Privacy \
                 & Security → Local Network → allow SlotLab."
                    .to_string()
            } else {
                err
            }
        );
    }
    let body = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(&body)
        .with_context(|| format!("unreadable response: {}", body.chars().take(200).collect::<String>()))
}

fn curl_code(url: &str) -> Result<u32> {
    let out = Command::new("curl")
        .args(["-s", "-o", "/dev/null", "-w", "%{http_code}", "--max-time", "15", url])
        .output()
        .context("could not start curl")?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().parse().unwrap_or(0))
}

fn hub_get(cfg: &DeployConfig, path: &str) -> Result<Value> {
    curl_json(&[
        "-H".into(),
        format!("x-slotlab-token: {}", cfg.admin_token),
        format!("{}{}", cfg.hub_url.trim_end_matches('/'), path),
    ])
}

fn hub_post(cfg: &DeployConfig, path: &str, body: &Value) -> Result<Value> {
    curl_json(&[
        "-X".into(),
        "POST".into(),
        "-H".into(),
        format!("x-slotlab-token: {}", cfg.admin_token),
        "-H".into(),
        "Content-Type: application/json".into(),
        "--data".into(),
        body.to_string(),
        format!("{}{}", cfg.hub_url.trim_end_matches('/'), path),
    ])
}

fn cf_api(cfg: &DeployConfig, method: &str, path: &str, body: Option<&Value>) -> Result<Value> {
    let mut args = vec![
        "-X".to_string(),
        method.to_string(),
        "-H".into(),
        format!("Authorization: Bearer {}", cfg.cf_token),
        "-H".into(),
        "Content-Type: application/json".into(),
    ];
    if let Some(b) = body {
        args.push("--data".into());
        args.push(b.to_string());
    }
    args.push(format!("https://api.cloudflare.com/client/v4{path}"));
    let v = curl_json(&args)?;
    if v.get("success").and_then(Value::as_bool) != Some(true) {
        let msg = v
            .get("errors")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|e| e.get("message").and_then(Value::as_str))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| v.to_string());
        bail!("Cloudflare: {msg}");
    }
    Ok(v.get("result").cloned().unwrap_or(Value::Null))
}

fn cloudflare_route(cfg: &DeployConfig, sub: &str, host: &str) -> Result<String> {
    for (label, v) in [
        ("account ID", &cfg.cf_account_id),
        ("tunnel ID", &cfg.cf_tunnel_id),
        ("zone ID", &cfg.cf_zone_id),
        ("tunnel target", &cfg.tunnel_target),
    ] {
        if v.trim().is_empty() {
            bail!("incomplete Cloudflare settings: {label} missing");
        }
    }
    let conf_path = format!(
        "/accounts/{}/cfd_tunnel/{}/configurations",
        cfg.cf_account_id, cfg.cf_tunnel_id
    );
    let current = cf_api(cfg, "GET", &conf_path, None)?;
    let mut config = current
        .get("config")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let ingress = config
        .get("ingress")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    // The catch-all (the rule with no hostname) must stay last, otherwise
    // it swallows every rule placed after it.
    let mut named: Vec<Value> = ingress
        .iter()
        .filter(|r| {
            r.get("hostname").and_then(Value::as_str).is_some()
                && r.get("hostname").and_then(Value::as_str) != Some(host)
        })
        .cloned()
        .collect();
    let catch: Vec<Value> = ingress
        .iter()
        .filter(|r| r.get("hostname").and_then(Value::as_str).is_none())
        .cloned()
        .collect();
    named.push(json!({ "hostname": host, "service": cfg.tunnel_target }));
    named.extend(if catch.is_empty() {
        vec![json!({ "service": "http_status:404" })]
    } else {
        catch
    });
    config["ingress"] = Value::Array(named);
    cf_api(cfg, "PUT", &conf_path, Some(&json!({ "config": config })))?;

    // CNAME towards the tunnel, proxied — unproxied it resolves to nothing
    // reachable from outside.
    let existing = cf_api(
        cfg,
        "GET",
        &format!("/zones/{}/dns_records?name={host}", cfg.cf_zone_id),
        None,
    )?;
    let already = existing
        .as_array()
        .map(|a| {
            a.iter().any(|r| {
                r.get("content")
                    .and_then(Value::as_str)
                    .is_some_and(|c| c.contains(&cfg.cf_tunnel_id))
            })
        })
        .unwrap_or(false);
    if already {
        return Ok(format!("route updated, DNS {host} already in place"));
    }
    cf_api(
        cfg,
        "POST",
        &format!("/zones/{}/dns_records", cfg.cf_zone_id),
        Some(&json!({
            "type": "CNAME",
            "name": sub,
            "content": format!("{}.cfargotunnel.com", cfg.cf_tunnel_id),
            "proxied": true,
            "ttl": 1,
        })),
    )?;
    Ok(format!("route + DNS created for {host}"))
}
