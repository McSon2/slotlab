//! Retriggers de free spins, comptés partout avec la même règle : index de
//! recherche (import des books), console de démonstration, scan.
//!
//! ── Ce qu'est un retrigger ──────────────────────────────────────────────────
//! Une annonce, à l'intérieur d'un bonus et après son déclenchement, d'un
//! TOTAL de free spins strictement supérieur à tous ceux annoncés jusque-là.
//! Les jeux ne le disent pas tous de la même façon :
//!
//!   · math-sdk : un événement `freeSpinRetrigger` (`totalFs` = nouveau total),
//!     puis des `updateFreeSpin` qui répètent ce total (`total`) ;
//!   · BLOOD WATER : aucun événement dédié — chaque poisson mangé donne un
//!     spin, annoncé par `sharkFeed.hunts[].totalFs` (un objet par repas, le
//!     total monte de 1 à chaque fois), puis par `updateFreeSpin.total`.
//!
//! Ne compter que `freeSpinRetrigger` lit 0 sur le second ; compter chaque
//! `updateFreeSpin` dont le total bouge lirait deux fois chaque retrigger du
//! premier. Suivre le plus haut total annoncé lit les deux correctement :
//! chaque annonce qui le dépasse est UN retrigger, et son écho dans
//! `updateFreeSpin` n'en est pas un.
//!
//! Les pistes d'annonce, dans l'ordre du book :
//!   · `freeSpinTrigger.totalFs` — fixe la base, remise à zéro à chaque
//!     nouveau déclenchement ;
//!   · le `totalFs` de premier niveau de tout autre événement
//!     (`freeSpinRetrigger`…) ;
//!   · `updateFreeSpin.total` ;
//!   · le `totalFs` des objets portés par n'importe quel tableau d'un
//!     événement (`sharkFeed.hunts[]`).
//!
//! Un repas au plafond du bonus ré-annonce le même total : ce n'est pas un
//! retrigger. Une annonce hors bonus (le `totalFs: 0` d'un repas du jeu de
//! base) non plus. Un `freeSpinRetrigger` qui ne porte aucun total compte
//! quand même, une fois — la hausse qu'il provoque ensuite est son écho, pas
//! un second retrigger.
//!
//! ── Le format « actions » (UNDEAD KING, front du client) ────────────────────
//! Un événement n'y a pas de `type` : c'est une suite d'actions `{kind, data}`.
//! `BonusGames` (data = free spins attribués) ouvre le bonus, `FreeSpinsWin` le
//! clôt. Le palier est le nombre de symboles de déclenchement (`BONUS`,
//! `SCATTER`) du dernier `GridSnapshot` avant `BonusGames` — le front le déduit
//! de la même façon. Ce format ne connaît pas de retrigger.
//!
//! ── Les plateaux ────────────────────────────────────────────────────────────
//! Les symboles des plateaux révélés (`reveal.board`, `GridSnapshot.cols`) sont
//! comptés au passage, par nom : c'est ce que le joueur voit, low / high
//! (`crate::symbols`). L'`anticipation` du PREMIER `reveal` (le spin payé) dit si
//! les rouleaux ont ralenti pour le scatter manquant — ce que le joueur ressent
//! comme un near miss quand aucun bonus ne suit (BUBBLE HEX, 2026-10-09 : la
//! « Feature nearmiss » ne compte que les criteria forcés, la moitié du ressenti).
//!
//! ── Pourquoi un désérialiseur écrit à la main ───────────────────────────────
//! L'import lit chaque book de fichiers qui dépassent plusieurs Go une fois
//! décompressés. Un `serde_json::Value` par événement coûterait des milliers
//! d'allocations par book ; ici, un champ est soit l'une des pistes, soit
//! sauté sans être construit — et un tableau n'est ouvert que pour y chercher
//! `totalFs`. Les nombres sont lus avec tolérance : `total` est lu sur TOUS
//! les événements (l'ordre des clés ne garantit pas que `type` vienne avant),
//! et sur un événement qui n'est pas `updateFreeSpin` il peut valoir autre
//! chose qu'un nombre — ce qui ne doit jamais faire échouer un import.

use crate::symbols::{SymbolMap, SymbolTally};
use serde::de::{self, Deserialize, DeserializeSeed, Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};
use std::borrow::Cow;
use std::fmt;
use std::marker::PhantomData;

const TRIGGER: &str = "freeSpinTrigger";
const RETRIGGER: &str = "freeSpinRetrigger";
const UPDATE: &str = "updateFreeSpin";
const END: &str = "freeSpinEnd";
const REVEAL: &str = "reveal";
/// Format « actions » : ouverture du bonus (data = free spins attribués), fin
/// du bonus, plateau révélé.
const ACTION_BONUS: &str = "BonusGames";
const ACTION_END: &str = "FreeSpinsWin";
const ACTION_BOARD: &str = "GridSnapshot";

/// Symbole qui déclenche un bonus dans le format « actions ».
fn is_trigger_symbol(name: &str) -> bool {
    name.eq_ignore_ascii_case("BONUS") || name.eq_ignore_ascii_case("SCATTER")
}

// ─── Ce qu'un événement annonce ─────────────────────────────────────────────

/// Ce qu'un événement dit du total de free spins.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FsAnnounce {
    /// `totalFs` de premier niveau.
    pub total_fs: Option<f64>,
    /// `total` de premier niveau — pris en compte sur `updateFreeSpin` seulement.
    pub total: Option<f64>,
    /// `totalFs` des objets portés par les tableaux de l'événement, dans l'ordre.
    pub nested: Vec<f64>,
}

/// Rôle d'une clé d'événement pour la règle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsKey {
    TotalFs,
    Total,
    /// Tout le reste : parcouru pour les `totalFs` de ses objets s'il s'agit
    /// d'un tableau, sauté sinon.
    Other,
}

impl FsKey {
    pub fn of(name: &str) -> Self {
        match name {
            "totalFs" => FsKey::TotalFs,
            "total" => FsKey::Total,
            _ => FsKey::Other,
        }
    }
}

impl FsAnnounce {
    /// Lit la valeur d'un champ d'événement. À appeler depuis le `visit_map`
    /// d'un événement pour chaque clé qu'il ne lit pas lui-même — c'est ce qui
    /// permet à un autre désérialiseur d'événement (celui du scan) de capter
    /// les annonces au passage, sans relire le book.
    pub fn read<'de, A: MapAccess<'de>>(&mut self, key: FsKey, map: &mut A) -> Result<(), A::Error> {
        match key {
            FsKey::TotalFs => self.total_fs = map.next_value::<LooseNum>()?.0,
            FsKey::Total => self.total = map.next_value::<LooseNum>()?.0,
            FsKey::Other => {
                let arr = map.next_value::<NestedTotalFs>()?;
                self.nested.extend(arr.found);
            }
        }
        Ok(())
    }
}

// ─── La règle ───────────────────────────────────────────────────────────────

