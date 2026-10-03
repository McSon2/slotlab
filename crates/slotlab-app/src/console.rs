//! Console de démonstration injectée dans la page du jeu.
//!
//! Elle laisse le visiteur choisir le prochain spin — mode, gain visé,
//! critère — pour rejouer une animation précise (un gros gain, un bonus,
//! le max win) sans attendre que le hasard la lui donne. C'est un outil de
//! présentation client, activé par jeu dans son `game.json`.
//!
//! Le forçage passe par le mécanisme de session qui existait déjà côté RGS
//! (`Session::force`), donc le jeu n'a rien à savoir de tout ça : il reçoit
//! un spin normal.

use crate::hub::HubGame;
use crate::state::AppState;
use axum::extract::{Extension, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/console/meta", get(meta))
        .route("/api/console/search", post(search))
        .route("/api/console/force", post(force))
        .route("/api/console/state", get(state_of))
}

fn enabled(game: &Arc<HubGame>) -> Result<(), Response> {
    if game.manifest.console {
        Ok(())
    } else {
        Err((StatusCode::NOT_FOUND, "console désactivée").into_response())
    }
}

/// Modes jouables, critères et bornes de gain — de quoi remplir le panneau.
async fn meta(State(state): State<AppState>, Extension(game): Extension<Arc<HubGame>>) -> Response {
    if let Err(r) = enabled(&game) {
        return r;
    }
    let Some(loaded) = state.registry.get(&game.manifest.name) else {
        return (StatusCode::NOT_FOUND, "jeu non chargé").into_response();
    };
    let modes: Vec<_> = loaded
        .modes
        .iter()
        .map(|m| {
            let max = m
                .lut
                .rows
                .iter()
                .map(|r| r.payout_x100)
                .max()
                .unwrap_or(0) as f64
                / 100.0;
            let criteria = state
                .books
                .mode_books(&loaded, &m.config.name)
                .ok()
                .map(|b| b.index.criteria_names.clone())
                .unwrap_or_default();
            json!({
                "mode": m.config.name,
                "cost": m.config.cost,
                "maxPayout": max,
                "criteria": criteria,
            })
        })
        .collect();
    Json(json!({ "game": game.manifest.name, "modes": modes })).into_response()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchReq {
    mode: String,
    #[serde(default)]
    payout_min: Option<f64>,
    #[serde(default)]
    payout_max: Option<f64>,
    #[serde(default)]
    criteria: Option<String>,
    /// `any` (défaut), `with` ou `without` : présence du bonus dans le tour.
    #[serde(default)]
    bonus: crate::search::BonusFilter,
    #[serde(default)]
    limit: Option<usize>,
}

/// Books correspondant au filtre. On lit l'index (.idx), jamais les books
/// eux-mêmes : une recherche coûte quelques millisecondes même sur 500 000
/// entrées.
async fn search(
    State(state): State<AppState>,
    Extension(game): Extension<Arc<HubGame>>,
    Json(req): Json<SearchReq>,
) -> Response {
    if let Err(r) = enabled(&game) {
        return r;
    }
    let Some(loaded) = state.registry.get(&game.manifest.name) else {
        return (StatusCode::NOT_FOUND, "jeu non chargé").into_response();
    };
    let Some(mode) = loaded.resolve_mode(&req.mode) else {
        return (StatusCode::BAD_REQUEST, "mode inconnu").into_response();
    };
    let Ok(books) = state.books.mode_books(&loaded, &mode.config.name) else {
        return (StatusCode::SERVICE_UNAVAILABLE, "books absents").into_response();
    };
    let idx = &books.index;
    let min = req.payout_min.map(|v| (v * 100.0).round() as u64);
    let max = req.payout_max.map(|v| (v * 100.0).round() as u64);
    let crit = req.criteria.as_ref().filter(|c| !c.is_empty()).map(|c| {
        idx.criteria_names
            .iter()
            .position(|n| n.eq_ignore_ascii_case(c))
            .map(|i| i as u16)
    });
    // Critère demandé mais absent de ce mode : zéro résultat, pas tous.
    if matches!(crit, Some(None)) {
        return Json(json!({ "total": 0, "hits": [] })).into_response();
    }
    let crit = crit.flatten();
    let limit = req.limit.unwrap_or(40).min(200);

    let mut found: Vec<&crate::books::format::BookRecord> = Vec::new();
    for b in &idx.books {
        if min.is_some_and(|m| b.payout_x100 < m) || max.is_some_and(|m| b.payout_x100 > m) {
            continue;
        }
        if crit.is_some_and(|c| b.criteria != c) {
            continue;
        }
        // Sans ce filtre, « gros gain » ne rend que des tours à bonus : au-delà
        // de quelques dizaines de x, les books de freegame sont bien plus
        // nombreux que ceux du jeu de base et occupent seuls la page.
        if !req.bonus.keeps(b.fs_triggers) {
            continue;
        }
        found.push(b);
    }
    let total = found.len();
    // Les plus gros gains d'abord : c'est ce qu'on vient chercher, et l'ordre
    // des books (par sim_id) n'a aucun sens pour qui fait une démo.
    found.sort_by(|a, b| b.payout_x100.cmp(&a.payout_x100).then(a.sim_id.cmp(&b.sim_id)));
    let hits: Vec<_> = found
        .iter()
        .take(limit)
        .map(|b| {
            json!({
                "simId": b.sim_id,
                "payout": b.payout_x100 as f64 / 100.0,
                "criteria": idx.criteria_names.get(b.criteria as usize),
                "fsTriggers": b.fs_triggers,
                "retriggers": b.fs_retriggers,
            })
        })
        .collect();
    Json(json!({ "total": total, "hits": hits })).into_response()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ForceReq {
    #[serde(rename = "sessionID")]
    session_id: String,
    /// null → annule le forçage.
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    sim_id: Option<u64>,
    /// false → le book reste imposé jusqu'à annulation.
    #[serde(default = "yes")]
    once: bool,
}

fn yes() -> bool {
    true
}

async fn force(
    State(state): State<AppState>,
    Extension(game): Extension<Arc<HubGame>>,
    Json(req): Json<ForceReq>,
) -> Response {
    if let Err(r) = enabled(&game) {
        return r;
    }
    // Une session d'un AUTRE jeu ne doit pas être pilotable depuis ici.
    if !state
        .sessions
        .is_live_shared(&req.session_id, &game.manifest.name)
    {
        return (StatusCode::FORBIDDEN, "session inconnue").into_response();
    }
    let force = match (req.mode.as_ref(), req.sim_id) {
        (Some(m), Some(sim)) => {
            let Some(loaded) = state.registry.get(&game.manifest.name) else {
                return (StatusCode::NOT_FOUND, "jeu non chargé").into_response();
            };
            let Some(mode) = loaded.resolve_mode(m) else {
                return (StatusCode::BAD_REQUEST, "mode inconnu").into_response();
            };
            // Un sim_id absent de la LUT ferait échouer le spin côté RGS
            // avec une erreur illisible : on refuse tout de suite.
            if !mode.lut.rows.iter().any(|r| r.sim_id == sim) {
                return (StatusCode::BAD_REQUEST, "book absent de la table").into_response();
            }
            Some((mode.config.name.clone(), sim))
        }
        _ => None,
    };
    state.sessions.set_console_force(
        &req.session_id,
        &game.manifest.name,
        force.clone(),
        req.once,
    );
    Json(json!({ "ok": true, "forced": force.map(|(m, s)| json!({ "mode": m, "simId": s })) }))
        .into_response()
}

#[derive(Deserialize)]
struct StateQuery {
    #[serde(rename = "sessionID")]
    session_id: String,
}

async fn state_of(
    State(state): State<AppState>,
    Extension(game): Extension<Arc<HubGame>>,
    Query(q): Query<StateQuery>,
) -> Response {
    if let Err(r) = enabled(&game) {
        return r;
    }
    let forced = state
        .sessions
        .get(&q.session_id)
        .and_then(|s| s.force.map(|(m, sim)| json!({ "mode": m, "simId": sim })));
    Json(json!({ "forced": forced })).into_response()
}

/// Passerelle injectée à la place du panneau quand le jeu est affiché dans
/// l'onglet Play de Reelhouse (`embed=1`).
///
/// Reelhouse dessine sa propre console à côté du jeu et appelle les mêmes
/// routes `/api/console/*`, mais il ne peut pas lire la session : elle est
/// dans l'adresse de l'iframe, d'une autre origine. La page la lui envoie,
/// avec l'adresse de base (clé comprise) à laquelle joindre la console.
/// Envoyée au chargement, puis à chaque `slotlab:hello` — Reelhouse peut
/// écouter après le premier envoi.
pub const BRIDGE: &str = r##"
<script>
(() => {
  if (window.parent === window) return;
  const q = new URLSearchParams(location.search);
  const sid = q.get('sessionID');
  if (!sid) return;
  const key = (location.pathname.match(/^\/k\/[^/]+/) || [''])[0];
  const msg = { source: 'slotlab', type: 'session', sessionID: sid, base: location.origin + key };
  window.parent.postMessage(msg, '*');
  window.addEventListener('message', e => {
    if (e.data && e.data.type === 'slotlab:hello' && e.source) e.source.postMessage(msg, e.origin);
  });
})();
</script>
"##;

/// Panneau injecté avant `</body>` de la page du jeu.
///
/// Volontairement autonome : pas de dépendance, pas de style hérité du jeu,
/// et un `z-index` assez haut pour passer au-dessus du canvas Pixi.
pub const WIDGET: &str = r##"
<div id="slbc" data-slotlab-console>
<style>
  #slbc, #slbc * { box-sizing: border-box; font-family: ui-sans-serif, system-ui,
    -apple-system, "Segoe UI", sans-serif; }
  #slbc { position: fixed; z-index: 2147483000; left: 12px; bottom: 12px;
    color: #e8e2d9; font-size: 13px; }
  #slbc .tab { display: inline-flex; align-items: center; gap: 7px; padding: 8px 13px;
    background: #17141ae6; border: 1px solid #35303b; border-radius: 9px;
    cursor: pointer; backdrop-filter: blur(6px); user-select: none; }
  #slbc .tab:hover { background: #241f29f2; }
  #slbc .dot { width: 7px; height: 7px; border-radius: 50%; background: #6a6472; }
  #slbc.armed .dot { background: #d2a03c; box-shadow: 0 0 7px #d2a03c; }
  #slbc .panel { display: none; width: min(370px, calc(100vw - 24px));
    background: #141118f2; border: 1px solid #35303b; border-radius: 12px;
    padding: 14px; margin-bottom: 8px; backdrop-filter: blur(10px);
    box-shadow: 0 18px 50px #0000007a; max-height: min(78vh, 620px); overflow: auto; }
  #slbc.open .panel { display: block; }
  #slbc h4 { margin: 0 0 10px; font-size: 13px; letter-spacing: .02em; }
  #slbc .row { display: flex; gap: 8px; align-items: center; margin-bottom: 8px; }
  #slbc label { font-size: 11px; text-transform: uppercase; letter-spacing: .07em;
    color: #8d8593; display: block; margin-bottom: 4px; }
  #slbc select, #slbc input { width: 100%; padding: 7px 9px; font-size: 13px;
    color: #e8e2d9; background: #0e0c11; border: 1px solid #35303b; border-radius: 7px; }
  #slbc button { font: inherit; font-size: 12px; padding: 7px 11px; border-radius: 7px;
    border: 1px solid #35303b; background: #1e1a23; color: #e8e2d9; cursor: pointer; }
  #slbc button:hover { background: #292330; }
  #slbc button.go { background: #d2a03c; border-color: #d2a03c; color: #150f05;
    font-weight: 600; }
  #slbc button.go:hover { background: #e0ae47; }
  #slbc .presets { display: flex; flex-wrap: wrap; gap: 6px; margin-bottom: 10px; }
  #slbc .seg { display: flex; gap: 0; margin-bottom: 6px; }
  #slbc .seg button { flex: 1; border-radius: 0; border-right-width: 0; }
  #slbc .seg button:first-child { border-radius: 7px 0 0 7px; }
  #slbc .seg button:last-child { border-radius: 0 7px 7px 0; border-right-width: 1px; }
  #slbc .seg button.on { background: #d2a03c; border-color: #d2a03c; color: #150f05;
    font-weight: 600; }
  #slbc .donebox { display: none; margin-top: 10px; padding: 9px 10px; border-radius: 8px;
    background: #122019; border: 1px solid #2c5c41; color: #7fd6a1; font-size: 12px; }
  #slbc .donebox.show { display: block; }
  #slbc .hits { margin-top: 10px; border-top: 1px solid #2a2530; padding-top: 8px; }
  #slbc .hit { display: flex; justify-content: space-between; align-items: center;
    gap: 8px; padding: 6px 7px; border-radius: 6px; cursor: pointer; }
  #slbc .hit:hover { background: #221d29; }
  #slbc .hit b { color: #e8c98a; font-variant-numeric: tabular-nums; }
  #slbc .hit span { color: #8d8593; font-size: 11px; }
  #slbc .note { color: #8d8593; font-size: 11px; line-height: 1.45; }
  #slbc .armedbox { margin-top: 10px; padding: 9px 10px; border-radius: 8px;
    background: #241d0f; border: 1px solid #5c4a22; display: none; }
  #slbc.armed .armedbox { display: block; }
  #slbc .err { color: #e2757c; font-size: 12px; }
