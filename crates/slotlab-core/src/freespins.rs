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
//! ── Pourquoi un désérialiseur écrit à la main ───────────────────────────────
//! L'import lit chaque book de fichiers qui dépassent plusieurs Go une fois
//! décompressés. Un `serde_json::Value` par événement coûterait des milliers
//! d'allocations par book ; ici, un champ est soit l'une des pistes, soit
//! sauté sans être construit — et un tableau n'est ouvert que pour y chercher
//! `totalFs`. Les nombres sont lus avec tolérance : `total` est lu sur TOUS
//! les événements (l'ordre des clés ne garantit pas que `type` vienne avant),
//! et sur un événement qui n'est pas `updateFreeSpin` il peut valoir autre
//! chose qu'un nombre — ce qui ne doit jamais faire échouer un import.

use serde::de::{self, Deserialize, Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};
use std::borrow::Cow;
use std::fmt;
use std::marker::PhantomData;

const TRIGGER: &str = "freeSpinTrigger";
const RETRIGGER: &str = "freeSpinRetrigger";
const UPDATE: &str = "updateFreeSpin";
const END: &str = "freeSpinEnd";

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
}

pub fn summarize(events: &[FsEvent<'_>]) -> BookFs {
    let mut out = BookFs::default();
    let mut counter = RetriggerCounter::default();
    for ev in events {
        counter.push(&ev.typ, &ev.fs);
        if ev.typ.eq_ignore_ascii_case(TRIGGER) {
            if out.triggers == 0 {
                out.tier = ev.positions.unwrap_or(0);
            }
            out.triggers += 1;
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
                        FsEventKey::Fs(k) => ev.fs.read(k, &mut map)?,
                    }
                }
                Ok(ev)
            }
        }
        d.deserialize_map(V(PhantomData))
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
            BookFs { triggers: 2, tier: 3, retriggers: 1, spins_initial: 6, spins_final: 7 }
        );
        // Sans déclenchement : palier 0. Déclenchement sans positions : 0 aussi.
        let none_line = book(REVEAL);
        let none: FsBook = serde_json::from_str(&none_line).unwrap();
        assert_eq!(summarize(&none.events), BookFs::default());
        let bare_line = book(r#"{"type":"freeSpinTrigger","totalFs":8}"#);
        let bare: FsBook = serde_json::from_str(&bare_line).unwrap();
        assert_eq!(
            summarize(&bare.events),
            BookFs { triggers: 1, tier: 0, retriggers: 0, spins_initial: 8, spins_final: 8 }
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
}