/// La règle, nourrie un événement à la fois, dans l'ordre du book.
#[derive(Debug, Clone, Default)]
pub struct RetriggerCounter {
    /// Entre un déclenchement (ou le premier événement propre aux free spins)
    /// et `freeSpinEnd`.
    in_bonus: bool,
    /// Plus haut total annoncé depuis le déclenchement. `None` : le
    /// déclenchement ne l'a pas dit, la première annonce le fixera.
    top: Option<f64>,
    /// `freeSpinRetrigger` déjà comptés sans total : la hausse que chacun
    /// provoque ensuite ne doit pas être comptée une seconde fois.
    unannounced: u32,
    count: u32,
    /// Bonus ouverts jusqu'ici (un achat qui démarre en freegame sans
    /// `freeSpinTrigger` compte pour le premier).
    bonuses: u32,
    /// Premier total annoncé du PREMIER bonus : les spins attribués à l'entrée.
    first_base: Option<f64>,
    /// Plus haut total annoncé du premier bonus : les spins joués, retriggers
    /// compris.
    first_top: Option<f64>,
}

impl RetriggerCounter {
    pub fn push(&mut self, typ: &str, fs: &FsAnnounce) {
        if typ.eq_ignore_ascii_case(TRIGGER) {
            // Nouveau bonus : sa base est ce qu'il annonce, rien n'est compté.
            self.in_bonus = true;
            self.unannounced = 0;
            self.bonuses += 1;
            self.top = fs
                .total_fs
                .into_iter()
                .chain(fs.nested.iter().copied())
                .reduce(f64::max);
            self.note_first();
            return;
        }
        if typ.eq_ignore_ascii_case(END) {
            self.in_bonus = false;
            self.top = None;
            self.unannounced = 0;
            return;
        }
        let retrigger = typ.eq_ignore_ascii_case(RETRIGGER);
        let update = typ.eq_ignore_ascii_case(UPDATE);
        // Ces deux-là n'existent qu'à l'intérieur d'un bonus, même quand le
        // jeu n'a pas émis de `freeSpinTrigger` (achat qui démarre en freegame).
        if retrigger || update {
            self.in_bonus = true;
            self.bonuses = self.bonuses.max(1);
        }
        if !self.in_bonus {
            return;
        }
        if retrigger && fs.total_fs.is_none() && fs.nested.is_empty() {
            self.count += 1;
            self.unannounced += 1;
        }
        if let Some(v) = fs.total_fs {
            self.announce(v);
        }
        if update {
            if let Some(v) = fs.total {
                self.announce(v);
            }
        }
        for &v in &fs.nested {
            self.announce(v);
        }
    }

    fn announce(&mut self, v: f64) {
        match self.top {
            None => {
                // Première annonce d'un bonus dont le déclenchement n'a rien
                // dit : c'est la base (ou le total qui suit un retrigger déjà
                // compté), pas une hausse.
                self.top = Some(v);
                self.unannounced = 0;
            }
            Some(top) if v > top => {
                self.top = Some(v);
                if self.unannounced > 0 {
                    self.unannounced -= 1;
                } else {
                    self.count += 1;
                }
            }
            Some(_) => {}
        }
        self.note_first();
    }

    /// Suit le total du premier bonus : sa première valeur connue (spins
    /// attribués) et sa plus haute (spins joués).
    fn note_first(&mut self) {
        if self.bonuses > 1 {
            return;
        }
        if let Some(top) = self.top {
            self.first_base.get_or_insert(top);
            self.first_top = Some(self.first_top.map_or(top, |t| t.max(top)));
        }
    }

    pub fn count(&self) -> u32 {
        self.count
    }

    /// (spins attribués, spins joués) du premier bonus ; 0 si aucun total
    /// n'a été annoncé.
    pub fn first_bonus_spins(&self) -> (u32, u32) {
        let as_u32 = |v: Option<f64>| v.map_or(0, |v| v.max(0.0).round().min(u32::MAX as f64) as u32);
        (as_u32(self.first_base), as_u32(self.first_top))
    }
}

/// Retriggers d'une suite d'événements (un book).
pub fn count_retriggers(events: &[FsEvent<'_>]) -> u32 {
    summarize(events).retriggers
}

/// Ce qu'un book dit de ses free spins, lu en une passe.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BookFs {
    /// Événements `freeSpinTrigger`.
    pub triggers: u32,
    /// Palier du bonus : nombre de `positions` du PREMIER `freeSpinTrigger`,
    /// c.-à-d. les symboles qui l'ont déclenché (2, 3, 4 scatters…). Aucun nom
    /// de criteria n'est lu : un `wincap` a le palier de son vrai
    /// déclenchement. 0 : pas de déclenchement, ou un déclenchement qui ne dit
    /// pas ses positions.
    pub tier: u32,
    /// Retriggers, selon la règle de ce module.
    pub retriggers: u32,
    /// Free spins attribués à l'entrée du premier bonus (premier total annoncé).
    pub spins_initial: u32,
    /// Free spins du premier bonus au bout du compte, retriggers compris (plus
    /// haut total annoncé).
    pub spins_final: u32,
    /// Symboles payants low / high des plateaux révélés, hors bonus et en free
    /// spins (`crate::symbols`).
    pub symbols: SymbolTally,
    /// Le premier `reveal` (le spin payé) annonce le scatter manquant : au moins
    /// un rouleau de son `anticipation` est non nul.
    pub anticipation: bool,
}

