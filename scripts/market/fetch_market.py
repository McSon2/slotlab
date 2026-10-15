#!/usr/bin/env python3
"""Market reference for SlotLab: every mode of the slots the market actually
plays, read from the certified lookup tables StakeCruncher serves.

    scripts/market/fetch_market.py [work dir] [coverage=0.90]

Who counts: the slots the market PLAYS NOW — the most played slots of the last
30 days (turnover, of the 400 most played games), from the top down, until
they make `coverage` of the slot turnover, whatever their studio (2026-10-03,
client: « les plus joués sur les 30 derniers jours, sinon ça n'a pas de
sens » ; the former rule — studios ≥ 1 % of the turnover, then their slots ≥
0.1 % — left out much-played slots of small studios). SlotLab re-runs the same
selection every day (`slotlab-app/src/market_refresh.rs`); the work dir is
SlotLab's own cache by default, so either one downloads only what is new.

Instant games (dice, keno, plinko… — a studio's hit can be one) are left
out: they are not slots, and their dozens of modes would swamp the rest.

Every active mode of those slots is read (base, ante, buys, all-or-nothing
modes) and written to crates/slotlab-core/data/market.json, embedded in
SlotLab at build time. The figures are in multiples of the mode's own STAKE
(its cost): hit rate, return quantiles, the win ladder, the value it carries,
the max win and its odds, volatility — see `figures` below, mirrored by
`slotlab_core::market`.

Polite: 0.5 s between lookup tables, 20 s back-off on HTTP 429. The work dir
keeps each mode's distribution (payout → weight), so a rerun downloads only
what is new, and a new figure needs no download at all.
"""
import gzip, io, json, math, os, subprocess, sys, time
from datetime import date
import numpy as np

N = 400
HOME = os.path.expanduser("~")
APP = os.path.join(HOME, "Library/Application Support/slotlab") if sys.platform == "darwin" else os.path.join(HOME, ".slotlab")
WD = sys.argv[1] if len(sys.argv) > 1 else os.path.join(APP, "market", "cache")
COVERAGE = float(sys.argv[2]) if len(sys.argv) > 2 else 0.90
ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
OUT = os.path.join(ROOT, "crates/slotlab-core/data/market.json")
RUNTIME = os.path.join(APP, "market", "market.json")
UA = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0 Safari/537.36"
API = "https://stakecruncher.com/tracker-api/engine"
REF = "https://stakecruncher.com/slots-tracker"
os.makedirs(os.path.join(WD, "dist"), exist_ok=True)

LADDER = [1, 2, 5, 10, 50, 100, 500, 1000, 5000, 10000]


def get(url, ref, out, headers=None):
    cmd = ["curl", "-sL", "--max-time", "180", "-A", UA, "-H", f"Referer: {ref}", "-o", out, "-w", "%{http_code}", url]
    if headers:
        cmd[1:1] = ["-D", headers]
    for _ in range(6):
        code = subprocess.run(cmd, capture_output=True, text=True).stdout.strip()
        if code != "429":
            return code
        time.sleep(20)
    return code


def kind_of(cost, zero):
    if abs(cost - 1) < 1e-9:
        return "base"
    if cost < 10:
        return "ante"
    return "bust" if zero >= 0.25 else "buy"


SPINS = 100
SESSION_STEP = 0.1
SESSION_CAP = 2.0 * SPINS
BUST_RETURN = 0.2


