use super::pool::Pool;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Cap on the number of Share sessions kept in memory. An always-on server
/// hands out one per visit and nothing ever closes them: without this the
/// map grows for as long as the process lives.
const MAX_SHARED_SESSIONS: usize = 10;

/// Idle time after which a Share session is forgotten. A demo visitor who
/// closed the tab hours ago is not "on the game"; keeping their row makes
/// the dashboard unreadable.
pub const SHARED_SESSION_TTL: Duration = Duration::from_secs(10 * 60);

/// Same caps for every OTHER session (SSP driver, Play tab, unknown IDs
/// created on the fly). SSP mints a new timestamped ID per run and each
/// session keeps its last round's book: left alone, the map grew until the
/// hub hit its container limit every ~3 h.
const MAX_OTHER_SESSIONS: usize = 10;
const OTHER_SESSION_TTL: Duration = Duration::from_secs(10 * 60);

/// Default balance of a new session: $10,000 in micro-units.
pub const DEFAULT_BALANCE_MICRO: u64 = 10_000 * 1_000_000;

/// Prefix of the sessions opened by an EXTERNAL driver — SSP in dev, which
/// spins the games in progress to judge the feel of the math. Like a Share
/// visitor, such a session draws from the plain LUT: a book pinned or a
/// « bonus only » draw set in the Play tab would fake its stats.
pub const EXTERNAL_SESSION_PREFIX: &str = "ssp-";

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
    /// Le forçage se consomme au premier spin qui l'utilise. Sinon la
    /// console imposerait le même book à tous les tours suivants, ce qui
    /// n'est pas ce qu'on veut pour montrer UNE animation.
    pub force_once: bool,
    pub round_counter: u64,
    /// Session handed out by Share to a public visitor. Such a session
    /// ignores the GAME-level force: what the dev pins in Play must never
    /// leak into a client's spins.
    pub shared: bool,
    /// Session of an external driver (`EXTERNAL_SESSION_PREFIX`): same
    /// exclusion of the GAME-level force and pool as a Share session.
    pub external: bool,
    /// Cumulative amounts (micro), for the Share sessions panel.
    pub wagered_micro: u64,
    pub won_micro: u64,
    /// Last touch, for evicting the oldest Share sessions.
    pub last_seen: Instant,
    /// Ce qu'on sait du visiteur, sans jamais rien lui demander : les
    /// en-têtes de Cloudflare. Sans ça le tableau de bord n'affiche qu'un
    /// mur d'identifiants hexadécimaux indiscernables.
    pub ip: String,
    pub country: String,
    pub device: String,
}

/// What we know about a visitor when their session is created — uniquement
/// ce que le réseau nous dit, rien n'est demandé à la personne.
#[derive(Default, Clone)]
pub struct Identity {
    pub ip: String,
    pub country: String,
    pub device: String,
}

impl Session {
    fn new(game: &str, balance_micro: u64) -> Self {
        Self {
            game: game.to_string(),
            currency: "USD".into(),
            language: "en".into(),
            social: false,
            balance_micro,
            round: None,
            force: None,
            force_once: true,
            round_counter: 0,
            shared: false,
            external: false,
            wagered_micro: 0,
            won_micro: 0,
            last_seen: Instant::now(),
            ip: String::new(),
            country: String::new(),
            device: String::new(),
        }
    }

    /// True if the GAME-level force and « bonus only » pool must not apply:
    /// a Share visitor or an external driver sees the plain LUT.
    pub fn plain_draw(&self) -> bool {
        self.shared || self.external
    }
}

/// RGS session store. Unknown sessionIDs are created on the fly
/// (necessary: the game authenticates with the ID passed in the URL, and
/// share hands out IDs that were never prepared).
pub struct Sessions {
    inner: Mutex<HashMap<String, Session>>,
    /// GAME-level force (all viewports): game → (mode, sim_id).
    game_force: Mutex<HashMap<String, (String, u64)>>,
    /// GAME-level restricted draw, per mode: (game, mode) → bonus-only books.
    /// A pinned book wins over it; clearing the book falls back to the pool.
    game_pool: Mutex<HashMap<(String, String), Arc<Pool>>>,
    /// Balance given to a session created on the fly. The share-only
    /// server raises it to its own setting, so a visitor whose session was
    /// evicted comes back with the same balance as everyone else.
    default_balance: AtomicU64,
}

impl Default for Sessions {
    fn default() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            game_force: Mutex::new(HashMap::new()),
            game_pool: Mutex::new(HashMap::new()),
            default_balance: AtomicU64::new(DEFAULT_BALANCE_MICRO),
        }
    }
}