pub fn summarize(events: &[FsEvent<'_>]) -> BookFs {
    summarize_with(events, &SymbolMap::default())
}

/// `summarize`, les noms de symboles lus à travers `symbols` (la table du front
/// d'un jeu qui ne nomme pas ses symboles `H*` / `L*`).
pub fn summarize_with(events: &[FsEvent<'_>], symbols: &SymbolMap) -> BookFs {
    let mut out = BookFs::default();
    let mut counter = RetriggerCounter::default();
    // Entre un déclenchement et la fin du bonus : les plateaux sans `gameType`
    // (format « actions ») sont ceux des free spins.
    let mut in_fs = false;
    // Format « actions » : symboles de déclenchement du dernier plateau.
    let mut last_triggers = 0u32;
    let mut first_reveal = true;
    for ev in events {
        counter.push(&ev.typ, &ev.fs);
        if ev.typ.eq_ignore_ascii_case(TRIGGER) {
            if out.triggers == 0 {
                out.tier = ev.tier.or(ev.positions).unwrap_or(0);
            }
            out.triggers += 1;
            in_fs = true;
        } else if ev.typ.eq_ignore_ascii_case(END) {
            in_fs = false;
        } else if ev.typ.eq_ignore_ascii_case(REVEAL) {
            if first_reveal {
                out.anticipation = ev.anticipation;
                first_reveal = false;
            }
            if let Some(board) = &ev.board {
                let fs = ev.game_type.as_deref().map_or(in_fs, |g| g.eq_ignore_ascii_case("freegame"));
                board.tally(symbols, fs, &mut out.symbols);
            }
        }
        for a in &ev.actions {
            match a.kind.as_ref() {
                ACTION_BOARD => {
                    if let Some(board) = &a.board {
                        last_triggers = board.count(is_trigger_symbol);
                        board.tally(symbols, in_fs, &mut out.symbols);
                    }
                }
                ACTION_BONUS => {
                    let announce = FsAnnounce { total_fs: a.number, ..Default::default() };
                    counter.push(TRIGGER, &announce);
                    if out.triggers == 0 {
                        out.tier = last_triggers;
                    }
                    out.triggers += 1;
                    in_fs = true;
                }
                ACTION_END => {
                    counter.push(END, &FsAnnounce::default());
                    in_fs = false;
                }
                _ => {}
            }
        }
    }
    out.retriggers = counter.count();
    (out.spins_initial, out.spins_final) = counter.first_bonus_spins();
    out
}

/// Retriggers d'un book, depuis sa ligne JSON.
pub fn book_retriggers(line: &str) -> Result<u32, serde_json::Error> {
    let book: FsBook = serde_json::from_str(line)?;
    Ok(count_retriggers(&book.events))
}

// ─── Lecture ────────────────────────────────────────────────────────────────

/// Un événement tel que la règle le lit : son type et ses annonces. C'est
/// aussi tout ce que l'index de recherche a besoin de savoir d'un événement.
#[derive(Debug, Clone, Default)]
pub struct FsEvent<'a> {
    pub typ: Cow<'a, str>,
    pub fs: FsAnnounce,
    /// Nombre d'éléments de `positions` quand l'événement en porte (sur un
    /// `freeSpinTrigger` : les symboles qui l'ont déclenché).
    pub positions: Option<u32>,
    /// Palier EXPLICITE d'un `freeSpinTrigger` (`"tier": 4`) : un jeu à plusieurs
    /// symboles de bonus déclenche des bonus DIFFÉRENTS avec le même nombre de
    /// symboles (SAUCER PANIC, 2026-10-12 : 3 LAB EXPERIMENT = Bonus, 3 FULL
    /// INVASION = Super) — compter les positions les confondait en « 3 scatters ».
    /// Prioritaire sur `positions` quand il est présent.
    pub tier: Option<u32>,
    /// `gameType` du math-sdk (`basegame` / `freegame`) : à quelle partie du
    /// round appartient le plateau d'un `reveal`.
    pub game_type: Option<Cow<'a, str>>,
    /// Les symboles de `board` — comptés sur un `reveal` seulement.
    pub board: Option<BoardSyms<'a>>,
    /// `anticipation` non nul (au moins un rouleau ralenti) — lu sur un `reveal`.
    pub anticipation: bool,
    /// Format « actions » : l'événement est une suite d'actions `{kind, data}`.
    pub actions: Vec<FsAction<'a>>,
}

impl FsEvent<'_> {
    /// Ce que l'événement montre, pour l'index de recherche : son `type`, puis
    /// le `kind` de chacune de ses actions.
    pub fn kinds(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.typ.as_ref())
            .chain(self.actions.iter().map(|a| a.kind.as_ref()))
            .filter(|k| !k.is_empty())
    }
}

/// Un book réduit à ses événements.
#[derive(Debug, Default, serde::Deserialize)]
pub struct FsBook<'a> {
    #[serde(default, borrow)]
    pub events: Vec<FsEvent<'a>>,
}

enum FsEventKey {
    Type,
    Positions,
    Tier,
    GameType,
    Board,
    Anticipation,
    Actions,
    Fs(FsKey),
}

impl<'de> Deserialize<'de> for FsEventKey {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = FsEventKey;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("an event field name")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<FsEventKey, E> {
                Ok(match v {
                    "type" => FsEventKey::Type,
                    "positions" => FsEventKey::Positions,
                    "tier" => FsEventKey::Tier,
                    "gameType" => FsEventKey::GameType,
                    "board" => FsEventKey::Board,
                    "anticipation" => FsEventKey::Anticipation,
                    "actions" => FsEventKey::Actions,
                    other => FsEventKey::Fs(FsKey::of(other)),
                })
            }
        }
        d.deserialize_identifier(V)
    }
}

impl<'de: 'a, 'a> Deserialize<'de> for FsEvent<'a> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V<'a>(PhantomData<FsEvent<'a>>);
        impl<'de: 'a, 'a> Visitor<'de> for V<'a> {
            type Value = FsEvent<'a>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a book event")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<FsEvent<'a>, A::Error> {
                let mut ev = FsEvent::default();
                while let Some(key) = map.next_key::<FsEventKey>()? {
                    match key {
                        FsEventKey::Type => ev.typ = map.next_value::<CowStr<'a>>()?.0,
                        FsEventKey::Positions => {
                            // Lu comme tout autre tableau (ses objets restent
                            // une piste d'annonce), en retenant sa longueur.
                            let arr = map.next_value::<NestedTotalFs>()?;
                            ev.positions = arr.len;
                            ev.fs.nested.extend(arr.found);
                        }
                        FsEventKey::Tier => ev.tier = map.next_value::<OptCount>()?.0,
                        FsEventKey::GameType => ev.game_type = map.next_value::<Cell<'a>>()?.0,
                        FsEventKey::Board => {
                            let board = map.next_value::<BoardSyms<'a>>()?;
                            ev.board = (!board.0.is_empty()).then_some(board);
                        }
                        FsEventKey::Anticipation => ev.anticipation = map.next_value::<NonZero>()?.0,
                        FsEventKey::Actions => ev.actions = map.next_value::<Actions<'a>>()?.0,
                        FsEventKey::Fs(k) => ev.fs.read(k, &mut map)?,
                    }
                }
                Ok(ev)
            }
        }
        d.deserialize_map(V(PhantomData))
    }
}

/// Un entier positif (`"tier": 4`) ; toute autre valeur — chaîne, objet, tableau,
/// `null`, négatif — vaut `None` : un champ inattendu ne fait jamais échouer un import.
struct OptCount(Option<u32>);

