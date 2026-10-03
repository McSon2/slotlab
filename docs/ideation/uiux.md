# SlotLab — audit UI/UX (2026-09-13)

Audit live (Chrome, 1440 px et 1080 px = largeur mini de la fenêtre Tauri,
13 onglets, onboarding, Lighthouse a11y 90) + lecture statique des 32
composants Svelte. Chaque idée : observé → changement → fichiers.

## P0 — prioritaires

### 1. Changer d'onglet recharge tous les jeux — performance perçue
- **Observé :** en revenant sur Play, le splash de chargement réapparaît dans
  chaque viewport : Play est démonté par la chaîne `{#if}` d'`App.svelte`.
  Le timer `pollImport` fuit après démontage.
- **Changement :** Play reste monté (masqué par `hidden`) dès sa première
  ouverture ; prop `active` pour suspendre ses sondages en arrière-plan.
- **Fichiers :** `App.svelte`, `pages/Play.svelte`.

### 2. « 2 FAIL » rouge à côté de ★★★ — usability
- **Observé :** toutes les contraintes 3★ passent ; les 2 « fails » sont des
  plafonds 2★ (max payout 50000x > 25000, std 59 > 50) que `fail_count`
  comptait même quand le jeu atteint 3★.
- **Changement :** `fail_count` n'inclut les contraintes de palier que si
  aucun palier n'est atteint (et alors le nombre de corrections du palier le
  plus proche). Badge cliquable → vue Compliance ; ligne d'info « not eligible
  for 2★ : … ». Test de non-régression.
- **Fichiers :** `crates/slotlab-core/src/checks.rs`, `pages/Analyze.svelte`.

### 3. Contraste de `--text-faint` : 3,07–3,35:1 — accessibility
- **Observé :** couleur de tous les overlines, en-têtes de tableau et labels
  (10–10,5 px) sous le seuil AA 4,5:1.
- **Changement :** `oklch(0.5 …)` → `oklch(0.6 …)` = 4,71:1 sur `--bg-panel`.
- **Fichiers :** `app.css`.

### 4. Les échecs passent inaperçus — usability / accessibility
- **Observé :** « Force » sans Event ID ne fait rien ; aucun `aria-live` ;
  `app.loading` jamais affiché ; appels API sans `try/catch` (Play, Share,
  Deploy) ; un message de succès affiché dans une bannière rouge.
- **Changement :** store de notifications (`notify`, `run`) rendu dans une
  région `aria-live` d'`App.svelte` ; chaque action utilisateur passe par
  `run` avec état « en cours » sur son bouton.
- **Fichiers :** `lib/stores.svelte.ts`, `App.svelte`, toutes les pages.

### 5. Challenge et Deploy en français — cohérence
- **Changement :** tout en anglais (comme les 11 autres onglets), unités
  décimales anglaises (`fmtBytes` : KB/MB au lieu de « Mo »), nombres en-US.
- **Fichiers :** `pages/Challenge.svelte`, `pages/Deploy.svelte`, `pages/Assets.svelte`.

### 6. Tokens de Deploy en clair — sécurité côté interface
- **Changement :** `type="password"` + bouton afficher/masquer.
- **Fichiers :** `pages/Deploy.svelte`.

### 7. Actions destructrices sans confirmation — usability
- **Observé :** Stop (Share, coupe le lien pour tous), Publier (écrase le jeu
  en ligne), Reset sessions, suppression de bookmark, Clear des pins ; 4
  `confirm()` natifs.
- **Changement :** composant `ConfirmButton` (deux clics, retombe après 4 s,
  Échap annule) partout, `confirm()` supprimés.
- **Fichiers :** `components/ConfirmButton.svelte` + pages concernées.

### 8. Navigation : 13 onglets à plat, rechargement → Play — usability
- **Changement :** menu groupé Setup / Play / Math / QA / Ship ; Share + Deploy
  fusionnés dans **Publish** (Temporary link / Homelab) ; onglet dans l'URL
  (`#/analyze`, Back/Forward) ; `aria-current` ; icônes SVG homogènes à la
  place des glyphes Unicode ; lien d'évitement ; sans jeu → onglet Games.
