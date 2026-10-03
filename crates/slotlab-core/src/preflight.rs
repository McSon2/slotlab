//! The pre-upload checklist, run against the files instead of by hand.
//!
//! ── Why a grep engine belongs in a math tool ────────────────────────────────
//! The review that decides whether a game is published is dominated by the
//! VISUAL: the math earns no points, it is only ever eliminatory. Every refusal
//! the studio has actually received is either a screen decision or a one-line
//! deviation in the frontend — a renderer preference, a bet-level filter, a
//! missing modal, a word that is restricted in Social Mode. All of them are
//! visible in the source or in the build, and all of them were found, in
//! practice, by running the same thirty greps by hand before every upload.
//!
//! So this module is that list, executed: each rule says WHERE to look, WHAT
//! pattern to look for, and whether finding it is the failure or not finding it
//! is. A rule whose target does not exist is SKIPPED and says so — a check that
//! silently passes because the folder was missing is worse than no check.
//!
//! ⚠️ A hit is not a verdict. The rules are deliberately literal, and a game
//! that solves a problem differently will show up as a false positive; the
//! report gives the file and line so the reading takes seconds.

use serde::Serialize;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// A refusal the studio has already received for this exact cause.
    Fail,
    /// Worth reading before upload; not a known refusal on its own.
    Warn,
    /// Context, never a verdict.
    Info,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Pass,
    Warn,
    Fail,
    /// The files this rule reads are not on this machine.
    Skip,
}

/// Where a rule reads. The sets are kept apart because the same pattern means
/// different things in a source tree and in a bundle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Scope {
    /// `<app>/src` — the game's own sources.
    AppSrc,
    /// `<app>` — package.json, static/, svelte.config…
    AppRoot,
    /// `<frontend>/packages` — the shared kit.
    Packages,
    /// The static build that gets uploaded.
    Build,
    /// `library/publish_files`.
    Publish,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expect {
    /// Finding the pattern is the failure.
    Absent,
    /// NOT finding it is the failure.
    Present,
}

pub struct Rule {
    pub id: &'static str,
    pub label: &'static str,
    /// What the hit means, in one line — the report is read by whoever is
    /// about to upload, not by whoever wrote the rule.
    pub explain: &'static str,
    /// Section of the studio doc this comes from.
    pub doc: &'static str,
    pub severity: Severity,
    pub scope: Scope,
    /// Path patterns, matched against the path relative to the scope root.
    /// `*` matches inside a segment, `**` across segments.
    pub paths: &'static [&'static str],
    /// Regex searched line by line. Empty → the rule is about the FILES
    /// matching `paths` existing at all.
    pub pattern: &'static str,
    pub expect: Expect,
    /// Lines matching this are dropped before the verdict (the localized
    /// copy of a restricted word is legitimate, its English label is not).
    pub ignore_lines: &'static [&'static str],
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hit {
    pub file: String,
    pub line: usize,
    /// The matching line, trimmed and truncated — enough to judge without
    /// opening the file.
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub id: String,
    pub label: String,
    pub explain: String,
    pub doc: String,
    pub severity: Severity,
    pub status: Status,
    pub scope: Scope,
    /// One line: what was found, or why the rule was skipped.
    pub detail: String,
    pub hits: Vec<Hit>,
    /// Hits beyond the ones listed.
    pub extra_hits: usize,
}

/// Roots a report is run against. A missing root skips its rules instead of
/// failing them.
#[derive(Debug, Clone, Default)]
pub struct Roots {
    pub app_src: Option<PathBuf>,
    pub app_root: Option<PathBuf>,
    pub packages: Option<PathBuf>,
    pub build: Option<PathBuf>,
    pub publish: Option<PathBuf>,
}

impl Roots {
    fn get(&self, scope: Scope) -> Option<&PathBuf> {
        match scope {
            Scope::AppSrc => self.app_src.as_ref(),
            Scope::AppRoot => self.app_root.as_ref(),
            Scope::Packages => self.packages.as_ref(),
            Scope::Build => self.build.as_ref(),
            Scope::Publish => self.publish.as_ref(),
        }
    }
}

const MAX_HITS: usize = 12;
const MAX_LINE: usize = 180;
/// Bundles are single-line megabytes; reading a whole one into a String is
/// fine, printing one is not.
const MAX_FILE_BYTES: u64 = 24 * 1024 * 1024;

/// Directories that never hold anything a rule should read.
const SKIP_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    ".svelte-kit",
    ".turbo",
    "dist",
    "coverage",
    "__pycache__",
];

