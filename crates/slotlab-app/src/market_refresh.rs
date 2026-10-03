//! Market reference, rebuilt every day from what the market PLAYS NOW (client,
//! 2026-10-03: « il faut que ce soit les plus joués sur les 30 derniers jours,
//! sinon ça n'a pas de sens »).
//!
//! Same selection and figures as `scripts/market/fetch_market.py`: the 400 most
//! played games of the last 30 days (StakeCruncher, turnover), instant games
//! apart, then the slots from the top down until they make 90 % of the slot
//! turnover — whatever their studio — and every active mode of each, read from
//! its certified lookup table. Distributions are cached in the script's format
//! (`market/cache/dist`): a day's refresh only downloads the games and versions
//! that are new. The result is written to `market/market.json` and swapped in
//! live (`slotlab_core::market::install`); the reference built into SlotLab is
//! the offline fallback.
//!
//! HTTP goes through `curl`, like `deploy.rs`: no second TLS stack in the
//! binary. Polite: 0.5 s between lookup tables, 20 s back-off on HTTP 429.

use crate::persist;
use anyhow::{bail, Context};
use serde::Serialize;
use serde_json::{json, Value};
use slotlab_core::market::{self, Fig, MarketInfo, MarketMode};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

const API: &str = "https://stakecruncher.com/tracker-api/engine";
const REFERER: &str = "https://stakecruncher.com/slots-tracker";
const UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0 Safari/537.36";
/// Games read from the 30-day ranking.
const RANKED: usize = 400;
/// Share of the slot turnover the reference covers, from the most played down.
const COVERAGE: f64 = 0.90;
/// Not slots: a studio's hit can be one, and their dozens of modes would swamp the rest.
const INSTANT: [&str; 10] = ["keno", "dice", "plinko", "mines", "limbo", "crash", "roulette", "blackjack", "baccarat", "hilo"];
/// After a failed attempt, the next one waits this long (seconds).
const RETRY_AFTER: u64 = 6 * 3600;

/// Where the refresh stands — `GET /api/ui/market`.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub running: bool,
    pub step: String,
    pub done: usize,
    pub total: usize,
    /// Lookup tables downloaded by this refresh (the others came from the cache).
    pub downloaded: usize,
    pub last_error: Option<String>,
    /// Last attempt, Unix seconds.
    pub last_attempt: Option<u64>,
}

static RUNNING: AtomicBool = AtomicBool::new(false);
static STATUS: Mutex<Option<Status>> = Mutex::new(None);

pub fn status() -> Status {
    STATUS.lock().ok().and_then(|s| s.clone()).unwrap_or_default()
}

fn set(f: impl FnOnce(&mut Status)) {
    if let Ok(mut g) = STATUS.lock() {
        f(g.get_or_insert_with(Status::default));
    }
}

pub fn dir() -> PathBuf {
    persist::app_dir().join("market")
}

fn today() -> String {
    time::OffsetDateTime::now_utc().date().to_string()
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// The market saved by the last refresh, swapped in if it is newer than the one
/// built into SlotLab.
pub fn load_saved() {
    let path = dir().join("market.json");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return;
    };
    let fetched = serde_json::from_str::<Value>(&text).ok().and_then(|v| v["fetched"].as_str().map(str::to_owned)).unwrap_or_default();
    if fetched.as_str() < market::embedded_fetched() {
        return;
    }
    match market::install(&text) {
        Ok(info) => tracing::info!("market: {} modes of {} slots, ranking of {}", info.modes, info.slots, info.fetched),
        Err(e) => tracing::warn!("market: {} ignored — {e}", path.display()),
    }
}

/// Loads the saved market, then refreshes it once a day while SlotLab runs
/// (UI server only: the hub never shows the market).
pub fn start() {
    load_saved();
    tokio::spawn(async {
        loop {
            if stale() {
                trigger();
            }
            tokio::time::sleep(Duration::from_secs(3600)).await;
        }
    });
}

/// The ranking in use is older than today, and no attempt failed in the last hours.
fn stale() -> bool {
    if market::info().fetched >= today().as_str() {
        return false;
    }
    status().last_attempt.map_or(true, |t| now_secs().saturating_sub(t) >= RETRY_AFTER)
}