impl Sessions {
    /// Balance of sessions created on the fly.
    pub fn set_default_balance(&self, micro: u64) {
        self.default_balance.store(micro, Ordering::Relaxed);
    }

    /// Makes room before a new, non-Share session is inserted: drops the
    /// idle ones, then the oldest beyond the cap. Share sessions keep their
    /// own rules (`create_shared`).
    fn admit(map: &mut HashMap<String, Session>, session_id: &str) {
        if map.contains_key(session_id) {
            return;
        }
        map.retain(|_, s| s.shared || s.last_seen.elapsed() < OTHER_SESSION_TTL);
        while map.values().filter(|s| !s.shared).count() >= MAX_OTHER_SESSIONS {
            let Some(oldest) = map
                .iter()
                .filter(|(_, s)| !s.shared)
                .min_by_key(|(_, s)| s.last_seen)
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            map.remove(&oldest);
        }
    }

    fn new_session(&self, session_id: &str, game: &str) -> Session {
        let mut s = Session::new(game, self.default_balance.load(Ordering::Relaxed));
        s.external = session_id.starts_with(EXTERNAL_SESSION_PREFIX);
        s
    }

    pub fn get_or_create(&self, session_id: &str, game: &str) -> Session {
        let mut map = self.inner.lock().unwrap();
        Self::admit(&mut map, session_id);
        let s = map
            .entry(session_id.to_string())
            .or_insert_with(|| self.new_session(session_id, game));
        s.last_seen = Instant::now();
        s.clone()
    }

    #[allow(dead_code)]
    pub fn get(&self, session_id: &str) -> Option<Session> {
        self.inner.lock().unwrap().get(session_id).cloned()
    }

