/** localStorage that never throws (private window, blocked site data…):
 * a failed read is "nothing stored", a failed write is silently dropped. */

export function storageGet(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

export function storageSet(key: string, value: string): void {
  try {
    localStorage.setItem(key, value);
  } catch {
    /* storage unavailable — the preference just won't persist */
  }
}

export function storageRemove(key: string): void {
  try {
    localStorage.removeItem(key);
  } catch {
    /* ignore */
  }
}

export function storageGetJson<T>(key: string, fallback: T): T {
  const raw = storageGet(key);
  if (raw == null) return fallback;
  try {
    return JSON.parse(raw) as T;
  } catch {
    return fallback;
  }
}

export function storageSetJson(key: string, value: unknown): void {
  storageSet(key, JSON.stringify(value));
}