impl<'de> Deserialize<'de> for OptCount {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = OptCount;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a count")
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<OptCount, E> {
                Ok(OptCount(u32::try_from(v).ok().filter(|&n| n > 0)))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<OptCount, E> {
                Ok(OptCount(u32::try_from(v).ok().filter(|&n| n > 0)))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<OptCount, E> {
                Ok(OptCount((v.fract() == 0.0 && v >= 1.0 && v <= u32::MAX as f64).then_some(v as u32)))
            }
            fn visit_bool<E: de::Error>(self, _: bool) -> Result<OptCount, E> {
                Ok(OptCount(None))
            }
            fn visit_str<E: de::Error>(self, _: &str) -> Result<OptCount, E> {
                Ok(OptCount(None))
            }
            fn visit_unit<E: de::Error>(self) -> Result<OptCount, E> {
                Ok(OptCount(None))
            }
            fn visit_none<E: de::Error>(self) -> Result<OptCount, E> {
                Ok(OptCount(None))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<OptCount, A::Error> {
                while seq.next_element::<IgnoredAny>()?.is_some() {}
                Ok(OptCount(None))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<OptCount, A::Error> {
                while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
                Ok(OptCount(None))
            }
        }
        d.deserialize_any(V)
    }
}

/// Un nombre non nul, ou un tableau qui en contient un (`anticipation:
/// [0, 0, 1, 1, 1]`). Toute autre valeur — chaîne, objet, `null` — vaut `false` :
/// un champ inattendu ne doit jamais faire échouer un import.
struct NonZero(bool);

impl<'de> Deserialize<'de> for NonZero {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = NonZero;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a number or an array of numbers")
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<NonZero, E> {
                Ok(NonZero(v))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<NonZero, E> {
                Ok(NonZero(v != 0))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<NonZero, E> {
                Ok(NonZero(v != 0))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<NonZero, E> {
                Ok(NonZero(v != 0.0 && v.is_finite()))
            }
            fn visit_str<E: de::Error>(self, _: &str) -> Result<NonZero, E> {
                Ok(NonZero(false))
            }
            fn visit_unit<E: de::Error>(self) -> Result<NonZero, E> {
                Ok(NonZero(false))
            }
            fn visit_none<E: de::Error>(self) -> Result<NonZero, E> {
                Ok(NonZero(false))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<NonZero, A::Error> {
                let mut any = false;
                while let Some(NonZero(v)) = seq.next_element::<NonZero>()? {
                    any |= v;
                }
                Ok(NonZero(any))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<NonZero, A::Error> {
                while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
                Ok(NonZero(false))
            }
        }
        d.deserialize_any(V)
    }
}

/// Chaîne empruntée à la ligne quand c'est possible (le cas normal : un type
/// d'événement ne contient jamais d'échappement), copiée sinon.
struct CowStr<'a>(Cow<'a, str>);

impl<'de: 'a, 'a> Deserialize<'de> for CowStr<'a> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V<'a>(PhantomData<&'a ()>);
        impl<'de: 'a, 'a> Visitor<'de> for V<'a> {
            type Value = CowStr<'a>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a string")
            }
            fn visit_borrowed_str<E: de::Error>(self, v: &'de str) -> Result<CowStr<'a>, E> {
                Ok(CowStr(Cow::Borrowed(v)))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<CowStr<'a>, E> {
                Ok(CowStr(Cow::Owned(v.to_owned())))
            }
            fn visit_string<E: de::Error>(self, v: String) -> Result<CowStr<'a>, E> {
                Ok(CowStr(Cow::Owned(v)))
            }
        }
        d.deserialize_str(V(PhantomData))
    }
}

/// Un nombre, ou rien : toute autre valeur (chaîne, objet, `null`…) se lit
/// comme une absence au lieu d'être une erreur.
struct LooseNum(Option<f64>);

impl<'de> Deserialize<'de> for LooseNum {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = LooseNum;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("any JSON value")
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<LooseNum, E> {
                Ok(LooseNum(Some(v as f64)))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<LooseNum, E> {
                Ok(LooseNum(Some(v as f64)))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<LooseNum, E> {
                Ok(LooseNum(v.is_finite().then_some(v)))
            }
            fn visit_bool<E: de::Error>(self, _: bool) -> Result<LooseNum, E> {
                Ok(LooseNum(None))
            }
            fn visit_str<E: de::Error>(self, _: &str) -> Result<LooseNum, E> {
                Ok(LooseNum(None))
            }
            fn visit_unit<E: de::Error>(self) -> Result<LooseNum, E> {
                Ok(LooseNum(None))
            }
            fn visit_none<E: de::Error>(self) -> Result<LooseNum, E> {
                Ok(LooseNum(None))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<LooseNum, A::Error> {
                drain_seq(seq)?;
                Ok(LooseNum(None))
            }
            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<LooseNum, A::Error> {
                drain_map(map)?;
                Ok(LooseNum(None))
            }
        }
        d.deserialize_any(V)
    }
}

/// Les scalaires ne portent aucune annonce : ils se lisent comme « rien ».
macro_rules! scalars_are_nothing {
    ($nothing:expr) => {
        fn visit_bool<E: de::Error>(self, _: bool) -> Result<Self::Value, E> {
            Ok($nothing)
        }
        fn visit_i64<E: de::Error>(self, _: i64) -> Result<Self::Value, E> {
            Ok($nothing)
        }
        fn visit_u64<E: de::Error>(self, _: u64) -> Result<Self::Value, E> {
            Ok($nothing)
        }
        fn visit_f64<E: de::Error>(self, _: f64) -> Result<Self::Value, E> {
            Ok($nothing)
        }
        fn visit_str<E: de::Error>(self, _: &str) -> Result<Self::Value, E> {
            Ok($nothing)
        }
        fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok($nothing)
        }
        fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok($nothing)
        }
    };
}

/// La valeur d'un champ d'événement quelconque, lue seulement pour les
/// `totalFs` des objets qu'elle contient quand c'est un tableau
/// (`sharkFeed.hunts[]`) — et sa longueur. Un plateau (tableau de tableaux),
/// un objet ou un scalaire sont sautés sans être construits.
#[derive(Default)]
pub struct NestedTotalFs {
    /// `totalFs` des objets du tableau, dans l'ordre.
    pub found: Vec<f64>,
    /// Nombre d'éléments, quand la valeur est un tableau.
    pub len: Option<u32>,
}

impl<'de> Deserialize<'de> for NestedTotalFs {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = NestedTotalFs;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("any JSON value")
            }
            scalars_are_nothing!(NestedTotalFs::default());
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<NestedTotalFs, A::Error> {
                let mut out = NestedTotalFs {
                    found: Vec::new(),
                    len: Some(0),
                };
                while let Some(item) = seq.next_element::<ItemTotalFs>()? {
                    out.found.extend(item.0);
                    out.len = out.len.map(|n| n.saturating_add(1));
                }
                Ok(out)
            }
            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<NestedTotalFs, A::Error> {
                drain_map(map)?;
                Ok(NestedTotalFs::default())
            }
        }
        d.deserialize_any(V)
    }
}

/// Un élément de tableau : son `totalFs` si c'est un objet qui en porte un.
struct ItemTotalFs(Option<f64>);

impl<'de> Deserialize<'de> for ItemTotalFs {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = ItemTotalFs;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("any JSON value")
            }
            scalars_are_nothing!(ItemTotalFs(None));
            fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<ItemTotalFs, A::Error> {
                drain_seq(seq)?;
                Ok(ItemTotalFs(None))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<ItemTotalFs, A::Error> {
                let mut found = None;
                while let Some(key) = map.next_key::<IsTotalFs>()? {
                    if key.0 {
                        found = map.next_value::<LooseNum>()?.0;
                    } else {
                        map.next_value::<IgnoredAny>()?;
                    }
                }
                Ok(ItemTotalFs(found))
            }
        }
        d.deserialize_any(V)
    }
}

/// Une clé d'objet : est-ce `totalFs` ? (Lue sans allocation.)
struct IsTotalFs(bool);

impl<'de> Deserialize<'de> for IsTotalFs {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = IsTotalFs;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a field name")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<IsTotalFs, E> {
                Ok(IsTotalFs(v == "totalFs"))
            }
        }
        d.deserialize_identifier(V)
    }
}

fn drain_seq<'de, A: SeqAccess<'de>>(mut seq: A) -> Result<(), A::Error> {
    while seq.next_element::<IgnoredAny>()?.is_some() {}
    Ok(())
}

fn drain_map<'de, A: MapAccess<'de>>(mut map: A) -> Result<(), A::Error> {
    while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
    Ok(())
}

// ─── Plateaux et actions ────────────────────────────────────────────────────

