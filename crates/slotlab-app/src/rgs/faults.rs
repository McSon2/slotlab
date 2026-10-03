//! Faults the emulated RGS can be told to produce on purpose.
//!
//! ── Why the tool has to be able to break itself ─────────────────────────────
//! Four of the refusals the studio has received are about states nobody can
//! reach by playing normally: a round left open by a closed tab (does the game
//! offer to re-watch it, and does it restore the bet?), a balance too small for
//! the next spin (is there a modal, or does the button just do nothing?), an
//! RGS that answers an error mid-session, an RGS that answers slowly enough for
//! a double click to slip through. The reviewer produces those states by
//! accident, in five minutes of play; a developer cannot produce them at all —
//! unless the RGS is on their side of the wire, which here it is.
//!
//! So each fault is a switch on the SERVER, not a mock in the game: the build
//! under test is the build that ships, and it is the answers that change.
//!
//! Faults are per game, live for as long as the process, and reset with the
//! sessions — nothing here is persisted, because a fault that outlives the
//! session it was set for is a bug that looks like the game's.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

/// The RGS calls a game makes, as the fault panel names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Endpoint {
    Authenticate,
    Balance,
    Play,
    EndRound,
    Event,
    Replay,
}

/// How a call is made to fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum FailureKind {
    /// A well-formed RGS error object — what the game is supposed to handle.
    #[default]
    RgsError,
    /// HTTP 500 with a body the game did not ask for: the network path.
    Http500,
    /// A connection that never answers: the spin button must not stay stuck.
    Hang,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FailureRule {
    pub endpoint: Endpoint,
    #[serde(default)]
    pub kind: FailureKind,
    /// RGS error code, e.g. `ERR_IPB` (insufficient balance). Ignored by the
    /// other kinds.
    #[serde(default = "default_code")]
    pub code: String,
    /// How many calls are still to be failed. 0 = every call until cleared.
    #[serde(default)]
    pub remaining: u32,
    /// True when `remaining` counts down; false = unlimited.
    #[serde(default)]
    pub limited: bool,
}

fn default_code() -> String {
    "ERR_GE".to_string()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Faults {
    /// Delay added to every answer, in milliseconds. The reviewer plays on a
    /// worse connection than the developer, always.
    pub latency_ms: u64,
    /// `end-round` answers success but leaves the round open and unpaid —
    /// exactly what a closed tab leaves behind. The next authenticate then
    /// carries the round, and the game must offer to watch or skip it, with
    /// the round's own bet restored.
    pub keep_round_open: bool,
    /// True → `keep_round_open` clears itself after one round.
    pub keep_round_open_once: bool,
    pub failures: Vec<FailureRule>,
}

impl Faults {
    pub fn is_clean(&self) -> bool {
        self.latency_ms == 0 && !self.keep_round_open && self.failures.is_empty()
    }
}

#[derive(Default)]
pub struct FaultManager {
    by_game: Mutex<HashMap<String, Faults>>,
}

impl FaultManager {
    pub fn get(&self, game: &str) -> Faults {
        self.by_game
            .lock()
            .unwrap()
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(game))
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    }

    pub fn set(&self, game: &str, faults: Faults) {
        let mut map = self.by_game.lock().unwrap();
        map.retain(|k, _| !k.eq_ignore_ascii_case(game));
        if !faults.is_clean() {
            map.insert(game.to_string(), faults);
        }
    }

    pub fn clear(&self, game: Option<&str>) {
        let mut map = self.by_game.lock().unwrap();
        match game {
            Some(g) => map.retain(|k, _| !k.eq_ignore_ascii_case(g)),
            None => map.clear(),
        }
    }

    /// The rule that applies to this call, consuming one use of it.
    pub fn take(&self, game: &str, endpoint: Endpoint) -> Option<FailureRule> {
        let mut map = self.by_game.lock().unwrap();
        let key = map
            .keys()
            .find(|k| k.eq_ignore_ascii_case(game))
            .cloned()?;
        let f = map.get_mut(&key)?;
        let idx = f.failures.iter().position(|r| r.endpoint == endpoint)?;
        let rule = f.failures[idx].clone();
        if rule.limited {
            if rule.remaining <= 1 {
                f.failures.remove(idx);
            } else {
                f.failures[idx].remaining -= 1;
            }
        }
        Some(rule)
    }

    /// Should this round be left open? Consumes a one-shot.
    pub fn take_keep_round_open(&self, game: &str) -> bool {
        let mut map = self.by_game.lock().unwrap();
        let Some(key) = map.keys().find(|k| k.eq_ignore_ascii_case(game)).cloned() else {
            return false;
        };
        let Some(f) = map.get_mut(&key) else {
            return false;
        };
        if !f.keep_round_open {
            return false;
        }
        if f.keep_round_open_once {
            f.keep_round_open = false;
            f.keep_round_open_once = false;
        }
        true
    }
}