- **Fichiers :** `App.svelte`, `lib/stores.svelte.ts`, `components/Icon.svelte`,
  `pages/Publish.svelte`.

## P1

| Sujet | Constat → changement |
|---|---|
| Cartes de mode (Analyze) | Débordent de 6 px à 1080 px → colonnes `minmax(0, …)`, `fmtX`, hauteur égale. |
| Preflight | Filtre « all » par défaut noie les warns → filtre « to review » par défaut, même vocabulaire pastilles/filtres, compteurs. |
| Review | IDs en texte brut, « ~1/inf » → tableau avec ▶ Play (force + bascule), cote lisible. |
| Share / Deploy | Formulaires doublons, soldes par défaut incohérents, prérequis en prose → Publish unifié, checklist live, Browse…. |
| Scan (vide) | Deux boutons « re-scan » avant tout scan → un seul « Run the first scan ». |
| Formats de nombres | RTP à 1/2/3/4 décimales, `50000x` vs `50,000x`, KB/Mo → `fmtPct`/`fmtX`/`fmtInt`/`fmtBytes` partout, `table.fixed` + `colgroup`. |
| Onboarding | Focus resté dans l'iframe, Échap inopérant → `<dialog>` natif `showModal()`, étapes annoncées. |
| Barre Play | Mode forcé vide, placeholder trompeur, Entrée inopérante, retour à la ligne à 1080 px, champs W×H sans label. |
| Balance | Plage « 0.0 % – 5000000.0 % » → bornée ; état vide explicatif. |
| CrowdSim | Aide citant une case invisible, labels désalignés, presets minuscules. |

## P2 — polish et design system
- Échelle typographique (`--fs-*`, 7 tailles au lieu de 26), espacements
  (`--sp-*`), `--r-pill`, teintes via `color-mix` au lieu de 67 `oklch` en dur,
  palette de sévérité `--sv-*` unique, séries de graphiques `--series-*`.
- Classes globales : `.pill.big/.tiny`, `.banner.warn/.info` (utilisée mais
  jamais définie), `.field-error`, `.sr-only`, `table.fixed`, liens stylés.
- Accessibilité : labels sur tous les champs, `aria-label` sur les boutons
  icônes, `tablist`/`aria-selected` sur les sous-onglets, `aria-expanded` sur
  les disclosures, grille du Board opérable au clavier, graphiques `role="img"`,
  statuts jamais portés par la seule couleur.
- `localStorage` protégé (`lib/storage.ts`), sondages arrêtés au démontage.

## Statut — implémenté le 2026-09-13

Tout ce qui précède est en place, vérifié par `svelte-check` (0 erreur),
`vite build`, `cargo test` (core + app) et un contrôle live à 1080 px sur un
build `embed-ui`. En plus de la liste :

- Backend : pick « average win » des Event IDs limité aux books tirables
  (poids LUT > 0, plus de « ~1/inf ») ; règle Preflight §27.6 passée en
  *warn* (elle affichait « pass — 10 declared and unused ») ; messages de
  `deploy.rs` en anglais.
- UI embarquée servie avec `Cache-Control` (`no-cache` sur index.html,
  `immutable` sur `assets/`) : sans ça, une mise à jour gardait l'ancienne UI.
- `.sr-only` épinglé à gauche : un texte masqué dans un tableau scrollé
  élargissait la page.
- Board opérable au clavier, onboarding en `<dialog>` natif, logique d'ajout
  de jeu dédupliquée (`lib/addGame.svelte.ts`).

Reste à surveiller : les onglets de CohortView gardent un style « pills »
(deux barres soulignées superposées seraient confuses) ; les jeux laissés
montés en arrière-plan continuent de jouer leur son s'il y en a.