def short_session(dist, cost, spins=SPINS):
    """What `spins` rounds give back, in stakes — the exact law of the sum of
    independent rounds (FFT), as `slotlab_core::session` computes it: quartiles,
    share of sessions ending ahead, share giving back under 20 %."""
    bins = int(round(SESSION_CAP / SESSION_STEP)) + 1
    pmf = np.zeros(bins)
    W = sum(w for _, w in dist)
    for x, w in dist:
        k = min(int(round(max(x / cost, 0.0) / SESSION_STEP)), bins - 1)
        pmf[k] += w / W
    support = (bins - 1) * spins + 1
    size = 1 << (support - 1).bit_length()
    total = np.fft.irfft(np.fft.rfft(pmf, size) ** spins, size)[:support]
    total = np.maximum(total, 0.0)
    total /= total.sum()
    cum = np.cumsum(total)
    mass = cum[-1]
    q = lambda p: float(np.searchsorted(cum, p * mass - 1e-12)) * SESSION_STEP
    at_least = lambda stakes: float(mass - cum[int(round(stakes / SESSION_STEP)) - 1]) / mass if round(stakes / SESSION_STEP) > 0 else 1.0
    return {"s100Median": q(0.5), "s100Win": at_least(spins), "s100Bust": 1.0 - at_least(BUST_RETURN * spins)}


def figures(dist, cost):
    """dist: sorted [(payout in base bets, weight)]. Figures in stakes."""
    W = sum(w for _, w in dist)
    V = sum(w * x for x, w in dist)
    mean = V / W
    var = sum(w * (x - mean) ** 2 for x, w in dist) / W
    top = max(x for x, w in dist if w > 0)
    at = lambda t: sum(w for x, w in dist if x >= t * cost - 1e-9) / W
    under = lambda t: sum(w for x, w in dist if x < t * cost - 1e-9) / W
    vfrom = lambda t: (sum(w * x for x, w in dist if x >= t * cost - 1e-9) / V) if V > 0 else 0.0

    def q(p):
        cum = 0
        for x, w in dist:
            cum += w
            if cum >= p * W - 1e-9 * W:
                return x / cost
        return dist[-1][0] / cost

    hit = sum(w for x, w in dist if x > 0) / W
    f = {
        "rtp": mean / cost, "hit": hit, "zero": 1 - hit, "vol": math.sqrt(var) / cost,
        "max": top, "maxMult": top / cost, "pmax": at(top / cost),
        "q25": q(0.25), "median": q(0.5), "q75": q(0.75), "q90": q(0.9),
        "winsUnderStake": (sum(w for x, w in dist if 0 < x < cost - 1e-9) / W) / hit if hit else 0.0,
        "crumbs": under(0.25), "nearZero": under(0.1),
        "vs10": vfrom(10), "vs100": vfrom(100), "vs1000": vfrom(1000),
    }
    for t in LADDER:
        f[f"p{t}"] = at(t)
    # Value in the top 1 % of the weight, from the biggest win down (the
    # straddling row in part) — « the top 1 % carries … » of SlotLab.
    target, mass, top = 0.01 * W, 0.0, 0.0
    for x, w in reversed(dist):
        if mass >= target:
            break
        take = min(w, target - mass)
        top += take * x
        mass += take
    f["top1"] = top / V if V > 0 else 0.0
    if cost < 10:
        f.update(short_session(dist, cost))
    return f


ranked = []
for off in range(0, N, 100):
    f = os.path.join(WD, f"top_{off}.json")
    get(f"{API}/games?sort=turnover_30d&limit={min(100, N - off)}&offset={off}", REF, f)
    ranked += json.load(open(f))["items"]
catf = os.path.join(WD, "catalog.json")
get(f"{API}/verifier/catalog", REF + "/verifier", catf)
cat = {g["slug"]: g for g in json.load(open(catf))["games"]}

turn = lambda it: it.get("turnover_past_30d") or 0
total = sum(turn(it) for it in ranked) or 1
INSTANT = ("keno", "dice", "plinko", "mines", "limbo", "crash", "roulette", "blackjack", "baccarat", "hilo")


def catalog_slug(it):
    s, t = it["slug"], it["team_slug"]
    return s if s in cat else (s[len(t) + 1:] if s.startswith(t + "-") and s[len(t) + 1:] in cat else None)


def instant(slug):
    g = cat[slug]
    v = g.get("activeVersion") or max(m["version"] for m in g["modes"])
    return any(k in slug for k in INSTANT) or len([m for m in g["modes"] if m["version"] == v]) > 40