/// Extensions worth grepping. Everything else is an asset.
const TEXT_EXT: &[&str] = &[
    "ts", "js", "mjs", "cjs", "svelte", "html", "css", "json", "md", "txt", "py", "yaml", "yml",
    "map",
];

fn is_text(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| TEXT_EXT.contains(&e))
        .unwrap_or(false)
}

/// Minimal glob: `*` inside a segment, `**` across segments, `?` one char.
pub fn glob_match(pattern: &str, path: &str) -> bool {
    fn seg_match(p: &[u8], s: &[u8]) -> bool {
        // Classic backtracking wildcard match, `*` not crossing '/'.
        let (mut pi, mut si) = (0usize, 0usize);
        let (mut star, mut mark) = (usize::MAX, 0usize);
        while si < s.len() {
            if pi < p.len() && (p[pi] == b'?' || p[pi] == s[si]) {
                pi += 1;
                si += 1;
            } else if pi < p.len() && p[pi] == b'*' {
                star = pi;
                mark = si;
                pi += 1;
            } else if star != usize::MAX && s[si] != b'/' {
                pi = star + 1;
                mark += 1;
                si = mark;
            } else {
                return false;
            }
        }
        while pi < p.len() && p[pi] == b'*' {
            pi += 1;
        }
        pi == p.len()
    }

    if let Some((head, tail)) = pattern.split_once("**") {
        let head = head.trim_end_matches('/');
        let tail = tail.trim_start_matches('/');
        if !head.is_empty() {
            let Some(rest) = path.strip_prefix(head) else {
                return false;
            };
            let rest = rest.trim_start_matches('/');
            return tail.is_empty()
                || rest
                    .match_indices('/')
                    .map(|(i, _)| &rest[i + 1..])
                    .chain(std::iter::once(rest))
                    .any(|sub| glob_match(tail, sub));
        }
        return tail.is_empty()
            || path
                .match_indices('/')
                .map(|(i, _)| &path[i + 1..])
                .chain(std::iter::once(path))
                .any(|sub| glob_match(tail, sub));
    }
    seg_match(pattern.as_bytes(), path.as_bytes())
}

