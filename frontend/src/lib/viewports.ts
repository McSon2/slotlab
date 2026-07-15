export interface ViewportPreset {
  id: string;
  label: string;
  width: number;
  height: number;
  device: 'desktop' | 'mobile';
}

/** The 7 stake-dev-tool resolutions + custom presets added by the UI. */
export const VIEWPORT_PRESETS: ViewportPreset[] = [
  { id: 'desktop', label: 'Desktop', width: 1200, height: 675, device: 'desktop' },
  { id: 'laptop', label: 'Laptop', width: 1024, height: 576, device: 'desktop' },
  { id: 'popout-l', label: 'Popout L', width: 800, height: 450, device: 'desktop' },
  { id: 'popout-s', label: 'Popout S', width: 400, height: 225, device: 'desktop' },
  { id: 'mobile-l', label: 'Mobile L', width: 425, height: 821, device: 'mobile' },
  { id: 'mobile-m', label: 'Mobile M', width: 375, height: 667, device: 'mobile' },
  { id: 'mobile-s', label: 'Mobile S', width: 320, height: 568, device: 'mobile' },
];

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