    /// Applies a mutation under lock and returns the updated session.
    pub fn update<F: FnOnce(&mut Session)>(&self, session_id: &str, game: &str, f: F) -> Session {
        let mut map = self.inner.lock().unwrap();
        Self::admit(&mut map, session_id);
        let s = map
            .entry(session_id.to_string())
            .or_insert_with(|| self.new_session(session_id, game));
        f(s);
        s.last_seen = Instant::now();
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
        Self::admit(&mut map, session_id);
        let s = map
            .entry(session_id.to_string())
            .or_insert_with(|| self.new_session(session_id, game));
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

    /// Creates the session of one Share visitor. Each opening of the public
    /// link mints its own ID, so balance and active round are never shared
    /// between two people behind the same URL.
    pub fn create_shared(
        &self,
        session_id: &str,
        game: &str,
        balance_micro: u64,
        currency: &str,
        language: &str,
        social: bool,
        who: Identity,
    ) {
        let mut map = self.inner.lock().unwrap();
        map.retain(|_, s| !s.shared || s.last_seen.elapsed() < SHARED_SESSION_TTL);
        while map.values().filter(|s| s.shared).count() >= MAX_SHARED_SESSIONS {
            let Some(oldest) = map
                .iter()
                .filter(|(_, s)| s.shared)
                .min_by_key(|(_, s)| s.last_seen)
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            map.remove(&oldest);
        }
        let s = map
            .entry(session_id.to_string())
            .or_insert_with(|| self.new_session(session_id, game));
        s.game = game.to_string();
        s.balance_micro = balance_micro;
        s.currency = currency.to_string();
        s.language = language.to_string();
        s.social = social;
        s.shared = true;
        s.last_seen = Instant::now();
        s.ip = who.ip;
        s.country = who.country;
        s.device = who.device;
    }

    /// True if this id is a live Share session of that game — used to hand
    /// a returning visitor their own session back instead of minting one
    /// more.
    pub fn is_live_shared(&self, session_id: &str, game: &str) -> bool {
        self.inner.lock().unwrap().get(session_id).is_some_and(|s| {
            s.shared && s.game == game && s.last_seen.elapsed() < SHARED_SESSION_TTL
        })
    }

    /// Marks a known session as seen, keeping its identity fresh.
    pub fn touch_shared(&self, session_id: &str, who: &Identity) {
        let mut map = self.inner.lock().unwrap();
        if let Some(s) = map.get_mut(session_id) {
            s.last_seen = Instant::now();
            if !who.ip.is_empty() {
                s.ip = who.ip.clone();
            }
            if !who.country.is_empty() {
                s.country = who.country.clone();
            }
            if !who.device.is_empty() {
                s.device = who.device.clone();
            }
        }
    }

    pub fn remove(&self, session_id: &str) -> bool {
        self.inner.lock().unwrap().remove(session_id).is_some()
    }

    /// Drops every Share session of a game (or of all games).
    pub fn clear_shared(&self, game: Option<&str>) -> usize {
        let mut map = self.inner.lock().unwrap();
        let before = map.len();
        map.retain(|_, s| !s.shared || game.is_some_and(|g| s.game != g));
        before - map.len()
    }

    /// Share sessions of a game, newest first, for the "who is playing"
    /// panel. Expired ones are dropped on the way: the listing is the only
    /// thing that runs regularly enough to do the cleaning.
    pub fn list_shared(&self, game: &str) -> Vec<(String, Session)> {
        let mut map = self.inner.lock().unwrap();
        map.retain(|_, s| !s.shared || s.last_seen.elapsed() < SHARED_SESSION_TTL);
        let mut v: Vec<_> = map
            .iter()
            .filter(|(_, s)| s.shared && s.game == game)
            .map(|(k, s)| (k.clone(), s.clone()))
            .collect();
        drop(map);
        v.sort_by(|a, b| b.1.last_seen.cmp(&a.1.last_seen));
        v
    }

    /// Puts one session back to its starting balance and drops its round.
    /// Returns false if the session is unknown.
    pub fn reset_one(&self, session_id: &str, balance_micro: u64) -> bool {
        let mut map = self.inner.lock().unwrap();
        match map.get_mut(session_id) {
            Some(s) => {
                s.balance_micro = balance_micro;
                s.round = None;
                s.round_counter = 0;
                s.wagered_micro = 0;
                s.won_micro = 0;
                true
            }
            None => false,
        }
    }

    /// Sets a session's balance without touching its round. Used to walk a
    /// game up to the edge of "not enough for the next spin", which is where
    /// the affordability guards either exist or do not.
    pub fn set_balance(&self, session_id: &str, game: &str, balance_micro: u64) -> u64 {
        self.update(session_id, game, |s| s.balance_micro = balance_micro)
            .balance_micro
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

    pub fn set_game_pool(&self, game: &str, mode: &str, pool: Option<Arc<Pool>>) {
        let mut map = self.game_pool.lock().unwrap();
        let key = (game.to_string(), mode.to_string());
        match pool {
            Some(p) => {
                map.insert(key, p);
            }
            None => {
                map.remove(&key);
            }
        }
    }

    pub fn game_pool(&self, game: &str, mode: &str) -> Option<Arc<Pool>> {
        self.game_pool
            .lock()
            .unwrap()
            .get(&(game.to_string(), mode.to_string()))
            .cloned()
    }

    /// Every mode of a game currently restricted.
    pub fn game_pools(&self, game: &str) -> Vec<Arc<Pool>> {
        self.game_pool
            .lock()
            .unwrap()
            .iter()
            .filter(|((g, _), _)| g == game)
            .map(|(_, p)| p.clone())
            .collect()
    }

    pub fn set_session_force(&self, session_id: &str, game: &str, force: Option<(String, u64)>) {
        self.update(session_id, game, |s| s.force = force);
    }

    /// Forçage posé par la console du joueur.
    pub fn set_console_force(
        &self,
        session_id: &str,
        game: &str,
        force: Option<(String, u64)>,
        once: bool,
    ) {
        self.update(session_id, game, |s| {
            s.force = force;
            s.force_once = once;
        });
    }

    /// Retire le forçage après usage (mode « prochain spin seulement »).
    pub fn consume_force(&self, session_id: &str) {
        let mut map = self.inner.lock().unwrap();
        if let Some(s) = map.get_mut(session_id) {
            if s.force_once {
                s.force = None;
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_sessions_draw_from_the_plain_lut() {
        let sessions = Sessions::default();
        assert!(sessions.get_or_create("ssp-7-1727430000000", "g").plain_draw());
        assert!(!sessions.get_or_create("play-tab", "g").plain_draw());
        // L'ID fait foi même quand la session naît d'une mise à jour (authenticate).
        assert!(sessions.update("ssp-8-1", "g", |_| {}).external);
    }

    #[test]
    fn other_sessions_are_capped() {
        let sessions = Sessions::default();
        for i in 0..MAX_OTHER_SESSIONS + 50 {
            sessions.get_or_create(&format!("ssp-{i}-1"), "g");
        }
        assert_eq!(sessions.list(None).len(), MAX_OTHER_SESSIONS);
        // The most recent one survives, the first ones went.
        assert!(sessions.get(&format!("ssp-{}-1", MAX_OTHER_SESSIONS + 49)).is_some());
        assert!(sessions.get("ssp-0-1").is_none());
    }
}