/// Files of a root matching any of the patterns, relative paths returned.
fn collect(root: &Path, patterns: &[&str]) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let path = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            if is_dir {
                if !SKIP_DIRS.contains(&name.as_str()) {
                    stack.push(path);
                }
                continue;
            }
            let Ok(rel) = path.strip_prefix(root) else {
                continue;
            };
            let rel = rel.to_string_lossy().replace('\\', "/");
            if patterns.iter().any(|p| glob_match(p, &rel)) {
                out.push((rel, path));
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

pub fn run(roots: &Roots, rules: &[Rule]) -> Vec<Finding> {
    rules.iter().map(|r| run_rule(roots, r)).collect()
}

fn run_rule(roots: &Roots, rule: &Rule) -> Finding {
    let mut finding = Finding {
        id: rule.id.to_string(),
        label: rule.label.to_string(),
        explain: rule.explain.to_string(),
        doc: rule.doc.to_string(),
        severity: rule.severity,
        status: Status::Skip,
        scope: rule.scope,
        detail: String::new(),
        hits: Vec::new(),
        extra_hits: 0,
    };
    let Some(root) = roots.get(rule.scope) else {
        finding.detail = match rule.scope {
            Scope::Build => "no build for this game — build the frontend, then Reload".into(),
            Scope::Publish => "no publish_files".into(),
            _ => "no frontend sources registered for this game".into(),
        };
        return finding;
    };
    if !root.is_dir() {
        finding.detail = format!("{} is gone", root.display());
        return finding;
    }

    let files = collect(root, rule.paths);
    if files.is_empty() {
        finding.detail = format!("no file matching {}", rule.paths.join(", "));
        // A rule that must find something in files that do not exist is a
        // real failure of the "must exist" kind; the others are skipped.
        if rule.expect == Expect::Present && rule.pattern.is_empty() {
            finding.status = severity_status(rule.severity);
        }
        return finding;
    }

    // A rule without a pattern only checks that the files exist.
    if rule.pattern.is_empty() {
        finding.status = match rule.expect {
            Expect::Present => Status::Pass,
            Expect::Absent => severity_status(rule.severity),
        };
        finding.detail = format!("{} file(s)", files.len());
        finding.hits = files
            .iter()
            .take(MAX_HITS)
            .map(|(rel, _)| Hit {
                file: rel.clone(),
                line: 0,
                text: String::new(),
            })
            .collect();
        finding.extra_hits = files.len().saturating_sub(finding.hits.len());
        return finding;
    }

    let Ok(re) = regex::Regex::new(rule.pattern) else {
        finding.detail = format!("invalid rule pattern: {}", rule.pattern);
        return finding;
    };
    let ignores: Vec<regex::Regex> = rule
        .ignore_lines
        .iter()
        .filter_map(|p| regex::Regex::new(p).ok())
        .collect();

    let mut total = 0usize;
    let mut scanned = 0usize;
    for (rel, path) in &files {
        // A pattern is a text search: a path glob wide enough to catch a
        // texture must not drag megabytes of PNG through the regex.
        if !is_text(path) {
            continue;
        }
        let Ok(meta) = std::fs::metadata(path) else {
            continue;
        };
        if meta.len() > MAX_FILE_BYTES {
            continue;
        }
        let Ok(content) = std::fs::read(path) else {
            continue;
        };
        let text = String::from_utf8_lossy(&content);
        scanned += 1;
        for (i, line) in text.lines().enumerate() {
            if !re.is_match(line) {
                continue;
            }
            // A line that is nothing but a comment ships no behaviour and no
            // copy — and the studio's own files comment the very rules these
            // patterns look for, which would make every rule fail on the game
            // that documents itself best.
            if is_comment_line(line) {
                continue;
            }
            if ignores.iter().any(|ig| ig.is_match(line)) {
                continue;
            }
            total += 1;
            if finding.hits.len() < MAX_HITS {
                let t = line.trim();
                let text = if t.chars().count() > MAX_LINE {
                    format!("{}…", t.chars().take(MAX_LINE).collect::<String>())
                } else {
                    t.to_string()
                };
                finding.hits.push(Hit {
                    file: rel.clone(),
                    line: i + 1,
                    text,
                });
            }
        }
    }
    finding.extra_hits = total.saturating_sub(finding.hits.len());
    finding.status = match (rule.expect, total > 0) {
        (Expect::Absent, false) | (Expect::Present, true) => Status::Pass,
        _ => severity_status(rule.severity),
    };
    finding.detail = match (rule.expect, total) {
        (Expect::Absent, 0) => format!("clean over {scanned} file(s)"),
        (Expect::Absent, n) => format!("{n} hit(s) over {scanned} file(s)"),
        (Expect::Present, 0) => format!("not found in {scanned} file(s)"),
        (Expect::Present, n) => format!("{n} hit(s)"),
    };
    finding
}

/// `//`, `*` (inside a block comment), `#`, `<!--` — the line carries no code.
fn is_comment_line(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("//")
        || t.starts_with("/*")
        || t.starts_with('*')
        || t.starts_with("<!--")
        || t.starts_with('#')
}

fn severity_status(s: Severity) -> Status {
    match s {
        Severity::Fail => Status::Fail,
        Severity::Warn => Status::Warn,
        Severity::Info => Status::Pass,
    }
}

/// Assets referenced by a `key="…"` in the components but never declared in
/// `assets.ts` — the game asks the loader for something that was never
/// registered, and the moment shows up empty.
///
/// The pair is returned whole (missing, unused) because the second half is
/// what a build carries for nothing.
pub fn ghost_assets(app_src: &Path) -> (Vec<String>, Vec<String>) {
    let declared = declared_asset_keys(app_src);
    let mut used: BTreeSet<String> = BTreeSet::new();
    let key_re = regex::Regex::new(r#"\bkey=(?:"([A-Za-z0-9_]+)"|\{'([A-Za-z0-9_]+)'\})"#).unwrap();
    for (_, path) in collect(app_src, &["**/*.svelte", "**/*.ts"]) {
        let Ok(content) = std::fs::read(&path) else {
            continue;
        };
        let text = String::from_utf8_lossy(&content);
        for c in key_re.captures_iter(&text) {
            if let Some(m) = c.get(1).or_else(|| c.get(2)) {
                used.insert(m.as_str().to_string());
            }
        }
    }
    if declared.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let missing: Vec<String> = used.difference(&declared).cloned().collect();
    let unused: Vec<String> = declared.difference(&used).cloned().collect();
    (missing, unused)
}

/// Keys of the `assets.ts` map: the top-level entries of the exported object.
fn declared_asset_keys(app_src: &Path) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let entry_re = regex::Regex::new(r"^\s{0,2}([A-Za-z0-9_]+)\s*:").unwrap();
    for (_, path) in collect(app_src, &["**/assets.ts", "**/assets*.ts"]) {
        let Ok(content) = std::fs::read(&path) else {
            continue;
        };
        for line in String::from_utf8_lossy(&content).lines() {
            if let Some(c) = entry_re.captures(line) {
                out.insert(c[1].to_string());
            }
        }
    }
    out
}

/// Book event types the frontend never names. Every event of a book must have
/// a player-facing feedback; one that no source file mentions is a moment the
/// game plays silently.
pub fn uncovered_events(app_src: &Path, event_types: &[String]) -> Vec<String> {
    if event_types.is_empty() {
        return Vec::new();
    }
    let mut haystack = String::new();
    for (_, path) in collect(app_src, &["**/*.ts", "**/*.svelte"]) {
        if let Ok(content) = std::fs::read(&path) {
            haystack.push_str(&String::from_utf8_lossy(&content));
            haystack.push('\n');
        }
    }
    event_types
        .iter()
        .filter(|t| !haystack.contains(t.as_str()))
        .cloned()
        .collect()
}

/// The game's declared version, from `package.json` and from the
/// `<GameVersion version="…" />` the player reads. They must agree: the
/// dashboard shows one and the screen the other.
pub fn version_pair(app_root: &Path) -> (Option<String>, Option<String>) {
    let pkg = std::fs::read_to_string(app_root.join("package.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("version").and_then(|v| v.as_str()).map(str::to_string));

    let re = regex::Regex::new(r#"GameVersion[^>]*version=\{?["'`]([^"'`]+)["'`]"#).unwrap();
    let mut shown = None;
    for (_, path) in collect(&app_root.join("src"), &["**/*.svelte", "**/*.ts"]) {
        let Ok(content) = std::fs::read(&path) else {
            continue;
        };
        if let Some(c) = re.captures(&String::from_utf8_lossy(&content)) {
            shown = Some(c[1].to_string());
            break;
        }
    }
    (pkg, shown)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs() {
        assert!(glob_match("**/*.ts", "src/game/config.ts"));
        assert!(glob_match("**/*.ts", "config.ts"));
        assert!(!glob_match("**/*.ts", "src/game/config.svelte"));
        assert!(glob_match("src/app.html", "src/app.html"));
        assert!(glob_match("*.json", "package.json"));
        assert!(!glob_match("*.json", "src/package.json"));
        assert!(glob_match("_app/immutable/**/*.js", "_app/immutable/chunks/a.js"));
    }

    #[test]
    fn absent_rule_passes_on_clean_tree() {
        let dir = std::env::temp_dir().join(format!("slotlab-pf-{}", std::process::id()));
        let src = dir.join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("a.ts"), "const preference = 'webgl';\n").unwrap();
        let roots = Roots {
            app_src: Some(src.clone()),
            ..Default::default()
        };
        let rules = [Rule {
            id: "webgpu",
            label: "WebGPU",
            explain: "",
            doc: "",
            severity: Severity::Fail,
            scope: Scope::AppSrc,
            paths: &["**/*.ts"],
            pattern: r"preference:\s*['\x22]webgpu",
            expect: Expect::Absent,
            ignore_lines: &[],
        }];
        let out = run(&roots, &rules);
        assert_eq!(out[0].status, Status::Pass);

        std::fs::write(src.join("b.ts"), "  preference: 'webgpu',\n").unwrap();
        let out = run(&roots, &rules);
        assert_eq!(out[0].status, Status::Fail);
        assert_eq!(out[0].hits[0].line, 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_root_skips() {
        let out = run(
            &Roots::default(),
            &[Rule {
                id: "x",
                label: "x",
                explain: "",
                doc: "",
                severity: Severity::Fail,
                scope: Scope::Build,
                paths: &["index.html"],
                pattern: "",
                expect: Expect::Present,
                ignore_lines: &[],
            }],
        );
        assert_eq!(out[0].status, Status::Skip);
    }
}

// ─── The catalogue ──────────────────────────────────────────────────────────
//
// One entry per line of the pre-upload checklist that a machine can read.
// `doc` points at the section of the studio doc that justifies the rule —
// several of them exist because the exact refusal was received, and the
// wording says which.

/// Shorthand so the table below reads as a table.
macro_rules! rule {
    ($id:literal, $label:literal, $sev:ident, $scope:ident, $paths:expr, $pat:literal,
     $expect:ident, $doc:literal, $explain:literal $(, ignore: $ig:expr)?) => {
        Rule {
            id: $id,
            label: $label,
            explain: $explain,
            doc: $doc,
            severity: Severity::$sev,
            scope: Scope::$scope,
            paths: $paths,
            pattern: $pat,
            expect: Expect::$expect,
            ignore_lines: rule!(@ig $($ig)?),
        }
    };
    (@ig) => { &[] };
    (@ig $ig:expr) => { $ig };
}

const TS: &[&str] = &["**/*.ts", "**/*.svelte", "**/*.js"];
const SVELTE: &[&str] = &["**/*.svelte"];

pub const RULES: &[Rule] = &[
    // ── Renderer ────────────────────────────────────────────────────────────
    rule!(
        "renderer_webgpu_kit", "Renderer: no WebGPU in the kit", Fail, Packages, TS,
        r#"preference:\s*['"]webgpu"#, Absent, "§29",
        "WebGPU + a fractional resolution gives the reviewer a BLACK SCREEN (depth-stencil off by one pixel). The refusal reads \"game doesn't display properly\"."
    ),
    rule!(
        "renderer_webgpu_app", "Renderer: no WebGPU in the game", Fail, AppSrc, TS,
        r#"preference:\s*['"]webgpu"#, Absent, "§29",
        "Same cause, in the game's own sources."
    ),
    rule!(
        "typekit_csp", "No external Typekit in the build", Fail, Build, &["index.html"],
        "aba0ebl", Absent, "§29",
        "An external font host is blocked by the CSP: the page loads without its font."
    ),
    // ── Bet guards ──────────────────────────────────────────────────────────
    rule!(
        "insufficient_funds_modal", "ModalInsufficientFunds exists", Fail, Packages,
        &["**/ModalInsufficientFunds.svelte"], "", Present, "§24.3",
        "Without it, a spin the player cannot afford fails silently instead of saying so."
    ),
    rule!(
        "bet_menu", "The bet menu is reachable", Fail, AppSrc, TS,
        r"betAmountMenu|betAmountOptions|BetMenu", Present, "§24.2",
        "Zero hit is a guaranteed refusal: \"betting parameters hardcoded\" — received three times."
    ),
    rule!(
        "bet_level_slicing", "All bet levels displayed", Fail, Packages, TS,
        r"MOST_USED_BET_INDEXES|CHIP_COUNT|\.slice\(\s*0\s*,\s*1[0-9]\s*\)", Absent, "§24.2",
        "Refusal \"all bet levels must be displayed\": a slice or a most-used shortlist hides part of the ladder."
    ),
    rule!(
        "bet_level_slicing_app", "All bet levels displayed (game)", Fail, AppSrc, TS,
        r"MOST_USED_BET_INDEXES|CHIP_COUNT", Absent, "§24.2",
        "Same shortlist, re-introduced in the game."
    ),
    rule!(
        "step_bet_filter", "resolveBetConfig without the step filter", Fail, Packages,
        &["**/betLevels.ts"], r"level\s*%\s*step\s*===\s*0", Absent, "§24.1",
        "Filtering explicit levels by `stepBet` drops the levels the operator declared — the exact cause of \"betting parameters hardcoded\"."
    ),
    rule!(
        "replay_label_guard", "LabelBet disabled during a replay", Warn, Packages,
        &["**/LabelBet.svelte"], r"disabled\s*=.*replay", Present, "§24.5",
        "During a bet replay the bet label must not be clickable — it would open a session call on a round that is being re-watched."
    ),
    rule!(
        "space_hold_guard", "Spacebar gated while a menu is open", Warn, Packages, TS,
        r"isMenuOpen|stateModal\.modal !== null", Present, "§24.4",
        "The third spin path (holding space) is the one that bypassed the affordability check: \"bypasses the check when holding spacebar\"."
    ),
    rule!(
        "activate_modes", "Modes wired by direct assignment", Info, AppSrc, TS,
        r"activeBetModeKey\s*=", Absent, "§24.4",
        "A `type: 'activate'` mode assigned directly starts a 3-4x spin on ONE click: \"high-cost bet mode activated with a single click\". Each hit needs a confirmation in front of it."
    ),
    // ── Amounts ─────────────────────────────────────────────────────────────
    rule!(
        "amount_reformat", "No re-formatting over the kit helper", Fail, AppSrc, TS,
        r"bookEventAmountToNormalisedAmount\([^)]*\)\s*\.\s*(toFixed|toLocaleString)", Absent, "§30",
        "Re-rounding to two decimals over the helper produced \"payouts below one cent\"."
    ),
    rule!(
        "win_fraction_digits", "winFractionDigits used", Warn, AppSrc, TS,
        r"winFractionDigits", Present, "§29",
        "The kit's helper carries the currency's own precision. No mention of it in the \
         game means its amounts are formatted somewhere else — which is how \"payouts \
         below one cent\" happened."
    ),
    rule!(
        "probability_wording", "No numeric odds in the copy", Fail, AppSrc, TS,
        r"[0-9]+x? ?(more |the normal )?chance", Absent, "§25",
        "Stating a probability in the info page is refused outright."
    ),
    // ── Social mode & terminology ───────────────────────────────────────────
    rule!(
        "social_demo_strings", "Demo copy stripped from the bundle", Fail, Build,
        &["_app/immutable/**/*.js", "**/*.js"], r"PLACE YOUR BET|twist-game\.app", Absent, "§23.4",
        "`state-shared/constants.ts` ships its demo content into EVERY bundle; the sweep has to be done on the build, not on the sources."
    ),
    rule!(
        "restricted_terms", "Restricted terminology", Fail, AppSrc, SVELTE,
        r">PAYS<|>PAYTABLE<|pay anywhere|paying symbol|\bpays\b|Play cost", Absent, "§23.1",
        "Four refusals for \"restricted terminology\", one of them re-introduced by our own fix.",
        ignore: &["social", "i18n", "msg\\(", "PAYS_ANYWHERE"]
    ),
    rule!(
        "payline_term", "\"payline\" outside the config", Warn, AppSrc, TS,
        r"\bpaylines?\b", Absent, "§23.1",
        "Refused in a single card once. Legitimate inside `config.paylines`.",
        ignore: &["social", "config\\.paylines", "paylines:", "numberOfPaylines"]
    ),
    // ── Theme, fonts, accessibility ─────────────────────────────────────────
    rule!(
        "ui_tokens", "--ui-* tokens defined", Fail, AppRoot, &["src/app.html"],
        r"--ui-", Present, "§21.9",
        "Undefined, every modal falls back to the SYSTEM font — the literal refusal motive \"standard fonts\". It hit the whole fleet."
    ),
    rule!(
        "reduced_motion", "prefers-reduced-motion honoured", Warn, AppSrc, TS,
        r"prefers-reduced-motion", Present, "§16.10",
        "The canvas does not see media queries: WCAG 2.3.3 has to be wired by hand."
    ),
    rule!(
        "system_font", "No system-ui font", Warn, AppSrc, &["**/*.svelte", "**/*.css", "**/*.ts"],
        r#"font-family:\s*['\"]?(system-ui|-apple-system|sans-serif)"#, Absent, "§21.9",
        "A system font LEADING the stack is what the reviewer reads as \"standard fonts\". \
         A fallback after the game's own font is fine."
    ),
    // ── Staging driven by the book ──────────────────────────────────────────
    rule!(
        "winlevel_from_book", "setWin does not read bookEvent.winLevel", Warn, AppSrc, TS,
        r"bookEvent\.winLevel", Absent, "§16.6",
        "With a ladder per bet mode, the math's win level lies: it ignores what the spin cost."
    ),
    rule!(
        "anticipation_row", "Intra-column anticipation", Info, AppSrc, TS,
        r"anticipationRow", Present, "§18.3",
        "Present means the staging follows the row the book gives; its consistency is checked by the book scan."
    ),
    rule!(
        "uninterruptible_wait", "Wait loops", Info, Packages, &["**/utils-slots/**/*.ts"],
        r"while\s*\(", Absent, "§35",
        "A `while` whose only await can resolve synchronously kills the tab. Each hit should go through `interruptibleWait`."
    ),
    // ── Hygiene ─────────────────────────────────────────────────────────────
    rule!(
        "ds_store", "No .DS_Store shipped", Warn, AppRoot, &["**/.DS_Store"],
        "", Absent, "§29",
        "Uploaded with the rest of static/."
    ),
    rule!(
        "backup_dirs", "No _backup folder in static/", Warn, AppRoot, &["**/_backup/**"],
        "", Absent, "§29",
        "A backup folder inside static/ is uploaded and served."
    ),
    rule!(
        "scaffold_leftovers", "No scaffold leftovers", Warn, AppSrc, TS,
        r"slot_template", Absent, "§27.4",
        "Assets and identifiers inherited from the scaffold ship with the game."
    ),
    rule!(
        "build_exists", "A build to upload", Fail, Build, &["index.html"],
        "", Present, "§29",
        "The build folder goes up with publish_files."
    ),
];
