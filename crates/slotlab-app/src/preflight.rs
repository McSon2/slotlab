//! The pre-upload checklist for one game: the text rules of
//! `slotlab-core::preflight` plus the checks only this process can make —
//! the ones that need the LUT, the books index, or both.
//!
//! Everything is computed on demand: the answer has to describe the files as
//! they are right now, seconds before they go up.

use crate::api::routes::ApiError;
use crate::registry::LoadedGame;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use slotlab_core::preflight::{self, Finding, Roots, Scope, Severity, Status};
use std::collections::BTreeSet;

pub fn router() -> Router<AppState> {
    Router::new().route("/api/ui/games/{name}/preflight", get(preflight_report))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Report {
    game: String,
    /// Roots the rules actually ran against, so a skip can be explained.
    app_dir: Option<String>,
    build_dir: Option<String>,
    fail_count: usize,
    warn_count: usize,
    skip_count: usize,
    passed: bool,
    findings: Vec<Finding>,
}

/// A finding produced here rather than by a text rule.
#[allow(clippy::too_many_arguments)]
fn computed(
    id: &str,
    label: &str,
    explain: &str,
    doc: &str,
    severity: Severity,
    scope: Scope,
    ok: bool,
    detail: String,
    hits: Vec<preflight::Hit>,
) -> Finding {
    Finding {
        id: id.into(),
        label: label.into(),
        explain: explain.into(),
        doc: doc.into(),
        severity,
        status: if ok {
            Status::Pass
        } else {
            match severity {
                Severity::Fail => Status::Fail,
                Severity::Warn => Status::Warn,
                Severity::Info => Status::Pass,
            }
        },
        scope,
        detail,
        hits,
        extra_hits: 0,
    }
}

fn skipped(id: &str, label: &str, doc: &str, scope: Scope, why: &str) -> Finding {
    Finding {
        id: id.into(),
        label: label.into(),
        explain: String::new(),
        doc: doc.into(),
        severity: Severity::Warn,
        status: Status::Skip,
        scope,
        detail: why.into(),
        hits: Vec::new(),
        extra_hits: 0,
    }
}

fn hit(file: &str, text: String) -> preflight::Hit {
    preflight::Hit {
        file: file.to_string(),
        line: 0,
        text,
    }
}

async fn preflight_report(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<Report>, ApiError> {
    let game = state
        .registry
        .get(&name)
        .ok_or_else(|| ApiError::not_found(format!("unknown game: {name}")))?;

    let app_root = game.entry.app_dir.clone();
    let app_src = app_root.as_ref().map(|p| p.join("src"));
    let packages = game.frontend_root().map(|p| p.join("packages"));
    let build = game.entry.build_dir.clone();
    let roots = Roots {
        app_src: app_src.clone().filter(|p| p.is_dir()),
        app_root: app_root.clone().filter(|p| p.is_dir()),
        packages: packages.filter(|p| p.is_dir()),
        build: build.clone().filter(|p| p.is_dir()),
        publish: Some(game.entry.publish_dir.clone()),
    };

    let game_for_rules = game.clone();
    let state_for_rules = state.clone();
    let mut findings = tokio::task::spawn_blocking(move || {
        let mut out = preflight::run(&roots, preflight::RULES);
        out.extend(source_checks(&roots));
        out.extend(publish_checks(&state_for_rules, &game_for_rules));
        out
    })
    .await
    .map_err(|e| ApiError::internal(format!("preflight failed: {e}")))?;

    // Failures first, then warnings, then everything that passed — the report
    // is read top-down under time pressure.
    findings.sort_by_key(|f| match f.status {
        Status::Fail => 0,
        Status::Warn => 1,
        Status::Skip => 2,
        Status::Pass => 3,
    });

    let fail_count = findings.iter().filter(|f| f.status == Status::Fail).count();
    let warn_count = findings.iter().filter(|f| f.status == Status::Warn).count();
    let skip_count = findings.iter().filter(|f| f.status == Status::Skip).count();
    Ok(Json(Report {
        game: name,
        app_dir: app_root.map(|p| p.display().to_string()),
        build_dir: build.map(|p| p.display().to_string()),
        fail_count,
        warn_count,
        skip_count,
        passed: fail_count == 0,
        findings,
    }))
}

/// Checks over the sources that are a comparison, not a grep.
fn source_checks(roots: &Roots) -> Vec<Finding> {
    let mut out = Vec::new();

    // Ghost assets: a `key=` the loader was never given.
    match &roots.app_src {
        Some(src) => {
            let (missing, unused) = preflight::ghost_assets(src);
            out.push(computed(
                "ghost_assets",
                "No ghost assets",
                "A component asks the loader for a key that `assets.ts` never declares: \
                 the moment plays with nothing on screen, and nothing errors.",
                "§27.5",
                Severity::Fail,
                Scope::AppSrc,
                missing.is_empty(),
                if missing.is_empty() {
                    "every key= is declared".into()
                } else {
                    format!("{} undeclared key(s)", missing.len())
                },
                missing.iter().map(|k| hit("assets.ts", k.clone())).collect(),
            ));
            out.push(computed(
                "unused_assets",
                "Declared assets actually used",
                "Declared but never referenced: the build carries it for nothing.",
                "§27.6",
                // Warn, not Info: an Info rule reads "pass" even when it hits,
                // which showed "pass — 10 declared and unused".
                Severity::Warn,
                Scope::AppSrc,
                unused.is_empty(),
                if unused.is_empty() {
                    "every declared asset is referenced".into()
                } else {
                    format!("{} declared and unused", unused.len())
                },
                unused
                    .iter()
                    .take(20)
                    .map(|k| hit("assets.ts", k.clone()))
                    .collect(),
            ));
        }
        None => out.push(skipped(
            "ghost_assets",
            "No ghost assets",
            "§27.5",
            Scope::AppSrc,
            "no frontend sources registered for this game",
        )),
    }

    // Version: package.json and the version the player reads.
    match &roots.app_root {
        Some(app) => {
            let (pkg, shown) = preflight::version_pair(app);
            let ok = match (&pkg, &shown) {
                (Some(a), Some(b)) => a == b,
                // A game with no GameVersion on screen is not a mismatch.
                _ => shown.is_none(),
            };
            out.push(computed(
                "version_sync",
                "Version synchronised",
                "The dashboard shows package.json, the player sees <GameVersion />. \
                 Two numbers that disagree make a bug report unattributable.",
                "§38.10",
                Severity::Warn,
                Scope::AppRoot,
                ok,
                format!(
                    "package.json {} · screen {}",
                    pkg.clone().unwrap_or_else(|| "—".into()),
                    shown.clone().unwrap_or_else(|| "—".into())
                ),
                Vec::new(),
            ));
        }
        None => out.push(skipped(
            "version_sync",
            "Version synchronised",
            "§38.10",
            Scope::AppRoot,
            "no frontend sources registered for this game",
        )),
    }
    out
}

/// Checks that read publish_files: the LUT against the books, and the folder
/// against what index.json claims.
fn publish_checks(state: &AppState, game: &LoadedGame) -> Vec<Finding> {
    let mut out = Vec::new();
    let dir = &game.entry.publish_dir;

    // 1. Files in the folder that index.json does not name. The classic case
    //    is a rename of the bet modes leaving the previous set behind — and
    //    the whole folder gets uploaded.
    let mut referenced: BTreeSet<String> = BTreeSet::new();
    referenced.insert("index.json".to_string());
    for m in &game.modes {
        referenced.insert(m.config.events.clone());
        referenced.insert(m.config.weights.clone());
    }
    let mut orphans: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let n = e.file_name().to_string_lossy().to_string();
            if n.starts_with('.') {
                continue;
            }
            if !referenced.contains(&n) {
                orphans.push(n);
            }
        }
    }
    orphans.sort();
    out.push(computed(
        "orphan_publish_files",
        "No orphan file in publish_files",
        "A file index.json does not name still goes up with the rest — usually the \
         leftovers of a previous bet-mode naming.",
        "§29",
        Severity::Warn,
        Scope::Publish,
        orphans.is_empty(),
        if orphans.is_empty() {
            format!("{} file(s), all referenced", referenced.len())
        } else {
            format!("{} orphan(s)", orphans.len())
        },
        orphans.iter().take(12).map(|n| hit(n, String::new())).collect(),
    ));

    // 2. LUT ↔ books, by id. This is `verify_publish.py`: the payout the RGS
    //    will pay (LUT column 3) against the payout the book actually plays.
    //    A mismatch means the player watches one round and is paid another.
    let mut mismatches: Vec<preflight::Hit> = Vec::new();
    let mut missing_books = 0usize;
    let mut compared = 0usize;
    let mut biggest_line = 0u32;
    let mut biggest_line_mode = String::new();
    let mut unsorted_modes: Vec<String> = Vec::new();
    let mut imported_modes = 0usize;

    for m in &game.modes {
        let Ok(books) = state.books.mode_books(game, &m.config.name) else {
            continue;
        };
        imported_modes += 1;
        let idx = &books.index;
        for b in &idx.books {
            if b.len > biggest_line {
                biggest_line = b.len;
                biggest_line_mode = m.config.name.clone();
            }
        }
        if !idx.books.windows(2).all(|w| w[0].sim_id <= w[1].sim_id) {
            unsorted_modes.push(m.config.name.clone());
        }
        for row in &m.lut.rows {
            match idx.find(row.sim_id) {
                Some(rec) => {
                    compared += 1;
                    if rec.payout_x100 != row.payout_x100 && mismatches.len() < 12 {
                        mismatches.push(preflight::Hit {
                            file: m.config.events.clone(),
                            line: row.sim_id as usize,
                            text: format!(
                                "LUT pays {:.2}x, book #{} plays {:.2}x",
                                row.payout_x100 as f64 / 100.0,
                                row.sim_id,
                                rec.payout_x100 as f64 / 100.0
                            ),
                        });
                    }
                }
                None => missing_books += 1,
            }
        }
    }

    if imported_modes == 0 {
        out.push(skipped(
            "lut_book_match",
            "LUT payouts match the books",
            "§29",
            Scope::Publish,
            "books not imported yet — Games tab → Import books",
        ));
    } else {
        let ok = mismatches.is_empty() && missing_books == 0;
        let mut hits = mismatches;
        if missing_books > 0 {
            hits.push(hit(
                "index.json",
                format!("{missing_books} LUT row(s) point at a sim_id no book carries"),
            ));
        }
        out.push(computed(
            "lut_book_match",
            "LUT payouts match the books",
            "The RGS pays the LUT and plays the book: when they disagree, the player \
             watches one round and is credited another.",
            "§29",
            Severity::Fail,
            Scope::Publish,
            ok,
            format!("{compared} row(s) compared over {imported_modes} imported mode(s)"),
            hits,
        ));

        out.push(computed(
            "book_line_size",
            "Book lines under 500 KB",
            "A single oversized book line is refused by the RGS ingest.",
            "§29",
            Severity::Warn,
            Scope::Publish,
            biggest_line < 500_000,
            format!(
                "largest line {} KB ({})",
                biggest_line / 1024,
                if biggest_line_mode.is_empty() {
                    "—"
                } else {
                    &biggest_line_mode
                }
            ),
            Vec::new(),
        ));

        out.push(computed(
            "books_index_sorted",
            "Books indexed in id order",
            "Out of order, every book lookup degrades to a linear scan of the whole \
             mode — silently, and on every spin.",
            "",
            Severity::Warn,
            Scope::Publish,
            unsorted_modes.is_empty(),
            if unsorted_modes.is_empty() {
                "every mode is sorted".into()
            } else {
                format!("unsorted: {}", unsorted_modes.join(", "))
            },
            Vec::new(),
        ));
    }

    // 3. Book event types the frontend never names (§36-19: every event of a
    //    book owes the player a feedback).
    match (&game.entry.app_dir, imported_modes > 0) {
        (Some(app), true) => {
            let mut types: BTreeSet<String> = BTreeSet::new();
            for m in &game.modes {
                if let Ok(books) = state.books.mode_books(game, &m.config.name) {
                    types.extend(books.index.event_type_names.iter().cloned());
                }
            }
            let all: Vec<String> = types.into_iter().collect();
            let uncovered = preflight::uncovered_events(&app.join("src"), &all);
            out.push(computed(
                "event_coverage",
                "Every book event is handled",
                "An event no source file names is a moment the game plays in silence — \
                 the literal complaint is \"nothing happens\".",
                "§36-19",
                Severity::Fail,
                Scope::AppSrc,
                uncovered.is_empty(),
                format!(
                    "{}/{} event type(s) named in the sources",
                    all.len() - uncovered.len(),
                    all.len()
                ),
                uncovered
                    .iter()
                    .map(|t| hit("book events", t.clone()))
                    .collect(),
            ));
        }
        _ => out.push(skipped(
            "event_coverage",
            "Every book event is handled",
            "§36-19",
            Scope::AppSrc,
            "needs both the frontend sources and an import of the books",
        )),
    }

    out
}
