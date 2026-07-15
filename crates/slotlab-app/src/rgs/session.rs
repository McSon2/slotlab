use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Default balance of a new session: $10,000 in micro-units.
pub const DEFAULT_BALANCE_MICRO: u64 = 10_000 * 1_000_000;

#[derive(Clone)]
#[allow(dead_code)] // sim_id exposed for upcoming session views
pub struct ActiveRound {
    pub round_id: u64,
    /// Base bet (micro), as sent by the game.
    pub amount: u64,
    /// Total payout (micro), credited at end-round.
    pub payout: u64,
    pub payout_multiplier: f64,
    pub mode: String,
    pub sim_id: u64,
    pub state: Arc<Value>,
    /// Last event index confirmed by /bet/event.
    pub event: String,
    pub active: bool,
}

#[derive(Clone)]
pub struct Session {
    pub game: String,
    pub currency: String,
    pub language: String,
    pub social: bool,
    pub balance_micro: u64,
    pub round: Option<ActiveRound>,
    /// Book forced for the next spins: (mode, sim_id). Sticky
    /// until cleared (dev-tool behavior).
    pub force: Option<(String, u64)>,
    pub round_counter: u64,
}

impl Session {
    fn new(game: &str) -> Self {
        Self {
            game: game.to_string(),
            currency: "USD".into(),
            language: "en".into(),
            social: false,
            balance_micro: DEFAULT_BALANCE_MICRO,
            round: None,
            force: None,
            round_counter: 0,
        }
    }
}

/// RGS session store. Unknown sessionIDs are created on the fly
/// (necessary: the game authenticates with the ID passed in the URL, and
/// share hands out IDs that were never prepared).
#[derive(Default)]
pub struct Sessions {
    inner: Mutex<HashMap<String, Session>>,
    /// GAME-level force (all viewports): game → (mode, sim_id).
    game_force: Mutex<HashMap<String, (String, u64)>>,
}

impl Sessions {
    pub fn get_or_create(&self, session_id: &str, game: &str) -> Session {
        let mut map = self.inner.lock().unwrap();
        map.entry(session_id.to_string())
            .or_insert_with(|| Session::new(game))
            .clone()
    }

    #[allow(dead_code)]
    pub fn get(&self, session_id: &str) -> Option<Session> {
        self.inner.lock().unwrap().get(session_id).cloned()
    }

    /// Applies a mutation under lock and returns the updated session.
    pub fn update<F: FnOnce(&mut Session)>(
        &self,
        session_id: &str,
        game: &str,
        f: F,
    ) -> Session {
        let mut map = self.inner.lock().unwrap();
        let s = map
            .entry(session_id.to_string())
            .or_insert_with(|| Session::new(game));
        f(s);
        s.clone()
    }

    /// Prepares/overwrites a session from the UI (balance, currency, social).
    pub fn prepare(
        &self,
        session_id: &str,
        game: &str,
        balance_micro: Option<u64>,
        currency: Option<String>,
        social: Option<bool>,
        language: Option<String>,
    ) {
        let mut map = self.inner.lock().unwrap();
        let s = map
            .entry(session_id.to_string())
            .or_insert_with(|| Session::new(game));
        s.game = game.to_string();
        if let Some(b) = balance_micro {
            s.balance_micro = b;
        }
        if let Some(c) = currency {
            s.currency = c;
        }
        if let Some(soc) = social {
            s.social = soc;
        }
        if let Some(l) = language {
            s.language = l;
        }
    }

    pub fn set_game_force(&self, game: &str, force: Option<(String, u64)>) {
        let mut map = self.game_force.lock().unwrap();
        match force {
            Some(f) => {
                map.insert(game.to_string(), f);
            }
            None => {
                map.remove(game);
            }
        }
    }

    pub fn game_force(&self, game: &str) -> Option<(String, u64)> {
        self.game_force.lock().unwrap().get(game).cloned()
    }

    pub fn set_session_force(
        &self,
        session_id: &str,
        game: &str,
        force: Option<(String, u64)>,
    ) {
        self.update(session_id, game, |s| s.force = force);
    }

    /// Reset: removes all sessions of a game (or all of them).
    pub fn reset(&self, game: Option<&str>) {
        let mut map = self.inner.lock().unwrap();
        match game {
            Some(g) => map.retain(|_, s| s.game != g),
            None => map.clear(),
        }
    }

    #[allow(dead_code)] // "active sessions" view planned in the UI
    pub fn list(&self, game: Option<&str>) -> Vec<(String, Session)> {
        self.inner
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, s)| game.is_none_or(|g| s.game == g))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }
}
