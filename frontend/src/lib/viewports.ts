export interface ViewportPreset {
  id: string;
  label: string;
  width: number;
  height: number;
  device: 'desktop' | 'mobile';
}

/**
 * The 7 stake-dev-tool resolutions, plus the two aspect ratios the review has
 * actually refused a game over: a wide desktop (a background painted in COVER
 * pushes the playfield out of frame at ratios ≥ 2.0) and a phone held
 * sideways, which is the same geometry on a small screen.
 *
 * ⚠️ And the two SHORT PORTRAITS nobody was testing: a phone that opens the game
 * on stake.bet WITHOUT going fullscreen. The site keeps its own header and tab
 * bar, so the game gets a frame of ratio ~0.68 instead of the ~0.46 of a phone
 * screen — measured on the real page: 1240x1834 device pixels on an iPhone 16
 * Pro Max (413x611 CSS), and one notch shorter on a 393 pt phone. It is still
 * `portrait` for the layout engine (ratio ≤ 0.8), but the HUD stops growing with
 * the width there (`--ui-unit` is `min(1vw, 0.7vh)`), which frees vertical band
 * and lets the playfield grow and climb — into whatever the game paints at a
 * FIXED height, its logo first. Skull Perch shipped with its logo sitting on the
 * top two rows of symbols in exactly this frame (2026-09-09).
 */
export const VIEWPORT_PRESETS: ViewportPreset[] = [
  { id: 'desktop', label: 'Desktop', width: 1200, height: 675, device: 'desktop' },
  { id: 'laptop', label: 'Laptop', width: 1024, height: 576, device: 'desktop' },
  { id: 'popout-l', label: 'Popout L', width: 800, height: 450, device: 'desktop' },
  { id: 'popout-s', label: 'Popout S', width: 400, height: 225, device: 'desktop' },
  { id: 'mobile-l', label: 'Mobile L', width: 425, height: 821, device: 'mobile' },
  { id: 'mobile-m', label: 'Mobile M', width: 375, height: 667, device: 'mobile' },
  { id: 'mobile-s', label: 'Mobile S', width: 320, height: 568, device: 'mobile' },
  { id: 'ultrawide', label: 'Ultrawide 2.4', width: 1720, height: 720, device: 'desktop' },
  // Spelled out: a ⟳ glyph read as "reload" next to the viewport checkboxes.
  { id: 'phone-landscape', label: 'Phone landscape', width: 812, height: 375, device: 'mobile' },
  { id: 'stake-mobile', label: 'Stake mobile', width: 413, height: 611, device: 'mobile' },
  { id: 'stake-mobile-s', label: 'Stake mobile S', width: 366, height: 507, device: 'mobile' },
];

/**
 * The viewports the review systematically opens. Popout S is the one that has
 * failed most often (a CTA 25 px under the fold, a replay panel without
 * overflow), Mobile S catches the scrollbars, Stake mobile catches everything a
 * short portrait breaks (see above), and the two wide ones catch a playfield
 * that leaves the frame.
 */
export const REVIEW_VIEWPORTS = ['popout-s', 'mobile-s', 'stake-mobile', 'ultrawide', 'desktop'];

export const CURRENCIES = [
  'USD', 'EUR', 'GBP', 'CAD', 'AUD', 'NZD', 'JPY', 'CNY', 'KRW', 'INR',
  'BRL', 'MXN', 'ARS', 'CLP', 'PEN', 'PLN', 'CZK', 'HUF', 'SEK', 'NOK',
  'DKK', 'CHF', 'TRY', 'RUB', 'UAH', 'VND', 'THB', 'IDR', 'PHP', 'MYR',
  'ZAR', 'NGN', 'KES', 'GHS',
];

/** Sweepstakes currencies, served only in social mode (Stake.US: XGC/XSC — Stake EU: XEC, shown as "SC"). */
export const SOCIAL_CURRENCIES = ['XGC', 'XSC', 'XEC'];

export const LANGUAGES = [
  'en', 'fr', 'de', 'es', 'pt', 'br', 'it', 'nl', 'pl', 'ru', 'tr', 'ja',
  'ko', 'zh', 'vi', 'th',
];