/// Starts a refresh in the background — `false` if one is already running.
pub fn trigger() -> bool {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return false;
    }
    set(|s| {
        *s = Status { running: true, step: "ranking".into(), last_attempt: Some(now_secs()), ..Status::default() };
    });
    std::thread::spawn(|| {
        let res = refresh();
        set(|s| {
            s.running = false;
            match &res {
                Ok(info) => {
                    s.step = format!("{} modes of {} slots, ranking of {}", info.modes, info.slots, info.fetched);
                    s.last_error = None;
                }
                Err(e) => {
                    s.step = "failed".into();
                    s.last_error = Some(format!("{e:#}"));
                }
            }
        });
        match res {
            Ok(info) => tracing::info!("market refreshed: {} modes of {} slots ({})", info.modes, info.slots, info.fetched),
            Err(e) => tracing::warn!("market refresh failed: {e:#}"),
        }
        RUNNING.store(false, Ordering::SeqCst);
    });
    true
}

/// `curl` with the tracker's browser headers; retried on HTTP 429. Returns the status code.
fn curl(url: &str, referer: &str, out: &Path, headers: Option<&Path>) -> anyhow::Result<String> {
    let mut code = String::new();
    for _ in 0..6 {
        let mut cmd = Command::new("curl");
        cmd.args(["-sL", "--max-time", "180", "-A", UA, "-H"]).arg(format!("Referer: {referer}")).arg("-o").arg(out);
        if let Some(h) = headers {
            cmd.arg("-D").arg(h);
        }
        cmd.args(["-w", "%{http_code}"]).arg(url);
        let o = cmd.output().context("curl")?;
        code = String::from_utf8_lossy(&o.stdout).trim().to_string();
        if code != "429" {
            break;
        }
        std::thread::sleep(Duration::from_secs(20));
    }
    Ok(code)
}

fn get_json(url: &str, referer: &str, tmp: &Path) -> anyhow::Result<Value> {
    let code = curl(url, referer, tmp, None)?;
    if code != "200" {
        bail!("HTTP {code} on {url}");
    }
    serde_json::from_slice(&std::fs::read(tmp)?).with_context(|| format!("JSON of {url}"))
}

/// A mode's distribution: payout (×100) → weight, as the script caches it.
struct Dist {
    cost: f64,
    payouts: Vec<f64>,
    weights: Vec<f64>,
}

fn read_cached(path: &Path) -> Option<Dist> {
    let mut text = String::new();
    flate2::read::GzDecoder::new(std::fs::File::open(path).ok()?).read_to_string(&mut text).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    let nums = |k: &str| v[k].as_array().map(|a| a.iter().filter_map(Value::as_f64).collect::<Vec<f64>>());
    let (payouts, weights) = (nums("payouts")?, nums("weights")?);
    (payouts.len() == weights.len()).then(|| Dist { cost: v["cost"].as_f64().unwrap_or(1.0), payouts, weights })
}