/// Les scalaires autres que le texte ne portent rien.
macro_rules! non_text_is_nothing {
    ($nothing:expr) => {
        fn visit_bool<E: de::Error>(self, _: bool) -> Result<Self::Value, E> {
            Ok($nothing)
        }
        fn visit_i64<E: de::Error>(self, _: i64) -> Result<Self::Value, E> {
            Ok($nothing)
        }
        fn visit_u64<E: de::Error>(self, _: u64) -> Result<Self::Value, E> {
            Ok($nothing)
        }
        fn visit_f64<E: de::Error>(self, _: f64) -> Result<Self::Value, E> {
            Ok($nothing)
        }
        fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok($nothing)
        }
        fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok($nothing)
        }
    };
}

/// Les symboles d'un plateau, par nom, dans l'ordre d'apparition. Lu sans
/// construire le plateau : un nom est emprunté à la ligne, et une case objet
/// (`{"name": "H1", "wild": true}`) n'est lue que pour son `name`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BoardSyms<'a>(pub Vec<(Cow<'a, str>, u16)>);

impl<'a> BoardSyms<'a> {
    fn add(&mut self, name: Cow<'a, str>) {
        match self.0.iter_mut().find(|(n, _)| *n == name) {
            Some((_, c)) => *c = c.saturating_add(1),
            None => self.0.push((name, 1)),
        }
    }

    /// Cases dont le nom vérifie `pred`.
    pub fn count(&self, pred: impl Fn(&str) -> bool) -> u32 {
        self.0.iter().filter(|(n, _)| pred(n)).map(|&(_, c)| c as u32).sum()
    }

    fn tally(&self, symbols: &SymbolMap, free_spins: bool, out: &mut SymbolTally) {
        for (name, n) in &self.0 {
            if let Some(class) = symbols.classify(name) {
                out.add(class, *n as u32, free_spins);
            }
        }
    }
}

impl<'de: 'a, 'a> Deserialize<'de> for BoardSyms<'a> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V<'a>(PhantomData<&'a ()>);
        impl<'de: 'a, 'a> Visitor<'de> for V<'a> {
            type Value = BoardSyms<'a>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("any JSON value")
            }
            scalars_are_nothing!(BoardSyms::default());
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<BoardSyms<'a>, A::Error> {
                let mut out = BoardSyms::default();
                while seq.next_element_seed(Column(&mut out))?.is_some() {}
                Ok(out)
            }
            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<BoardSyms<'a>, A::Error> {
                drain_map(map)?;
                Ok(BoardSyms::default())
            }
        }
        d.deserialize_any(V(PhantomData))
    }
}

/// Un élément de plateau, versé dans le plateau en cours : une colonne
/// (tableau de cases), ou une case seule d'un plateau à plat.
struct Column<'b, 'a>(&'b mut BoardSyms<'a>);

impl<'de: 'a, 'a> DeserializeSeed<'de> for Column<'_, 'a> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de: 'a, 'a> Visitor<'de> for Column<'_, 'a> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a board column or cell")
    }
    non_text_is_nothing!(());
    fn visit_borrowed_str<E: de::Error>(self, v: &'de str) -> Result<(), E> {
        self.0.add(Cow::Borrowed(v));
        Ok(())
    }
    fn visit_str<E: de::Error>(self, v: &str) -> Result<(), E> {
        self.0.add(Cow::Owned(v.to_owned()));
        Ok(())
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        while let Some(cell) = seq.next_element::<Cell<'a>>()? {
            if let Some(name) = cell.0 {
                self.0.add(name);
            }
        }
        Ok(())
    }
    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<(), A::Error> {
        if let Some(name) = cell_name(map)? {
            self.0.add(name);
        }
        Ok(())
    }
}

/// Le `name` d'une case objet, le reste sauté.
fn cell_name<'de: 'a, 'a, A: MapAccess<'de>>(mut map: A) -> Result<Option<Cow<'a, str>>, A::Error> {
    let mut name = None;
    while let Some(key) = map.next_key::<IsKey<NAME>>()? {
        if key.0 {
            name = map.next_value::<Cell<'a>>()?.0;
        } else {
            map.next_value::<IgnoredAny>()?;
        }
    }
    Ok(name)
}

/// Un texte, ou rien : toute autre valeur se lit comme une absence. Sur une
/// case de plateau, un objet se lit par son `name`.
struct Cell<'a>(Option<Cow<'a, str>>);

impl<'de: 'a, 'a> Deserialize<'de> for Cell<'a> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V<'a>(PhantomData<&'a ()>);
        impl<'de: 'a, 'a> Visitor<'de> for V<'a> {
            type Value = Cell<'a>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a board cell")
            }
            non_text_is_nothing!(Cell(None));
            fn visit_borrowed_str<E: de::Error>(self, v: &'de str) -> Result<Cell<'a>, E> {
                Ok(Cell(Some(Cow::Borrowed(v))))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Cell<'a>, E> {
                Ok(Cell(Some(Cow::Owned(v.to_owned()))))
            }
            fn visit_string<E: de::Error>(self, v: String) -> Result<Cell<'a>, E> {
                Ok(Cell(Some(Cow::Owned(v))))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<Cell<'a>, A::Error> {
                drain_seq(seq)?;
                Ok(Cell(None))
            }
            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Cell<'a>, A::Error> {
                Ok(Cell(cell_name(map)?))
            }
        }
        d.deserialize_any(V(PhantomData))
    }
}

const NAME: u8 = 0;
const BOARD: u8 = 1;

/// Une clé d'objet : est-ce celle que `K` désigne ? (Lue sans allocation.)
/// `NAME` : le nom d'une case ; `BOARD` : le plateau d'un `data` d'action
/// (`cols`, `board`, `grid`).
struct IsKey<const K: u8>(bool);

impl<'de, const K: u8> Deserialize<'de> for IsKey<K> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V<const K: u8>;
        impl<'de, const K: u8> Visitor<'de> for V<K> {
            type Value = IsKey<K>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a field name")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<IsKey<K>, E> {
                Ok(IsKey(match K {
                    NAME => v == "name",
                    _ => matches!(v, "cols" | "board" | "grid"),
                }))
            }
        }
        d.deserialize_identifier(V::<K>)
    }
}

/// Une action du format « actions » : son `kind`, et ce que son `data` porte
/// d'utile — un nombre (`BonusGames` : free spins attribués) ou un plateau
/// (`GridSnapshot.cols`). Le reste de `data` est sauté.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FsAction<'a> {
    pub kind: Cow<'a, str>,
    pub number: Option<f64>,
    pub board: Option<BoardSyms<'a>>,
}

/// `actions` : un tableau d'actions, ou rien.
struct Actions<'a>(Vec<FsAction<'a>>);

impl<'de: 'a, 'a> Deserialize<'de> for Actions<'a> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V<'a>(PhantomData<&'a ()>);
        impl<'de: 'a, 'a> Visitor<'de> for V<'a> {
            type Value = Actions<'a>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("an array of actions")
            }
            scalars_are_nothing!(Actions(Vec::new()));
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Actions<'a>, A::Error> {
                let mut out = Vec::new();
                while let Some(a) = seq.next_element::<Action<'a>>()? {
                    out.extend(a.0);
                }
                Ok(Actions(out))
            }
            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Actions<'a>, A::Error> {
                drain_map(map)?;
                Ok(Actions(Vec::new()))
            }
        }
        d.deserialize_any(V(PhantomData))
    }
}

/// Un élément de `actions` : une action si c'est un objet, rien sinon.
struct Action<'a>(Option<FsAction<'a>>);