</style>
<div class="panel">
  <h4>Console de démonstration</h4>
  <p class="note">Choisis le résultat du prochain tour, puis lance-le normalement.
    La table de gains n'est pas modifiée&nbsp;: on sélectionne un tour qui existe
    déjà dans la math.</p>
  <div class="presets"></div>
  <div class="row">
    <div style="flex:1"><label>Mode</label><select id="slbc-mode"></select></div>
  </div>
  <label>Bonus</label>
  <div class="seg" id="slbc-bonus">
    <button data-bonus="any" class="on">Peu importe</button>
    <button data-bonus="without">Sans bonus</button>
    <button data-bonus="with">Avec bonus</button>
  </div>
  <p class="note" id="slbc-bonusnote"></p>
  <div class="row">
    <div style="flex:1"><label>Gain min (x)</label><input id="slbc-min" type="number" step="1" placeholder="0"></div>
    <div style="flex:1"><label>Gain max (x)</label><input id="slbc-max" type="number" step="1" placeholder="∞"></div>
    <button class="go" id="slbc-search" style="align-self:flex-end">Chercher</button>
  </div>
  <label>Appliquer le tour choisi à</label>
  <div class="seg" id="slbc-seg">
    <button data-once="1" class="on">Un seul tour</button>
    <button data-once="0">Tous les tours</button>
  </div>
  <p class="note" id="slbc-segnote"></p>
  <div class="armedbox">
    <div id="slbc-armedtxt"></div>
    <button id="slbc-cancel" style="margin-top:7px">Annuler</button>
  </div>
  <div class="donebox" id="slbc-done"></div>
  <div class="hits" id="slbc-hits"></div>