/// Downloads a lookup table and aggregates it by payout; `None` if the tracker refuses.
fn download(slug: &str, version: i64, mode: &str, cache: &Path) -> anyhow::Result<Option<Dist>> {
    let (hf, bf) = (cache.join("h.txt"), cache.join("lut.bin"));
    let url = format!("{API}/verifier/lookup-table?slug={slug}&version={version}&mode={mode}&_lv=5");
    let code = curl(&url, &format!("{REFERER}/verifier"), &bf, Some(&hf))?;
    if code != "200" {
        tracing::warn!("market: {slug} {mode} HTTP {code}");
        return Ok(None);
    }
    let hdr = std::fs::read_to_string(&hf).unwrap_or_default().to_lowercase();
    let cost = hdr
        .lines()
        .find_map(|l| l.strip_prefix("x-lookup-cost-multiplier:"))
        .and_then(|v| v.trim().parse::<f64>().ok())
        .unwrap_or(1.0);
    let mut raw = std::fs::read(&bf)?;
    if hdr.contains("x-lookup-encoding: gzip") {
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(&raw[..]).read_to_end(&mut out)?;
        raw = out;
    }
    let mut agg: BTreeMap<u64, u128> = BTreeMap::new();
    for line in String::from_utf8_lossy(&raw).lines() {
        let p: Vec<&str> = line.trim().split(',').collect();
        if p.len() >= 3 {
            if let (Ok(w), Ok(x)) = (p[1].parse::<u128>(), p[2].parse::<u64>()) {
                *agg.entry(x).or_default() += w;
            }
        }
    }
    let payouts: Vec<u64> = agg.keys().copied().collect();
    let weights: Vec<u128> = agg.values().copied().collect();
    // Same file as the script: integers when they fit, so both read each other's cache.
    let num = |w: u128| u64::try_from(w).map_or_else(|_| json!(w as f64), |v| json!(v));
    let rec = json!({ "cost": cost, "payouts": payouts, "weights": weights.iter().map(|&w| num(w)).collect::<Vec<_>>() });
    let path = cache.join("dist").join(format!("{slug}__v{version}__{mode}.json.gz"));
    let mut gz = flate2::write::GzEncoder::new(std::fs::File::create(&path)?, flate2::Compression::default());
    gz.write_all(rec.to_string().as_bytes())?;
    gz.finish()?;
    Ok(Some(Dist { cost, payouts: payouts.iter().map(|&x| x as f64).collect(), weights: weights.iter().map(|&w| w as f64).collect() }))
}

/// Six significant digits, as the script writes them.
fn rnd(v: f64) -> f64 {
    format!("{v:.5e}").parse().unwrap_or(v)
}

fn round_fig(f: Fig) -> Fig {
    let mut v = serde_json::to_value(f).unwrap_or_default();
    if let Some(o) = v.as_object_mut() {
        for x in o.values_mut() {
            if let Some(n) = x.as_f64() {
                *x = json!(rnd(n));
            }
        }
    }
    serde_json::from_value(v).unwrap_or(f)
}