impl<'de: 'a, 'a> Deserialize<'de> for Action<'a> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V<'a>(PhantomData<&'a ()>);
        impl<'de: 'a, 'a> Visitor<'de> for V<'a> {
            type Value = Action<'a>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("an action")
            }
            scalars_are_nothing!(Action(None));
            fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<Action<'a>, A::Error> {
                drain_seq(seq)?;
                Ok(Action(None))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Action<'a>, A::Error> {
                let mut out = FsAction::default();
                while let Some(key) = map.next_key::<ActionKey>()? {
                    match key {
                        ActionKey::Kind => out.kind = map.next_value::<Cell<'a>>()?.0.unwrap_or_default(),
                        ActionKey::Data => {
                            let data = map.next_value::<ActionData<'a>>()?;
                            out.number = data.number;
                            out.board = data.board;
                        }
                        ActionKey::Other => {
                            map.next_value::<IgnoredAny>()?;
                        }
                    }
                }
                Ok(Action(Some(out)))
            }
        }
        d.deserialize_any(V(PhantomData))
    }
}

enum ActionKey {
    Kind,
    Data,
    Other,
}

impl<'de> Deserialize<'de> for ActionKey {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = ActionKey;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("an action field name")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<ActionKey, E> {
                Ok(match v {
                    "kind" => ActionKey::Kind,
                    "data" => ActionKey::Data,
                    _ => ActionKey::Other,
                })
            }
        }
        d.deserialize_identifier(V)
    }
}

/// Le `data` d'une action : un nombre, ou un objet qui porte un plateau.
#[derive(Default)]
struct ActionData<'a> {
    number: Option<f64>,
    board: Option<BoardSyms<'a>>,
}

