use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

/// DTOs matching the games' `frontend/packages/rgs-fetcher/src/schema.ts`.
/// Integer amounts in micro-units: 1,000,000 = $1 (API_AMOUNT_MULTIPLIER).

#[derive(Serialize, Clone)]
pub struct StatusObject {
    #[serde(rename = "statusCode")]
    pub status_code: &'static str,
    #[serde(rename = "statusMessage")]
    pub status_message: String,
}

impl StatusObject {
    pub fn success() -> Self {
        Self { status_code: "SUCCESS", status_message: "".into() }
    }
    pub fn err(code: &'static str, msg: impl Into<String>) -> Self {
        Self { status_code: code, status_message: msg.into() }
    }
}

#[derive(Serialize, Clone)]
pub struct BalanceObject {
    pub amount: u64,
    pub currency: String,
}

#[allow(dead_code)] // schema template, round_json builds the JSON by hand
#[derive(Serialize, Clone)]
pub struct RoundDetail {
    #[serde(rename = "roundID")]
    pub round_id: u64,
    pub amount: u64,
    pub payout: u64,
    #[serde(rename = "payoutMultiplier")]
    pub payout_multiplier: f64,
    pub active: bool,
    pub mode: String,
    pub event: String,
    pub state: Arc<Value>,
}

#[derive(Serialize)]
pub struct BetModeConfig {
    pub mode: String,
    #[serde(rename = "costMultiplier")]
    pub cost_multiplier: f64,
    pub feature: bool,
}

#[derive(Serialize)]
pub struct Jurisdiction {
    #[serde(rename = "socialCasino")]
    pub social_casino: bool,
    #[serde(rename = "disabledFullscreen")]
    pub disabled_fullscreen: bool,
    #[serde(rename = "disabledTurbo")]
    pub disabled_turbo: bool,
    #[serde(rename = "disabledSuperTurbo")]
    pub disabled_super_turbo: bool,
    #[serde(rename = "disabledAutoplay")]
    pub disabled_autoplay: bool,
    #[serde(rename = "disabledSlamstop")]
    pub disabled_slamstop: bool,
    #[serde(rename = "disabledSpacebar")]
    pub disabled_spacebar: bool,
    #[serde(rename = "disabledBuyFeature")]
    pub disabled_buy_feature: bool,
    #[serde(rename = "displayNetPosition")]
    pub display_net_position: bool,
    #[serde(rename = "displayRTP")]
    pub display_rtp: bool,
    #[serde(rename = "displaySessionTimer")]
    pub display_session_timer: bool,
    #[serde(rename = "minimumRoundDuration")]
    pub minimum_round_duration: u32,
}

impl Jurisdiction {
    pub fn new(social: bool) -> Self {
        Self {
            social_casino: social,
            disabled_fullscreen: false,
            disabled_turbo: false,
            disabled_super_turbo: false,
            disabled_autoplay: false,
            disabled_slamstop: false,
            disabled_spacebar: false,
            disabled_buy_feature: false,
            display_net_position: false,
            display_rtp: true,
            display_session_timer: false,
            minimum_round_duration: 0,
        }
    }
}

#[derive(Serialize)]
pub struct ConfigObject {
    #[serde(rename = "betLevels")]
    pub bet_levels: Vec<u64>,
    #[serde(rename = "betModes")]
    pub bet_modes: serde_json::Map<String, Value>,
    #[serde(rename = "defaultBetLevel")]
    pub default_bet_level: u64,
    #[serde(rename = "minBet")]
    pub min_bet: u64,
    #[serde(rename = "maxBet")]
    pub max_bet: u64,
    #[serde(rename = "stepBet")]
    pub step_bet: u64,
    pub jurisdiction: Jurisdiction,
}

/// Default bet levels, in micro-units ($0.10 → $1,000).
pub fn default_bet_levels() -> Vec<u64> {
    [
        0.10f64, 0.20, 0.30, 0.40, 0.50, 0.60, 0.80, 1.0, 1.20, 1.60, 2.0, 3.0,
        4.0, 5.0, 6.0, 8.0, 10.0, 12.0, 16.0, 20.0, 30.0, 40.0, 50.0, 60.0,
        80.0, 100.0, 120.0, 160.0, 200.0, 300.0, 400.0, 500.0, 600.0, 800.0,
        1000.0,
    ]
    .iter()
    .map(|d| (d * 1_000_000.0).round() as u64)
    .collect()
}

// ---- Requests ----

#[derive(Deserialize)]
pub struct ReqAuthenticate {
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(default)]
    pub language: Option<String>,
}

#[derive(Deserialize)]
pub struct ReqBalance {
    #[serde(rename = "sessionID")]
    pub session_id: String,
}

#[derive(Deserialize)]
pub struct ReqPlay {
    #[serde(rename = "sessionID")]
    pub session_id: String,
    pub mode: String,
    #[serde(default)]
    pub currency: Option<String>,
    pub amount: u64,
}

#[derive(Deserialize)]
pub struct ReqEndRound {
    #[serde(rename = "sessionID")]
    pub session_id: String,
}

#[derive(Deserialize)]
pub struct ReqEvent {
    #[serde(rename = "sessionID")]
    pub session_id: String,
    pub event: String,
}