fn refresh() -> anyhow::Result<MarketInfo> {
    let dir = dir();
    let cache = dir.join("cache");
    std::fs::create_dir_all(cache.join("dist"))?;
    let tmp = cache.join("api.json");

    let mut ranked: Vec<Value> = Vec::new();
    for off in (0..RANKED).step_by(100) {
        let url = format!("{API}/games?sort=turnover_30d&limit={}&offset={off}", (RANKED - off).min(100));
        let page = get_json(&url, REFERER, &tmp)?;
        ranked.extend(page["items"].as_array().cloned().unwrap_or_default());
    }
    if ranked.len() < 100 {
        bail!("ranking: {} games only", ranked.len());
    }
    set(|s| s.step = "catalog".into());
    let catalog = get_json(&format!("{API}/verifier/catalog"), &format!("{REFERER}/verifier"), &tmp)?;
    let cat: HashMap<String, Value> = catalog["games"]
        .as_array()
        .context("catalog: no games")?
        .iter()
        .filter_map(|g| Some((g["slug"].as_str()?.to_string(), g.clone())))
        .collect();

    let turn = |it: &Value| it["turnover_past_30d"].as_f64().unwrap_or(0.0);
    let total = ranked.iter().map(turn).sum::<f64>().max(1.0);
    let catalog_slug = |it: &Value| -> Option<String> {
        let (s, t) = (it["slug"].as_str()?, it["team_slug"].as_str().unwrap_or(""));
        if cat.contains_key(s) {
            return Some(s.to_string());
        }
        let short = s.strip_prefix(t).and_then(|r| r.strip_prefix('-'))?;
        cat.contains_key(short).then(|| short.to_string())
    };
    let version = |g: &Value| -> i64 {
        g["activeVersion"].as_i64().unwrap_or_else(|| {
            g["modes"].as_array().map_or(0, |ms| ms.iter().filter_map(|m| m["version"].as_i64()).max().unwrap_or(0))
        })
    };
    let modes_of = |g: &Value| -> Vec<Value> {
        let v = version(g);
        g["modes"]
            .as_array()
            .map(|ms| ms.iter().filter(|m| m["version"].as_i64() == Some(v) && m["active"].as_bool() != Some(false)).cloned().collect())
            .unwrap_or_default()
    };

    // The slots, most played first, until `COVERAGE` of their turnover.
    let mut slots: Vec<(u32, &Value, String)> = Vec::new();
    let mut excluded: Vec<String> = Vec::new();
    for (i, it) in ranked.iter().enumerate() {
        let Some(slug) = catalog_slug(it) else {
            continue;
        };
        let g = &cat[&slug];
        let all_modes = g["modes"].as_array().map_or(0, |ms| ms.iter().filter(|m| m["version"].as_i64() == Some(version(g))).count());
        if INSTANT.iter().any(|k| slug.contains(k)) || all_modes > 40 {
            excluded.push(format!("{} ({})", it["name"].as_str().unwrap_or("").trim(), it["team_name"].as_str().unwrap_or("")));
            continue;
        }
        slots.push(((i + 1) as u32, it, slug));
    }
    slots.sort_by(|a, b| turn(b.1).total_cmp(&turn(a.1)));
    let slot_total = slots.iter().map(|s| turn(s.1)).sum::<f64>().max(1.0);
    let mut chosen = Vec::new();
    let mut acc = 0.0;
    for s in slots {
        if acc >= COVERAGE * slot_total {
            break;
        }
        acc += turn(s.1);
        chosen.push(s);
    }

    let jobs: Vec<(u32, &Value, String, i64, String)> = chosen
        .iter()
        .flat_map(|(rank, it, slug)| {
            let g = &cat[slug];
            let v = version(g);
            modes_of(g).into_iter().filter_map(move |m| Some((*rank, *it, slug.clone(), v, m["name"].as_str()?.to_string())))
        })
        .collect();
    set(|s| {
        s.step = "lookup tables".into();
        s.total = jobs.len();
    });

    let mut rows: Vec<MarketMode> = Vec::new();
    for (k, (rank, it, slug, v, mode)) in jobs.iter().enumerate() {
        let path = cache.join("dist").join(format!("{slug}__v{v}__{mode}.json.gz"));
        let dist = match read_cached(&path) {
            Some(d) => Some(d),
            None => {
                let d = download(slug, *v, mode, &cache)?;
                set(|s| s.downloaded += 1);
                std::thread::sleep(Duration::from_millis(500));
                d
            }
        };
        set(|s| s.done = k + 1);
        let Some(d) = dist else {
            continue;
        };
        let pairs: Vec<(f64, f64)> = d.payouts.iter().zip(&d.weights).map(|(&x, &w)| (x / 100.0, w)).filter(|p| p.1 > 0.0).collect();
        if pairs.is_empty() {
            continue;
        }
        let fig = market::figures_of(pairs, d.cost);
        rows.push(MarketMode {
            rank: *rank,
            slug: slug.clone(),
            name: it["name"].as_str().unwrap_or("").trim().to_string(),
            studio: it["team_name"].as_str().unwrap_or("").to_string(),
            share: rnd(turn(it) / total),
            mode: mode.clone(),
            cost: d.cost,
            kind: market::kind_of(d.cost, fig.zero).to_string(),
            fig: round_fig(fig),
        });
    }

    let studios: HashSet<&str> = chosen.iter().filter_map(|c| c.1["team_name"].as_str()).collect();
    let n_slots = rows.iter().map(|r| r.slug.as_str()).collect::<HashSet<_>>().len();
    let out = json!({
        "source": "stakecruncher.com — certified lookup tables of Stake Engine games",
        "rule": format!("the most played slots of the last 30 days (turnover), up to {:.0}% of the slot turnover of the top {RANKED}", COVERAGE * 100.0),
        "studios": studios.len(),
        "slots": n_slots,
        "coverage": rnd(chosen.iter().map(|c| turn(c.1)).sum::<f64>() / total),
        "excluded": excluded,
        "fetched": today(),
        "modes": rows,
    });
    let text = serde_json::to_string(&out)?;
    let info = market::install(&text).map_err(anyhow::Error::msg)?;
    let file = dir.join("market.json");
    let part = dir.join("market.json.part");
    std::fs::write(&part, &text)?;
    std::fs::rename(&part, &file)?;
    Ok(info)
}