impl<'de: 'a, 'a> Deserialize<'de> for ActionData<'a> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V<'a>(PhantomData<&'a ()>);
        impl<'de: 'a, 'a> Visitor<'de> for V<'a> {
            type Value = ActionData<'a>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("action data")
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<ActionData<'a>, E> {
                Ok(ActionData { number: Some(v as f64), board: None })
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<ActionData<'a>, E> {
                Ok(ActionData { number: Some(v as f64), board: None })
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<ActionData<'a>, E> {
                Ok(ActionData { number: v.is_finite().then_some(v), board: None })
            }
            fn visit_bool<E: de::Error>(self, _: bool) -> Result<ActionData<'a>, E> {
                Ok(ActionData::default())
            }
            fn visit_str<E: de::Error>(self, _: &str) -> Result<ActionData<'a>, E> {
                Ok(ActionData::default())
            }
            fn visit_unit<E: de::Error>(self) -> Result<ActionData<'a>, E> {
                Ok(ActionData::default())
            }
            fn visit_none<E: de::Error>(self) -> Result<ActionData<'a>, E> {
                Ok(ActionData::default())
            }
            fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<ActionData<'a>, A::Error> {
                drain_seq(seq)?;
                Ok(ActionData::default())
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<ActionData<'a>, A::Error> {
                let mut out = ActionData::default();
                while let Some(key) = map.next_key::<IsKey<BOARD>>()? {
                    if key.0 {
                        // `GridSnapshot` porte `"board": 0` puis `"cols": [[…]]` :
                        // seul un vrai plateau est retenu.
                        let board = map.next_value::<BoardSyms<'a>>()?;
                        if !board.0.is_empty() {
                            out.board = Some(board);
                        }
                    } else {
                        map.next_value::<IgnoredAny>()?;
                    }
                }
                Ok(out)
            }
        }
        d.deserialize_any(V(PhantomData))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book(events: &str) -> String {
        format!(r#"{{"id":1,"payoutMultiplier":100,"events":[{events}]}}"#)
    }

    fn count(events: &str) -> u32 {
        book_retriggers(&book(events)).expect("book JSON")
    }

    /// Un plateau et des gains, pour que les tableaux sans `totalFs` soient
    /// bien traversés sans rien produire.
    const REVEAL: &str = r#"{"index":0,"type":"reveal","gameType":"freegame",
        "board":[[{"name":"H1"},{"name":"S","scatter":true}],[{"name":"L1"},{"name":"W","wild":true}]],
        "paddingPositions":[3,9]}"#;
    const WIN: &str = r#"{"index":1,"type":"winInfo","totalWin":40,
        "wins":[{"symbol":"H1","kind":3,"win":40,"positions":[{"reel":0,"row":1}],"meta":{"ways":1}}]}"#;

    #[test]
    fn sdk_book_counts_each_retrigger_once() {
        // Un retrigger annoncé par `freeSpinRetrigger`, puis répété par chaque
        // `updateFreeSpin` : 2 retriggers, pas 4.
        let events = [
            r#"{"type":"freeSpinTrigger","totalFs":10,"positions":[{"reel":0,"row":1}]}"#,
            r#"{"type":"updateFreeSpin","amount":1,"total":10}"#,
            REVEAL,
            WIN,
            r#"{"type":"freeSpinRetrigger","totalFs":15,"positions":[{"reel":1,"row":2}]}"#,
            r#"{"type":"updateFreeSpin","amount":2,"total":15}"#,
            REVEAL,
            r#"{"type":"updateFreeSpin","amount":3,"total":15}"#,
            r#"{"type":"freeSpinRetrigger","totalFs":20}"#,
            r#"{"type":"updateFreeSpin","amount":4,"total":20}"#,
            r#"{"type":"freeSpinEnd","amount":500}"#,
        ]
        .join(",");
        assert_eq!(count(&events), 2);
    }

    #[test]
    fn bloodwater_book_counts_each_meal_that_raises_the_total() {
        // Repas du jeu de base (totalFs 0, hors bonus) ignoré ; puis 1 + 3
        // repas qui montent le total ; puis un repas au plafond (10 → 10) qui
        // ne compte pas. Les `updateFreeSpin` qui suivent ne recomptent rien.
        let events = [
            r#"{"type":"sharkFeed","hunts":[{"fish":"X","totalFs":0}],"sharks":[{"reel":2,"row":3,"multiplier":1}]}"#,
            r#"{"type":"freeSpinTrigger","totalFs":6,"positions":[{"reel":2,"row":3}]}"#,
            r#"{"type":"updateFreeSpin","amount":1,"total":6}"#,
            REVEAL,
            r#"{"type":"sharkFeed","hunts":[{"fish":"F","from":{"reel":1,"row":4},"totalFs":7}]}"#,
            r#"{"type":"updateFreeSpin","amount":2,"total":7}"#,
            r#"{"type":"sharkFeed","hunts":[{"fish":"F","totalFs":8},{"fish":"F","totalFs":9},{"fish":"M","totalFs":10}]}"#,
            r#"{"type":"updateFreeSpin","amount":3,"total":10}"#,
            r#"{"type":"sharkFeed","hunts":[{"fish":"F","totalFs":10}]}"#,
            r#"{"type":"updateFreeSpin","amount":4,"total":10}"#,
            r#"{"type":"freeSpinEnd","amount":154140}"#,
            r#"{"type":"finalWin","amount":154380}"#,
        ]
        .join(",");
        assert_eq!(count(&events), 4);
    }

    #[test]
    fn book_without_bonus_counts_nothing() {
        let events = [
            REVEAL,
            WIN,
            r#"{"type":"sharkFeed","hunts":[{"fish":"X","totalFs":0}]}"#,
            r#"{"type":"setTotalWin","amount":40}"#,
            r#"{"type":"finalWin","amount":40}"#,
        ]
        .join(",");
        assert_eq!(count(&events), 0);
    }

    #[test]
    fn retrigger_without_total_counts_once_even_when_the_update_follows() {
        // Seul l'événement le dit : il compte. Le total qu'annonce ensuite
        // `updateFreeSpin` est son écho, pas un second retrigger.
        let with_update = [
            r#"{"type":"freeSpinTrigger","totalFs":10}"#,
            r#"{"type":"updateFreeSpin","amount":1,"total":10}"#,
            r#"{"type":"freeSpinRetrigger"}"#,
            r#"{"type":"updateFreeSpin","amount":2,"total":15}"#,
        ]
        .join(",");
        assert_eq!(count(&with_update), 1);
        // Et sans aucun total nulle part : un par événement.
        let bare = [
            r#"{"type":"freeSpinTrigger"}"#,
            r#"{"type":"freeSpinRetrigger"}"#,
            r#"{"type":"freeSpinRetrigger"}"#,
        ]
        .join(",");
        assert_eq!(count(&bare), 2);
    }

    #[test]
    fn a_new_trigger_resets_the_base() {
        // Entre les deux bonus, une annonce hors bonus ne compte pas. Le
        // second bonus est plus court que le premier : son total de départ (8)
        // n'est ni une hausse ni une baisse, c'est une nouvelle base — et 9
        // est bien une hausse, même sous le 15 du premier bonus.
        let events = [
            r#"{"type":"freeSpinTrigger","totalFs":10}"#,
            r#"{"type":"freeSpinRetrigger","totalFs":15}"#,
            r#"{"type":"freeSpinEnd"}"#,
            r#"{"type":"sharkFeed","hunts":[{"totalFs":20}]}"#,
            r#"{"type":"freeSpinTrigger","totalFs":8}"#,
            r#"{"type":"updateFreeSpin","amount":1,"total":8}"#,
            r#"{"type":"sharkFeed","hunts":[{"totalFs":9}]}"#,
        ]
        .join(",");
        assert_eq!(count(&events), 2);
    }

    #[test]
    fn a_trigger_without_total_takes_its_base_from_the_first_update() {
        let events = [
            r#"{"type":"freeSpinTrigger"}"#,
            r#"{"type":"updateFreeSpin","amount":1,"total":10}"#,
            r#"{"type":"updateFreeSpin","amount":2,"total":12}"#,
        ]
        .join(",");
        assert_eq!(count(&events), 1);
    }

    #[test]
    fn odd_values_and_key_order_never_break_the_parse() {
        // `type` après les autres clés, `total` non numérique sur un événement
        // étranger aux free spins, `totalFs` à null, `total` hors
        // `updateFreeSpin` : rien de tout ça ne fait échouer ni ne compte.
        let events = [
            r#"{"totalFs":10,"type":"freeSpinTrigger"}"#,
            r#"{"type":"collect","total":{"coins":99},"items":[1,"a",null,[2],{"totalFs":"x"}]}"#,
            r#"{"type":"setTotalWin","total":500}"#,
            r#"{"type":"freeSpinRetrigger","totalFs":null,"positions":[]}"#,
            r#"{"total":14,"amount":2,"type":"updateFreeSpin"}"#,
        ]
        .join(",");
        // Le retrigger sans total compte ; le 14 qui suit est son écho.
        assert_eq!(count(&events), 1);
    }

    #[test]
    fn an_explicit_tier_beats_the_positions() {
        // SAUCER PANIC : 3 FULL INVASION = le Super (palier 4), 3 LAB EXPERIMENT = le Bonus (palier 3)
        let tier_of = |trigger: &str| {
            let line = book(&[trigger, r#"{"type":"freeSpinEnd","amount":10}"#].join(","));
            let parsed: FsBook = serde_json::from_str(&line).unwrap();
            summarize(&parsed.events).tier
        };
        let pos = r#""positions":[{"reel":0,"row":1},{"reel":2,"row":3},{"reel":4,"row":2}]"#;
        assert_eq!(tier_of(&format!(r#"{{"type":"freeSpinTrigger","totalFs":10,{pos},"tier":4}}"#)), 4);
        assert_eq!(tier_of(&format!(r#"{{"type":"freeSpinTrigger","totalFs":10,"tier":3,{pos}}}"#)), 3);
        // un palier illisible ne casse rien : les positions comptent
        assert_eq!(tier_of(&format!(r#"{{"type":"freeSpinTrigger","totalFs":10,{pos},"tier":"super"}}"#)), 3);
        assert_eq!(tier_of(&format!(r#"{{"type":"freeSpinTrigger","totalFs":10,{pos},"tier":null}}"#)), 3);
    }

    #[test]
    fn tier_is_the_first_trigger_positions() {
        // Palier = positions du PREMIER déclenchement : un second déclenchement
        // ne le change pas, et le criteria (ici un wincap) n'y entre pour rien.
        let line = book(
            &[
                r#"{"type":"freeSpinTrigger","totalFs":6,"positions":[{"reel":0,"row":1},{"reel":2,"row":3},{"reel":4,"row":0}]}"#,
                r#"{"type":"sharkFeed","hunts":[{"totalFs":7}]}"#,
                r#"{"type":"freeSpinTrigger","totalFs":10,"positions":[{"reel":1,"row":1},{"reel":2,"row":2}]}"#,
                r#"{"type":"wincap","amount":2000000}"#,
            ]
            .join(","),
        );
        let parsed: FsBook = serde_json::from_str(&line).unwrap();
        // Spins : ceux du PREMIER bonus (6 attribués, 7 joués) — le second
        // déclenchement à 10 n'y entre pas.
        assert_eq!(
            summarize(&parsed.events),
            BookFs { triggers: 2, tier: 3, retriggers: 1, spins_initial: 6, spins_final: 7, ..Default::default() }
        );
        // Sans déclenchement : palier 0 (le plateau du `reveal` est compté :
        // un H1 et un L1 en free spins). Déclenchement sans positions : 0 aussi.
        let none_line = book(REVEAL);
        let none: FsBook = serde_json::from_str(&none_line).unwrap();
        assert_eq!(
            summarize(&none.events),
            BookFs { symbols: SymbolTally { low_fs: 1, high_fs: 1, ..Default::default() }, ..Default::default() }
        );
        let bare_line = book(r#"{"type":"freeSpinTrigger","totalFs":8}"#);
        let bare: FsBook = serde_json::from_str(&bare_line).unwrap();
        assert_eq!(
            summarize(&bare.events),
            BookFs { triggers: 1, tier: 0, retriggers: 0, spins_initial: 8, spins_final: 8, ..Default::default() }
        );
    }

    #[test]
    fn spins_follow_the_first_bonus_even_without_a_trigger() {
        // Achat qui démarre en freegame : aucun `freeSpinTrigger`, la première
        // annonce fixe les spins attribués, le retrigger les spins joués.
        let line = book(
            &[
                r#"{"type":"updateFreeSpin","amount":1,"total":10}"#,
                r#"{"type":"freeSpinRetrigger","totalFs":15}"#,
                r#"{"type":"updateFreeSpin","amount":2,"total":15}"#,
            ]
            .join(","),
        );
        let parsed: FsBook = serde_json::from_str(&line).unwrap();
        let fs = summarize(&parsed.events);
        assert_eq!((fs.retriggers, fs.spins_initial, fs.spins_final), (1, 10, 15));
    }

    #[test]
    fn event_types_are_read_for_the_index() {
        let line = book(&[REVEAL, r#"{"type":"freeSpinTrigger","totalFs":6}"#].join(","));
        let parsed: FsBook = serde_json::from_str(&line).unwrap();
        let types: Vec<&str> = parsed.events.iter().map(|e| e.typ.as_ref()).collect();
        assert_eq!(types, ["reveal", "freeSpinTrigger"]);
        assert!(matches!(parsed.events[0].typ, Cow::Borrowed(_)));
        assert_eq!(parsed.events[1].fs.total_fs, Some(6.0));
    }

    // ── Format « actions » (UNDEAD KING) ──

    /// Plateau d'entrée : 4 BONUS, lows P2 P1 P1 P1 P5, highs P9 P6 P7.
    const SNAP_4: &str = r#"{"index":0,"win":0,"actions":[{"kind":"Haywire"},{"kind":"GridSnapshot","data":{"board":0,"progressiveBarValue":0,
        "cols":[["BONUS","P9","P2"],["P6","BONUS","P1"],["P1","P1","BONUS"],["BONUS","P7","P5"]]}}]}"#;
    /// Free spin : highs P9 P8, low P1 (le Wild n'est ni l'un ni l'autre).
    const FS_SPIN: &str = r#"{"index":2,"win":0,"actions":[{"kind":"GridSnapshot","data":{"board":0,"cols":[["P9","WILD3"],["P8","P1"]]}},
        {"kind":"WildJump","data":{"from_panel":[0,1],"to_panel":[1,1]}}]}"#;
    const FS_LINES: &str = r#"{"index":3,"win":1.5,"actions":[{"kind":"LinesHit","data":{"line":1,"streak":3,"panels":[[0,0],[0,1],[0,2]],"symbol":"P9"}}]}"#;

    fn front_map() -> SymbolMap {
        SymbolMap::from_front_config(
            br#"{"math":{"symbols":{"P1":"L5","P2":"L4","P3":"L3","P4":"L2","P5":"L1","P6":"H4","P7":"H3","P8":"H2","P9":"H1"}}}"#,
        )
        .unwrap()
    }

    #[test]
    fn actions_book_takes_its_tier_from_the_board_before_bonusgames() {
        let line = book(
            &[
                SNAP_4,
                r#"{"index":1,"win":0,"actions":[{"kind":"BonusGames","data":10}]}"#,
                FS_SPIN,
                FS_LINES,
                r#"{"index":4,"win":0,"actions":[{"kind":"FreeSpinsWin","data":{"win":15}}]}"#,
            ]
            .join(","),
        );
        let parsed: FsBook = serde_json::from_str(&line).unwrap();
        let fs = summarize_with(&parsed.events, &front_map());
        assert_eq!(
            fs,
            BookFs {
                triggers: 1,
                tier: 4,
                retriggers: 0,
                spins_initial: 10,
                spins_final: 10,
                symbols: SymbolTally { low: 5, high: 3, low_fs: 1, high_fs: 2 },
                // le format « actions » ne dit pas d'anticipation
                anticipation: false,
            }
        );
        // Sans la table du front, `P1`…`P9` ne se classent pas : rien compté.
        assert_eq!(summarize(&parsed.events).symbols, SymbolTally::default());
        // Les actions entrent dans l'index de recherche comme des types.
        let kinds: Vec<&str> = parsed.events[0].kinds().collect();
        assert_eq!(kinds, ["Haywire", "GridSnapshot"]);
        assert!(matches!(parsed.events[0].actions[1].kind, Cow::Borrowed(_)));
    }

    #[test]
    fn actions_book_without_bonusgames_is_no_bonus() {
        // Mystery « no bonus » : un plateau à 2 BONUS, rien d'autre.
        let snap_2 = SNAP_4.replacen("\"BONUS\"", "\"P3\"", 2);
        let line = book(&snap_2);
        let parsed: FsBook = serde_json::from_str(&line).unwrap();
        let fs = summarize_with(&parsed.events, &front_map());
        assert_eq!((fs.triggers, fs.tier, fs.spins_initial), (0, 0, 0));
        assert_eq!(fs.symbols, SymbolTally { low: 7, high: 3, ..Default::default() });
    }

    #[test]
    fn sdk_boards_count_by_game_type_and_odd_boards_never_break() {
        let events = [
            r#"{"type":"reveal","gameType":"basegame","board":[[{"name":"H2"},{"name":"H2","wild":false}],[{"multiplier":2,"name":"L3"}]]}"#,
            // Plateau d'un autre événement que `reveal` : pas compté.
            r#"{"type":"tumbleBoard","board":[[{"name":"H1"}]]}"#,
            // Formes inattendues : un nombre, un objet, un plateau à plat.
            r#"{"type":"reveal","board":0}"#,
            r#"{"type":"reveal","board":{"x":1}}"#,
            r#"{"type":"reveal","board":["H1","L1",{"name":"L2"},7,null]}"#,
            REVEAL,
        ]
        .join(",");
        let line = book(&events);
        let parsed: FsBook = serde_json::from_str(&line).unwrap();
        // Base : H2 H2 L3, puis le plateau à plat (sans `gameType`, hors bonus) H1 L1 L2.
        // Free spins : H1 et L1 de `REVEAL` (`gameType` freegame).
        assert_eq!(summarize(&parsed.events).symbols, SymbolTally { low: 3, high: 3, low_fs: 1, high_fs: 1 });
    }

    #[test]
    fn anticipation_is_the_paid_spin_one() {
        let antic = |events: &[&str]| {
            let line = book(&events.join(","));
            let parsed: FsBook = serde_json::from_str(&line).unwrap();
            summarize(&parsed.events).anticipation
        };
        // Le spin payé ralentit les rouleaux 4 et 5 ; le reveal suivant ne compte pas.
        assert!(antic(&[
            r#"{"type":"reveal","gameType":"basegame","anticipation":[0,0,0,1,1]}"#,
            r#"{"type":"reveal","gameType":"freegame","anticipation":[0,0,0,0,0]}"#,
        ]));
        // Une anticipation en free spins seulement n'est pas celle du spin payé.
        assert!(!antic(&[
            r#"{"type":"reveal","gameType":"basegame","anticipation":[0,0,0,0,0]}"#,
            r#"{"type":"reveal","gameType":"freegame","anticipation":[0,1,1,1,1]}"#,
        ]));
        // Formes inattendues : jamais d'échec d'import.
        assert!(!antic(&[r#"{"type":"reveal","anticipation":"yes"}"#]));
        assert!(!antic(&[r#"{"type":"reveal","anticipation":{"reel":3}}"#]));
        assert!(antic(&[r#"{"type":"reveal","anticipation":[[0],[2]]}"#]));
        assert!(!antic(&[r#"{"type":"reveal"}"#]));
    }
}