</div>
<div class="tab" id="slbc-tab"><span class="dot"></span>Console</div>
</div>
<script>
(() => {
  const $ = id => document.getElementById(id);
  const box = $('slbc');
  const sid = new URLSearchParams(location.search).get('sessionID');
  if (!sid) { box.remove(); return; }
  let meta = null, armed = null, watch = null, doneTimer = null;
  let once = true, bonus = 'any';

  // Entré par `/k/<jeton>/`, la page n'a pas de cookie : la clé doit
  // accompagner ses appels comme elle accompagne ceux du jeu.
  const key = (location.pathname.match(/^\/k\/[^/]+/) || [''])[0];
  const api = (path, opts) => fetch(key + '/api/console/' + path, opts).then(r =>
    r.ok ? r.json() : r.text().then(t => Promise.reject(new Error(t))));

  $('slbc-tab').onclick = () => box.classList.toggle('open');

  // Deux options nommées plutôt qu'une case à cocher : « Un seul tour »
  // décoché ne disait pas ce qui se passait à la place.
  function setOnce(v) {
    once = v;
    $('slbc-seg').querySelectorAll('button').forEach(b =>
      b.classList.toggle('on', (b.dataset.once === '1') === v));
    $('slbc-segnote').textContent = v
      ? "Le tour choisi sort une fois, puis le jeu repart au hasard."
      : "Le tour choisi sort à chaque spin, jusqu'à ce que tu annules.";
    if (armed) setArmed(armed);
  }
  $('slbc-seg').querySelectorAll('button').forEach(b =>
    b.onclick = () => setOnce(b.dataset.once === '1'));

  // Un « gros gain » du jeu de base et un gros gain de bonus n'ont rien a voir
  // a l'ecran : sans ce choix, la recherche ne rendait que des tours a bonus,
  // qui sont bien plus nombreux des qu'on monte en gain.
  function setBonus(v) {
    bonus = v;
    $('slbc-bonus').querySelectorAll('button').forEach(b =>
      b.classList.toggle('on', b.dataset.bonus === v));
    $('slbc-bonusnote').textContent = v === 'without'
      ? "Tours qui ne declenchent PAS les free spins (jeu de base uniquement)."
      : v === 'with'
        ? "Tours qui declenchent les free spins."
        : "Avec ou sans free spins.";
    $('slbc-hits').innerHTML = '';
  }
  $('slbc-bonus').querySelectorAll('button').forEach(b =>
    b.onclick = () => { setBonus(b.dataset.bonus); rerun(); });

  function setArmed(f) {
    armed = f;
    box.classList.toggle('armed', !!f);
    if (f) {
      const gain = f.payout != null ? f.payout.toLocaleString('fr-FR') + 'x' : '#' + f.simId;
      $('slbc-armedtxt').innerHTML = once
        ? `Armé pour <b>le prochain tour</b>&nbsp;: <b>${gain}</b>
           <span style="color:#8d8593">· ${f.mode}</span>`
        : `Armé pour <b>tous les tours</b>&nbsp;: <b>${gain}</b>
           <span style="color:#8d8593">· ${f.mode}</span>`;
      $('slbc-done').classList.remove('show');
    }
    // Sans cette surveillance, le bandeau resterait allumé après le tour
    // forcé et « Un seul tour » aurait l'air de ne rien faire.
    clearInterval(watch);
    watch = f ? setInterval(async () => {
      try {
        const s = await api('state?sessionID=' + encodeURIComponent(sid));
        if (!s.forced) {
          setArmed(null);
          // Dire que le tour est passé, sinon le bandeau disparaît sans
          // qu'on sache si le forçage a servi ou s'il a été perdu.
          const d = $('slbc-done');
          d.textContent = 'Tour joué — le jeu est reparti au hasard.';
          d.classList.add('show');
          clearTimeout(doneTimer);
          doneTimer = setTimeout(() => d.classList.remove('show'), 6000);
        }
      } catch (e) { /* réseau : on retente au tick suivant */ }
    }, 1500) : null;
  }

  // « 1 trigger(s) » s'affichait sur tout tour à bonus : ce qui distingue deux
  // bonus, c'est le nombre de retriggers, pas celui des déclenchements.
  const bonusTag = h => !h.fsTriggers && !h.retriggers ? ''
    : ' · bonus' + (h.retriggers
      ? ' · ' + h.retriggers + ' retrigger' + (h.retriggers > 1 ? 's' : '') : '');

  async function search(min, max) {
    const mode = $('slbc-mode').value;
    $('slbc-hits').innerHTML = '<p class="note">Recherche…</p>';
    try {
      const r = await api('search', { method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ mode, payoutMin: min, payoutMax: max, bonus }) });
      if (!r.hits.length) {
        $('slbc-hits').innerHTML = '<p class="note">Aucun tour ne correspond'
          + (bonus === 'without' ? " — le jeu de base ne monte pas si haut."
            : bonus === 'with' ? " — aucun bonus ne paie aussi peu." : '.')
          + '</p>';
        return;
      }
      $('slbc-hits').innerHTML =
        `<p class="note">${r.total.toLocaleString('fr-FR')} tour(s) — ${r.hits.length} affichés</p>` +
        r.hits.map(h => `<div class="hit" data-sim="${h.simId}" data-pay="${h.payout}">
          <b>${h.payout.toLocaleString('fr-FR')}x</b>
          <span>${h.criteria || ''}${bonusTag(h)} · #${h.simId}</span>
        </div>`).join('');
      $('slbc-hits').querySelectorAll('.hit').forEach(el => el.onclick = () =>
        arm(+el.dataset.sim, +el.dataset.pay));
    } catch (e) { $('slbc-hits').innerHTML = `<p class="err">${e.message}</p>`; }
  }

  async function arm(simId, payout) {
    const mode = $('slbc-mode').value;
    try {
      await api('force', { method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ sessionID: sid, mode, simId, once }) });
      setArmed({ mode, simId, payout });
    } catch (e) { $('slbc-hits').innerHTML = `<p class="err">${e.message}</p>`; }
  }

  $('slbc-cancel').onclick = async () => {
    await api('force', { method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ sessionID: sid }) });
    setArmed(null);
  };

  $('slbc-search').onclick = () => rerun();
  const num = v => v === '' ? null : Number(v);
  // Changer le filtre doit remontrer des resultats tout de suite, sinon il
  // faut re-cliquer « Chercher » et on croit que le filtre n'a rien fait.
  function rerun() {
    search(num($('slbc-min').value), num($('slbc-max').value));
  }

  setOnce(true);
  setBonus('any');

  api('meta').then(m => {
    meta = m;
    $('slbc-mode').innerHTML = m.modes.map(x =>
      `<option value="${x.mode}">${x.mode}${x.cost > 1 ? ' (' + x.cost + 'x)' : ''}</option>`).join('');
    $('slbc-mode').onchange = () => { $('slbc-hits').innerHTML = ''; };
    const base = m.modes[0];
    // Chaque preset porte aussi son filtre bonus : « gros gain en jeu de base »
    // et « bonus » sont deux demos differentes, pas deux bornes de gain.
    $('slbc').querySelector('.presets').innerHTML = [
      ['Petit gain', 1, 5, 'without'],
      ['Gros gain (base)', 20, null, 'without'],
      ['Bonus', 1, null, 'with'],
      ['Gros bonus', 200, null, 'with'],
      ['Max win', Math.floor(base.maxPayout * 0.999), null, 'any'],
    ].map(([l, a, b, f]) =>
      `<button data-a="${a}" data-b="${b ?? ''}" data-f="${f}">${l}</button>`).join('');
    $('slbc').querySelectorAll('.presets button').forEach(el => el.onclick = () => {
      $('slbc-min').value = el.dataset.a;
      $('slbc-max').value = el.dataset.b;
      setBonus(el.dataset.f);
      search(+el.dataset.a, el.dataset.b === '' ? null : +el.dataset.b);
    });
    // Un forçage peut survivre à un rechargement de page.
    api('state?sessionID=' + encodeURIComponent(sid))
      .then(s => { if (s.forced) setArmed(s.forced); }).catch(() => {});
  }).catch(() => box.remove());
})();
</script>
"##;