# The slots (instant games apart), most played first, until `COVERAGE` of their turnover.
slots, excluded = [], []
for r, it in enumerate(ranked, 1):
    slug = catalog_slug(it)
    if not slug:
        continue
    if instant(slug):
        excluded.append(f"{it['name'].strip()} ({it['team_name']})")
        continue
    slots.append((r, it))
slots.sort(key=lambda ri: -turn(ri[1]))
slot_total = sum(turn(it) for _, it in slots) or 1
chosen, acc = [], 0.0
for r, it in slots:
    if acc >= COVERAGE * slot_total:
        break
    chosen.append((r, it))
    acc += turn(it)

rows = []
for rank, it in chosen:
    slug = catalog_slug(it)
    if not slug:
        continue
    g = cat[slug]
    v = g.get("activeVersion") or max(m["version"] for m in g["modes"])
    for m in (m for m in g["modes"] if m["version"] == v and m.get("active", True)):
        cache = os.path.join(WD, "dist", f"{slug}__v{v}__{m['name']}.json.gz")
        if os.path.exists(cache):
            rec = json.load(gzip.open(cache))
        else:
            hf, bf = os.path.join(WD, "h.txt"), os.path.join(WD, "lut.bin")
            code = get(f"{API}/verifier/lookup-table?slug={slug}&version={v}&mode={m['name']}&_lv=5", REF + "/verifier", bf, hf)
            if code != "200":
                print(rank, slug, m["name"], "HTTP", code, flush=True)
                continue
            hdr = open(hf, errors="replace").read().lower()
            cost = next((float(l.split(":", 1)[1]) for l in hdr.splitlines() if l.startswith("x-lookup-cost-multiplier:")), 1.0)
            raw = open(bf, "rb").read()
            if "x-lookup-encoding: gzip" in hdr:
                raw = gzip.decompress(raw)
            agg = {}
            for line in io.StringIO(raw.decode(errors="replace")):
                p = line.strip().split(",")
                if len(p) >= 3 and p[1].isdigit() and p[2].isdigit():
                    agg[int(p[2])] = agg.get(int(p[2]), 0) + int(p[1])
            rec = {"cost": cost, "payouts": sorted(agg), "weights": [agg[k] for k in sorted(agg)]}
            json.dump(rec, gzip.open(cache, "wt"))
            time.sleep(0.5)
        dist = [(p / 100, w) for p, w in zip(rec["payouts"], rec["weights"]) if w > 0]
        if not dist or sum(w for _, w in dist) == 0:
            continue
        fig = figures(dist, rec["cost"])
        rows.append({
            "rank": rank, "slug": slug, "name": it["name"].strip(), "studio": it.get("team_name") or "",
            "share": turn(it) / total, "mode": m["name"], "cost": rec["cost"], "kind": kind_of(rec["cost"], fig["zero"]), **fig,
        })
        print(rank, slug, m["name"], f"cost {rec['cost']:g} {rows[-1]['kind']} max {fig['max']:.0f}", flush=True)

rnd = lambda v: float(f"{v:.6g}")
out = {
    "source": "stakecruncher.com — certified lookup tables of Stake Engine games",
    "rule": f"the most played slots of the last 30 days (turnover), up to {COVERAGE:.0%} of the slot turnover of the top {N}",
    "studios": len({it.get("team_name") for _, it in chosen}),
    "slots": len({r["slug"] for r in rows}),
    "coverage": sum(turn(it) for _, it in chosen) / total,
    "excluded": excluded,
    "fetched": date.today().isoformat(),
    "modes": [{k: (rnd(v) if isinstance(v, float) else v) for k, v in r.items()} for r in rows],
}
os.makedirs(os.path.dirname(RUNTIME), exist_ok=True)
for path in (OUT, RUNTIME):
    json.dump(out, open(path, "w"), indent=0, ensure_ascii=False)
print(f"{len(rows)} modes of {out['slots']} slots -> {OUT} + {RUNTIME}", flush=True)
