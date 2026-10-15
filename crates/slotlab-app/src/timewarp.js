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

  window.addEventListener('message', (e) => {
    if (e.source !== window.parent || !e.data || e.data.slotlab !== 'speed') return;
    setSpeed(e.data.value);
  });

  window.__slotlabTimeWarp = { setSpeed, get speed() { return speed; } };

  // Ask the Play tab for the current rate, so the intro already runs at it.
  if (window.parent !== window) window.parent.postMessage({ slotlab: 'speed?' }, '*');
})();
