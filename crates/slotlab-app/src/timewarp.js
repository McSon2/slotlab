// SlotLab time warp — injected at the top of the <head> of the builds served
// under /games/<name>/, before any of the game's own code. It replaces the
// page's clocks with a virtual one whose rate the Play tab sets on the fly
// (postMessage), so a spin can be watched at 1/10th speed, or frozen.
//
// What runs on the virtual clock: performance.now, Date.now, the timestamp
// handed to requestAnimationFrame (Pixi's Ticker, Spine, Svelte's motion all
// derive their delta from these), setTimeout/setInterval (pending timers are
// re-armed when the rate changes) and Web Animations / CSS animations
// (playbackRate). Audio keeps its real pace: slowing it would only bend the
// pitch. At rate 1 the page behaves exactly as without the script.
//
// It also carries the Play tab's mute: every Web Audio graph is routed
// through a master gain the script owns (Howler, Pixi Sound and raw contexts
// all end on ctx.destination), and every <audio>/<video> is muted — the
// game's own volume settings are left alone.
(() => {
  if (window.__slotlabTimeWarp) return;

  const realNow = performance.now.bind(performance);
  const realSetTimeout = window.setTimeout.bind(window);
  const realClearTimeout = window.clearTimeout.bind(window);
  const realRaf = window.requestAnimationFrame.bind(window);
  const dateOffset = Date.now() - realNow();

  let speed = 1;
  let baseReal = realNow();
  let baseVirt = baseReal;
  const vnow = () => baseVirt + (realNow() - baseReal) * speed;

  performance.now = vnow;
  Date.now = () => Math.floor(dateOffset + vnow());
  window.requestAnimationFrame = (cb) => realRaf(() => cb(vnow()));

  // ── Timers: due dates live on the virtual clock ──────────────────────────
  const timers = new Map();
  let nextId = 1;

  function arm(t) {
    if (t.real !== undefined) realClearTimeout(t.real);
    t.real = undefined;
    if (speed <= 0) return; // frozen: re-armed when the rate comes back
    t.real = realSetTimeout(() => fire(t), Math.max(0, (t.due - vnow()) / speed));
  }

  function fire(t) {
    if (timers.get(t.id) !== t) return;
    if (t.every === undefined) {
      timers.delete(t.id);
    } else {
      t.due = Math.max(t.due + t.every, vnow());
      arm(t);
    }
    if (typeof t.fn === 'function') t.fn(...t.args);
    else (0, eval)(String(t.fn));
  }

  function schedule(fn, delay, args, repeat) {
    const d = Math.max(0, Number(delay) || 0);
    const t = { id: nextId++, fn, args, due: vnow() + d, every: repeat ? Math.max(d, 4) : undefined };
    timers.set(t.id, t);
    arm(t);
    return t.id;
  }

  function clear(id) {
    const t = timers.get(id);
    if (!t) return;
    timers.delete(id);
    if (t.real !== undefined) realClearTimeout(t.real);
  }

  window.setTimeout = (fn, delay, ...args) => schedule(fn, delay, args, false);
  window.setInterval = (fn, delay, ...args) => schedule(fn, delay, args, true);
  window.clearTimeout = clear;
  window.clearInterval = clear;

  // ── Web Animations (Svelte transitions) and CSS animations ───────────────
  const realAnimate = Element.prototype.animate;
  Element.prototype.animate = function (...a) {
    const anim = realAnimate.apply(this, a);
    if (speed !== 1) anim.playbackRate = speed;
    return anim;
  };

  function sweep() {
    if (!document.getAnimations) return;
    for (const a of document.getAnimations()) {
      if (a.playbackRate !== speed) a.playbackRate = speed;
    }
  }

  // CSS animations start whenever a class lands: catch them while slowed.
  let sweeping = false;
  function sweepLoop() {
    if (speed === 1) {
      sweeping = false;
      return;
    }
    sweep();
    realSetTimeout(sweepLoop, 250);
  }

  function setSpeed(value) {
    const s = Math.min(4, Math.max(0, Number(value)));
    if (!Number.isFinite(s) || s === speed) return;
    const v = vnow();
    baseReal = realNow();
    baseVirt = v;
    speed = s;
    for (const t of timers.values()) arm(t);
    sweep();
    if (!sweeping && speed !== 1) {
      sweeping = true;
      sweepLoop();
    }
  }

  // ── Mute ──────────────────────────────────────────────────────────────────
  let muted = false;
  const masters = new Map(); // AudioContext → its master GainNode

  // Whatever connects to a context's destination lands on the master gain
  // instead, which alone reaches the speakers.
  const realConnect = AudioNode.prototype.connect;
  const realDisconnect = AudioNode.prototype.disconnect;
  function master(ctx) {
    let g = masters.get(ctx);
    if (!g) {
      g = ctx.createGain();
      g.gain.value = muted ? 0 : 1;
      realConnect.call(g, ctx.destination);
      masters.set(ctx, g);
    }
    return g;
  }
  const reroute = (dest) =>
    typeof AudioDestinationNode !== 'undefined' && dest instanceof AudioDestinationNode
      ? master(dest.context)
      : dest;
  AudioNode.prototype.connect = function (dest, ...rest) {
    return realConnect.call(this, reroute(dest), ...rest);
  };
  AudioNode.prototype.disconnect = function (dest, ...rest) {
    return dest === undefined || typeof dest === 'number'
      ? realDisconnect.call(this, dest, ...rest)
      : realDisconnect.call(this, reroute(dest), ...rest);
  };

  // Media elements: only the ones the script muted get unmuted back.
  let silenced = new Set();
  const silence = (el) => {
    if (el.muted) return;
    el.muted = true;
    silenced.add(el);
  };
  const realPlay = HTMLMediaElement.prototype.play;
  HTMLMediaElement.prototype.play = function (...a) {
    if (muted) silence(this);
    return realPlay.apply(this, a);
  };

  function setMuted(value) {
    const m = Boolean(value);
    if (m === muted) return;
    muted = m;
    for (const [ctx, g] of masters) g.gain.setValueAtTime(m ? 0 : 1, ctx.currentTime);
    if (m) {
      for (const el of document.querySelectorAll('audio, video')) silence(el);
    } else {
      for (const el of silenced) el.muted = false;
      silenced = new Set();
    }
  }

  window.addEventListener('message', (e) => {
    if (e.source !== window.parent || !e.data) return;
    if (e.data.slotlab === 'speed') setSpeed(e.data.value);
    else if (e.data.slotlab === 'mute') setMuted(e.data.value);
  });

  window.__slotlabTimeWarp = {
    setSpeed, setMuted, get speed() { return speed; }, get muted() { return muted; },
  };

  // Ask the Play tab for the current rate (and mute), so the intro already
  // runs at it.
  if (window.parent !== window) window.parent.postMessage({ slotlab: 'speed?' }, '*');
})();
