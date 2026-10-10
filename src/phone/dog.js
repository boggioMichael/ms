// MapleSyrup's dog: the cream chow chow in the syrup captain's hat and cape
// from the MapleSyrup logo, cut into parts (the tail, the body, the front
// paws, the head, the eyes, the mouth: dog-parts.png) and brought to life in
// its own box at the top of the phone page. It hops about, runs, jumps,
// spins, rolls, sniffs, scratches, plays with its ball and naps in its bed
// as it feels like it; it looks at the player and listens when they talk,
// tilts its head while it thinks, barks at danger, jumps for joy at a level
// up, plays dead when the character dies, comes when the box is tapped, and
// talks: its mouth opens as far as the voice is loud.
//
// The page gives it: `talking` (and how loud the voice is now, 0..1, when
// the phone can hear it), `listening`, `thinking`, and events (`react`).
(function (root) {
  "use strict";

  const TAU = Math.PI * 2;
  const clamp = (v, a, b) => Math.max(a, Math.min(b, v));
  const lerp = (a, b, t) => a + (b - a) * t;
  const smooth = (t) => { t = clamp(t, 0, 1); return t * t * (3 - 2 * t); };
  // The same pseudo-random numbers every time for a seed.
  function rng(seed) {
    let s = (seed * 2654435761) >>> 0 || 1;
    return () => { s ^= s << 13; s >>>= 0; s ^= s >>> 17; s ^= s << 5; s >>>= 0; return s / 4294967296; };
  }
  const rand = (a, b) => a + Math.random() * (b - a);
  const ease = (v, to, rate, dt) => v + (to - v) * (1 - Math.exp(-rate * dt));

  // ---- the dog's parts ---------------------------------------------------------------------
  // Where each part is on the sheet, and where it sits on the dog (in the
  // logo's pixels: the dog is 304 by 495, sitting on the ground at 478);
  // where the parts turn.
  const ART = {"size":[304,495],"scale":0.7,"ground":478,"pivots":{"tail":[272,402],"head":[150,300],"body":[150,476],"pawL":[91,432],"pawR":[186,432]},"parts":{"tail":{"sheet":[2,2,37,55],"at":[245,326,53,78]},"body":{"sheet":[41,2,195,171],"at":[9,231,279,244]},"pawL":{"sheet":[238,2,41,31],"at":[62,431,58,44]},"pawR":{"sheet":[281,2,39,34],"at":[158,431,56,48]},"head":{"sheet":[2,175,196,220],"at":[4,9,280,315]},"eyeL":{"sheet":[200,175,24,18],"at":[86,191,34,25]},"eyeR":{"sheet":[226,175,22,18],"at":[172,190,32,25]},"mouth":{"sheet":[250,175,78,36],"at":[92,257,111,52]},"smile":{"sheet":[330,175,78,15],"at":[91,261,112,21]}}};
  const MID = 150, GROUND = ART.ground, TALL = GROUND - 9;
  const INK = "#3a2016";

  // Part `name` where it sits, squashed up and down to `sy` round a line
  // `anchor` of the way down it (0 its top, 0.5 its middle).
  function part(ctx, img, name, sy, anchor) {
    const p = ART.parts[name], s = p.sheet, a = p.at;
    if (sy == null || sy === 1) { ctx.drawImage(img, s[0], s[1], s[2], s[3], a[0], a[1], a[2], a[3]); return; }
    const ay = a[1] + a[3] * (anchor || 0);
    ctx.save();
    ctx.translate(0, ay); ctx.scale(1, sy); ctx.translate(0, -ay);
    ctx.drawImage(img, s[0], s[1], s[2], s[3], a[0], a[1], a[2], a[3]);
    ctx.restore();
  }

  // Turned by `rot` round point `at`.
  function turned(ctx, at, rot, draw) {
    if (!rot) { draw(); return; }
    ctx.save();
    ctx.translate(at[0], at[1]); ctx.rotate(rot); ctx.translate(-at[0], -at[1]);
    draw();
    ctx.restore();
  }

  function eyes(ctx, img, h) {
    for (const name of ["eyeL", "eyeR"]) {
      const a = ART.parts[name].at, cx = a[0] + a[2] / 2, cy = a[1] + a[3] / 2;
      ctx.strokeStyle = INK; ctx.lineCap = "round"; ctx.lineWidth = 3.4;
      if (h.dead) { // x x
        ctx.beginPath();
        ctx.moveTo(cx - 7, cy - 7); ctx.lineTo(cx + 7, cy + 7); ctx.moveTo(cx + 7, cy - 7); ctx.lineTo(cx - 7, cy + 7);
        ctx.stroke();
      } else if (h.happy) { // ^ ^
        ctx.beginPath();
        ctx.moveTo(cx - 10, cy + 4); ctx.quadraticCurveTo(cx, cy - 9, cx + 10, cy + 4);
        ctx.stroke();
      } else {
        ctx.save();
        ctx.translate(h.lookX * 2.2, h.lookY * 1.8);
        part(ctx, img, name, Math.max(0.1, 1 - h.blink * 0.92), 0.5);
        ctx.restore();
      }
    }
  }

  // The mouth: a closed smile, or open as far as `open` (past 1: a yawn).
  function mouth(ctx, img, open) {
    if (open < 0.07) { part(ctx, img, "smile"); return; }
    part(ctx, img, "mouth", clamp(0.2 + 0.8 * open, 0.2, 1.25), 0);
  }

  // ---- the whole dog -------------------------------------------------------------------------
  // `p`: how it is now (see `Pose`), drawn standing at p.x, p.y (the ground
  // under it), p.scale of the logo's size. Returns where the top of its head is.
  function drawDog(ctx, img, p) {
    ctx.save();
    ctx.translate(p.x, p.y);
    ctx.scale(p.scale, p.scale);
    // Its shadow: smaller and fainter the higher it is.
    const high = clamp(p.lift / 180, 0, 1);
    const sh = ctx.createRadialGradient(0, 0, 4, 0, 0, 150);
    sh.addColorStop(0, "rgba(0, 0, 0, " + (0.42 * (1 - high * 0.6)) + ")");
    sh.addColorStop(1, "rgba(0, 0, 0, 0)");
    ctx.save();
    ctx.scale((1 - high * 0.35) * (p.roll ? 1.2 : 1), 0.12 * (1 - high * 0.3));
    ctx.fillStyle = sh;
    ctx.beginPath(); ctx.arc(0, 0, 150, 0, TAU); ctx.fill();
    ctx.restore();
    ctx.translate(0, -p.lift);
    // Leaning, from the ground; squashed and stretched; spinning; rolling
    // round its middle.
    ctx.rotate(p.lean);
    ctx.scale(p.spin / Math.sqrt(p.squash), p.squash);
    if (p.roll) { ctx.translate(0, -200); ctx.rotate(p.roll); ctx.translate(0, 200); }
    ctx.translate(-MID, -GROUND);
    turned(ctx, ART.pivots.tail, p.tail, () => part(ctx, img, "tail"));
    // Breathing.
    ctx.save();
    ctx.translate(MID, GROUND); ctx.scale(1, p.breath); ctx.translate(-MID, -GROUND);
    part(ctx, img, "body");
    ctx.restore();
    ctx.save(); ctx.translate(0, -p.pawL); part(ctx, img, "pawL"); ctx.restore();
    ctx.save(); ctx.translate(0, -p.pawR); part(ctx, img, "pawR"); ctx.restore();
    // The head, on its neck, with its face.
    ctx.save();
    ctx.translate(p.headX, p.headY - (p.breath - 1) * 180);
    turned(ctx, ART.pivots.head, p.headRot, () => {
      part(ctx, img, "head");
      eyes(ctx, img, p.face);
      mouth(ctx, img, p.face.mouth);
    });
    ctx.restore();
    ctx.restore();
    // The top of its head, on the canvas (for what floats over it).
    const c = Math.cos(p.lean), s = Math.sin(p.lean), hy = -(GROUND - 60) * p.squash;
    return [p.x + (-hy * s + p.headX) * p.scale, p.y + (hy * c - p.lift + p.headY) * p.scale];
  }

  function Pose() {
    return {
      x: 100, y: 140, scale: 0.25, lift: 0, lean: 0, squash: 1, spin: 1, roll: 0, breath: 1,
      headRot: 0, headX: 0, headY: 0, pawL: 0, pawR: 0, tail: 0,
      face: { blink: 0, mouth: 0, happy: false, dead: false, lookX: 0, lookY: 0 },
    };
  }

  // ---- its room ------------------------------------------------------------------------------
  // The wall and the floor, drawn once for a size.
  function roomPicture(w, h, ground, dpr) {
    const c = document.createElement("canvas");
    c.width = Math.round(w * dpr); c.height = Math.round(h * dpr);
    const x = c.getContext("2d");
    x.scale(dpr, dpr);
    const wall = x.createLinearGradient(0, 0, 0, ground);
    wall.addColorStop(0, "#2c2117"); wall.addColorStop(1, "#211910");
    x.fillStyle = wall;
    x.fillRect(0, 0, w, ground);
    // Little maple leaves on the wallpaper, faint.
    const r = rng(99);
    x.fillStyle = "rgba(242, 163, 58, .06)";
    for (let i = 0; i < Math.round(w / 34); i++) {
      for (let j = 0; j < 3; j++) {
        const lx = (i + (j % 2) * 0.5) * 34 + 10 + r() * 6, ly = 16 + j * 34 + r() * 6;
        if (ly > ground - 14) continue;
        leaf(x, lx, ly, 4.6, r() * 0.8 - 0.4);
      }
    }
    // A soft light from above.
    const glow = x.createRadialGradient(w * 0.5, -20, 10, w * 0.5, -20, Math.max(w, h) * 0.9);
    glow.addColorStop(0, "rgba(255, 210, 150, .10)"); glow.addColorStop(1, "rgba(255, 210, 150, 0)");
    x.fillStyle = glow;
    x.fillRect(0, 0, w, ground);
    // The skirting board, then the wooden floor.
    x.fillStyle = "#3a2b1c";
    x.fillRect(0, ground - 4, w, 4);
    x.fillStyle = "rgba(255, 220, 170, .08)";
    x.fillRect(0, ground - 4, w, 1);
    const floor = x.createLinearGradient(0, ground, 0, h);
    floor.addColorStop(0, "#4a3421"); floor.addColorStop(1, "#33241a");
    x.fillStyle = floor;
    x.fillRect(0, ground, w, h - ground);
    x.strokeStyle = "rgba(0, 0, 0, .25)"; x.lineWidth = 1;
    const rows = [ground + 7, ground + 15];
    x.beginPath();
    for (const y of rows) { if (y < h) { x.moveTo(0, y + 0.5); x.lineTo(w, y + 0.5); } }
    for (let i = 0; i < 3; i++) {
      const y0 = i === 0 ? ground : rows[i - 1], y1 = i < rows.length ? rows[i] : h;
      for (let px = (i * 53) % 90 + 20; px < w; px += 90 + ((i * 17) % 30)) { x.moveTo(px + 0.5, y0); x.lineTo(px + 0.5, y1); }
    }
    x.stroke();
    x.fillStyle = "rgba(255, 230, 190, .05)";
    for (const y of [ground, ...rows]) x.fillRect(0, y + 1, w, 1);
    return c;
  }

  function leaf(x, cx, cy, s, rot) {
    x.save(); x.translate(cx, cy); x.rotate(rot); x.scale(s / 10, s / 10);
    x.beginPath();
    x.moveTo(0, 10);
    x.lineTo(0, 4); x.lineTo(-7, 6); x.lineTo(-5, 1); x.lineTo(-10, -2); x.lineTo(-6, -4); x.lineTo(-7, -9);
    x.lineTo(-2, -6); x.lineTo(0, -12); x.lineTo(2, -6); x.lineTo(7, -9); x.lineTo(6, -4); x.lineTo(10, -2);
    x.lineTo(5, 1); x.lineTo(7, 6); x.lineTo(0, 4);
    x.closePath(); x.fill();
    x.restore();
  }

  // Its bed: a round cushion with a soft rim, syrup coloured. The back of
  // it goes behind the dog, the front rim in front (`front`).
  function bed(ctx, x, y, s, front) {
    ctx.save();
    ctx.translate(x, y); ctx.scale(s, s);
    if (!front) {
      const sh = ctx.createRadialGradient(0, 0, 4, 0, 0, 52);
      sh.addColorStop(0, "rgba(0, 0, 0, .45)"); sh.addColorStop(1, "rgba(0, 0, 0, 0)");
      ctx.fillStyle = sh;
      ctx.save(); ctx.scale(1, 0.16); ctx.beginPath(); ctx.arc(0, 4, 52, 0, TAU); ctx.fill(); ctx.restore();
      // The rim at the back, and the cushion inside.
      const rim = ctx.createLinearGradient(0, -20, 0, 0);
      rim.addColorStop(0, "#f6b25a"); rim.addColorStop(1, "#b9681f");
      ctx.fillStyle = rim;
      ctx.beginPath(); ctx.ellipse(0, -12, 46, 12, 0, 0, TAU); ctx.fill();
      ctx.fillRect(-46, -12, 92, 9);
      const inside = ctx.createRadialGradient(-8, -12, 2, 0, -9, 40);
      inside.addColorStop(0, "#fff1d6"); inside.addColorStop(1, "#dcbf91");
      ctx.fillStyle = inside;
      ctx.beginPath(); ctx.ellipse(0, -9, 37, 7, 0, 0, TAU); ctx.fill();
      ctx.strokeStyle = "rgba(170, 120, 60, .35)"; ctx.lineWidth = 0.8;
      ctx.beginPath(); ctx.ellipse(0, -9, 31, 5, 0, Math.PI * 1.1, Math.PI * 1.9); ctx.stroke();
    } else {
      // The front of the rim: a soft roll.
      const roll = ctx.createLinearGradient(0, -8, 0, 4);
      roll.addColorStop(0, "#ffc773"); roll.addColorStop(0.5, "#e8902f"); roll.addColorStop(1, "#a85d1b");
      ctx.strokeStyle = roll; ctx.lineWidth = 11; ctx.lineCap = "round";
      ctx.beginPath(); ctx.ellipse(0, -10, 40.5, 6.5, 0, 0.06 * Math.PI, 0.94 * Math.PI); ctx.stroke();
      ctx.strokeStyle = "rgba(255, 240, 210, .45)"; ctx.lineWidth = 1.2;
      ctx.beginPath(); ctx.ellipse(0, -13.5, 40, 6, 0, 0.18 * Math.PI, 0.82 * Math.PI); ctx.stroke();
    }
    ctx.restore();
  }

  // Its ball: a tennis ball, rolling.
  function drawBall(ctx, x, y, r, a) {
    ctx.fillStyle = "rgba(0, 0, 0, .32)";
    ctx.beginPath(); ctx.ellipse(x, y + 0.5, r * 1.1, r * 0.3, 0, 0, TAU); ctx.fill();
    ctx.save();
    ctx.translate(x, y - r);
    const g = ctx.createRadialGradient(-r * 0.35, -r * 0.4, 0.5, 0, 0, r * 1.05);
    g.addColorStop(0, "#f4fb9e"); g.addColorStop(0.55, "#cbdc3d"); g.addColorStop(1, "#8a9d1c");
    ctx.fillStyle = g;
    ctx.beginPath(); ctx.arc(0, 0, r, 0, TAU); ctx.fill();
    ctx.clip();
    ctx.rotate(a);
    ctx.strokeStyle = "rgba(255, 255, 250, .92)"; ctx.lineWidth = Math.max(0.8, r * 0.16);
    ctx.beginPath(); ctx.arc(-r * 1.3, 0, r * 0.98, -0.95, 0.95); ctx.stroke();
    ctx.beginPath(); ctx.arc(r * 1.3, 0, r * 0.98, Math.PI - 0.95, Math.PI + 0.95); ctx.stroke();
    ctx.restore();
  }

  // A picture on the wall: a maple leaf in a wooden frame.
  function picture(x, cx, cy) {
    x.save();
    x.translate(cx, cy);
    x.fillStyle = "rgba(0, 0, 0, .35)"; x.fillRect(-15, -11, 32, 26);
    const wood = x.createLinearGradient(-16, -12, 16, 12);
    wood.addColorStop(0, "#8a5a2b"); wood.addColorStop(1, "#5b3818");
    x.fillStyle = wood; x.fillRect(-17, -13, 34, 26);
    x.fillStyle = "#f3e6cc"; x.fillRect(-13, -9, 26, 18);
    x.fillStyle = "#e8892c";
    leaf(x, 0, 0.5, 7.5, -0.15);
    x.restore();
  }

  // A heart, for being petted.
  function heart(ctx, x, y, s) {
    ctx.save(); ctx.translate(x, y); ctx.scale(s, s);
    ctx.beginPath();
    ctx.moveTo(0, 3);
    ctx.bezierCurveTo(-6, -1, -4, -7, 0, -3.6);
    ctx.bezierCurveTo(4, -7, 6, -1, 0, 3);
    ctx.fillStyle = "#ff6f7d"; ctx.fill();
    ctx.fillStyle = "rgba(255, 255, 255, .6)";
    ctx.beginPath(); ctx.ellipse(-2.2, -2.6, 1.1, 0.7, -0.6, 0, TAU); ctx.fill();
    ctx.restore();
  }

  // ---- its life ------------------------------------------------------------------------------
  // How it gets about, facing you: hopping from paw to paw, faster, or
  // running in bounds. `step`: how far a hop goes (in CSS pixels); `hop`: how
  // high (in the logo's pixels); `sway`: how far it leans from side to side.
  const GAITS = {
    walk: { speed: 32, step: 15, hop: 9, sway: 0.07, paw: 14 },
    trot: { speed: 66, step: 22, hop: 16, sway: 0.08, paw: 16 },
    run: { speed: 150, step: 38, hop: 40, sway: 0.05, paw: 10 },
  };

  // The dog in its box on `canvas`, drawn from the parts at `art` (an image
  // URL). Returns what the page talks to it with.
  function Life(canvas, art) {
    const ctx = canvas.getContext("2d");
    const img = new Image();
    let ready = false;
    img.onload = () => { ready = true; };
    img.src = art || "/dog-parts.png";
    const me = {
      talking: false,   // MapleSyrup is speaking
      level: null,      // how loud the voice is right now (0..1, or a function giving it), when the page can hear it
      listening: false, // the player is talking
      thinking: false,  // it's working on an answer
      bark: "Woof!",
      anchor: [0, 0],   // the top of its head, for the speech bubble
      react, start, stop, resize,
      // Do this now (for trying it out).
      play(name) { reaction = null; roll = 0; next(name); },
      get doing() { return reaction ? reaction.name : mode || (act && act.name) || ""; },
    };
    let W = 300, H = 150, dpr = 1, S = 0.25, ground = 130, minX = 60, maxX = 240, room = null;
    const p = Pose();
    let dir = 1, speed = 0, want = 0, gait = "walk", phase = 0, lift = 0, vy = 0, roll = 0, spinAngle = null;
    let act = null, mode = null, reaction = null, calm = 0;
    let energy = 0.8, wagPhase = 0, wagAmp = 0.15;
    let blinkIn = rand(1, 4), blinkT = -1, lid = 0;
    let flap = { open: 0, left: 0 };
    let clock = 0, last = 0, raf = 0, drew = 0, visible = true;
    const effects = [];
    const ball = { x: 200, vx: 0, a: 0, r: 6.5 };
    let bedX = 70;
    const T = {}; // what it's after this frame

    // ---- sizes ----
    function resize() {
      const box = canvas.getBoundingClientRect();
      const w = Math.max(160, Math.round(box.width)), h = Math.max(100, Math.round(box.height));
      const d = Math.min(2, window.devicePixelRatio || 1);
      if (w === W && h === H && d === dpr && room) return;
      const first = !room;
      W = w; H = h; dpr = d;
      canvas.width = Math.round(W * dpr); canvas.height = Math.round(H * dpr);
      ground = H - 14;
      // As tall as the box allows, leaving room to jump.
      S = clamp((H - 14 - 30) / TALL, 0.12, 0.4);
      const half = 135 * S;
      minX = half + 6; maxX = W - half - 6;
      bedX = Math.max(minX, 58 * Math.max(S / 0.25, 0.8));
      room = roomPicture(W, H, ground - 8, dpr);
      if (first) { p.x = W * 0.55; ball.x = W * 0.82; }
      p.x = clamp(p.x, minX, maxX);
      ball.x = clamp(ball.x, ball.r + 2, W - ball.r - 2);
    }

    // ---- getting about ----
    // Going to x, hopping (`kind`: walk, trot, run). True once there.
    function goTo(x, kind, near) {
      x = clamp(x, minX, maxX);
      const dx = x - p.x;
      if (Math.abs(dx) <= (near || 4)) { want = 0; return speed < 6; }
      dir = Math.sign(dx);
      gait = kind;
      want = Math.min(GAITS[kind].speed, Math.max(14, Math.abs(dx) * 2.5));
      return false;
    }

    function pickSpot(away) {
      for (let i = 0; i < 8; i++) {
        const x = rand(minX, maxX);
        if (Math.abs(x - p.x) >= away) return x;
      }
      return p.x < W / 2 ? maxX : minX;
    }

    function next(name) { act = { name, t: 0, dt: 0, seed: Math.random() }; }

    // ---- what it does on its own ----
    // Each: what it's after, from how long it's been at it; true when done.
    const ACTS = {
      stand(a) {
        if (!a.dur) { a.dur = rand(1.6, 4); a.look = rand(-1, 1); a.at = rand(0.3, 1.5); }
        if (a.t > a.at) { T.lookX = a.look; T.headRot = a.look * 0.06; T.headX = a.look * 4; }
        return a.t > a.dur;
      },
      wander(a) {
        if (a.to == null) a.to = pickSpot(50);
        T.lookX = dir * 0.8; T.headX = dir * 5;
        return goTo(a.to, a.kind || "walk", 5);
      },
      trot(a) { a.kind = "trot"; return ACTS.wander(a); },
      zoomies(a) {
        if (a.legs == null) { a.legs = Math.floor(rand(3, 6)); a.to = p.x < W / 2 ? maxX : minX; energy -= 0.12; }
        T.happy = true; T.mouth = 0.75; T.wagAmp = 0.35; T.wagSpeed = 9;
        if (a.legs > 0) {
          T.lean = dir * 0.12; T.lookX = dir;
          if (goTo(a.to, "run", 18)) { a.legs--; a.to = a.to === maxX ? minX : maxX; }
          return false;
        }
        // Out of breath.
        a.pant = (a.pant || 0) + a.dt;
        T.happy = a.pant < 1.2; T.mouth = 0.55 + 0.2 * Math.sin(a.t * 17);
        return a.pant > 2.6;
      },
      sit(a) {
        if (!a.dur) a.dur = rand(3, 7);
        T.headRot = 0.08 * Math.sin(a.t * 0.9 + a.seed * 6);
        if (energy < 0.45) T.mouth = 0.5 + 0.15 * Math.sin(a.t * 15);
        return a.t > a.dur;
      },
      scratch(a) {
        if (!a.dur) a.dur = rand(1.6, 2.6);
        if (a.t < a.dur) {
          T.headRot = 0.22; T.headX = 5; T.happy = true; T.lean = 0.05;
          T.pawR = 22 + 6 * Math.sin(clock * TAU * 8);
        } else T.headRot = 0.3 * Math.sin((a.t - a.dur) * 40) * clamp(1 - (a.t - a.dur) / 0.6, 0, 1);
        return a.t > a.dur + 0.7;
      },
      nap(a) {
        if (a.step == null) { a.step = 0; a.turns = 2; }
        if (a.step === 0) { if (goTo(bedX, "walk", 3)) { a.step = 1; a.at = a.t; } return false; }
        if (a.step === 1) { // round and round before lying down
          const t = a.t - a.at;
          T.lean = 0.08 * Math.sin(t * TAU * 1.6); lift = Math.max(lift, 6 * Math.abs(Math.sin(t * TAU * 1.6)));
          if (t > 1.3) { a.step = 2; a.at = a.t; a.dur = rand(16, 30); }
          return false;
        }
        T.low = 1;
        if (a.step === 2) { if (a.t - a.at > 1.2) { a.step = 3; a.at = a.t; } T.blink = 0.5; return false; }
        if (a.step === 3) { // asleep
          T.blink = 1; T.headRot = 0.14; T.headY = 10; T.wagAmp = 0; a.sleeping = true;
          energy = Math.min(1, energy + 0.035 * a.dt);
          if ((a.z = (a.z || 0) - a.dt) <= 0) { a.z = 1.5; effect("z"); }
          if (a.t - a.at > a.dur) { a.step = 4; a.at = a.t; a.sleeping = false; }
          return false;
        }
        // Waking up: a big yawn.
        const y = a.t - a.at;
        T.low = clamp(1 - y / 1.8, 0, 1);
        T.mouth = y < 1.6 ? 1.2 * Math.sin(Math.PI * clamp(y / 1.6, 0, 1)) : 0;
        T.blink = y < 1.4 ? 1 : 0; T.headRot = y < 1.6 ? -0.12 : 0;
        return y > 2.2;
      },
      jump(a) { return jumpScript(a, 520, false); },
      spin(a) {
        if (!a.turns) { a.turns = Math.round(rand(2, 3)); spinAngle = 0; }
        spinAngle += a.dt * TAU / 0.65;
        T.happy = true; T.mouth = 0.6; T.wagAmp = 0.35; T.wagSpeed = 10;
        lift = Math.max(lift, 10 * Math.abs(Math.sin(spinAngle)));
        if (spinAngle >= a.turns * TAU) { spinAngle = null; return true; }
        return false;
      },
      bow(a) { // down low, bottom wiggling, then a pounce
        if (!a.dur) { a.dur = rand(1.1, 1.7); a.dir = Math.random() < 0.5 ? -1 : 1; }
        if (a.t < a.dur) {
          T.low = 0.7; T.headY = 12; T.lean = 0.05 * Math.sin(a.t * TAU * 4.5); T.wagAmp = 0.4; T.wagSpeed = 13;
          T.lookX = a.dir; T.mouth = 0.25;
          return false;
        }
        if (!a.leapt) { a.leapt = true; vy = 420; lift = 0.1; dir = a.dir; gait = "trot"; }
        T.happy = true; T.mouth = 0.7;
        if (lift > 0) { want = 120; return false; }
        return true;
      },
      roll(a) { // over and over, sideways
        if (a.dir == null) a.dir = p.x < W / 2 ? 1 : -1;
        T.happy = true; T.mouth = 0.5;
        const r = clamp(a.t / 1.1, 0, 1);
        roll = smooth(r) * TAU * a.dir;
        if (r < 1) { p.x = clamp(p.x + a.dir * 45 * a.dt, minX, maxX); return false; }
        roll = 0;
        return a.t > 1.4;
      },
      sniff(a) {
        if (!a.dur) { a.dur = rand(2.5, 4.5); a.to = pickSpot(30); }
        T.headY = 18; T.headRot = 0.06 * Math.sin(a.t * 7); T.lookY = 1; T.blink = 0.35; T.low = 0.25;
        T.lookX = dir * 0.6;
        if (Math.sin(a.t * 2.2) < 0.55) goTo(a.to, "walk", 4); else want = 0;
        return a.t > a.dur;
      },
      ball(a) {
        if (a.pushes == null) a.pushes = Math.round(rand(2, 4));
        T.wagAmp = 0.3; T.wagSpeed = 9;
        if (Math.abs(ball.vx) > 25) { // after it
          T.lookX = Math.sign(ball.vx);
          goTo(ball.x - Math.sign(ball.vx) * 30 * S / 0.25, "trot", 10);
          return false;
        }
        const side = ball.x > p.x ? 1 : -1;
        const spot = ball.x - side * 52 * S / 0.25;
        T.lookX = side; T.lookY = 0.6;
        if (Math.abs(p.x - spot) > 5) { goTo(spot, Math.abs(p.x - spot) > 80 ? "trot" : "walk", 4); return false; }
        if (a.pushes <= 0) return true;
        // A nudge with the nose.
        a.nudge = (a.nudge || 0) + a.dt;
        T.headY = 16; T.headX = side * 10; T.headRot = side * 0.12; T.lean = side * 0.06;
        if (a.nudge > 0.35) {
          ball.vx = side * rand(110, 200);
          a.pushes--; a.nudge = 0;
          if ((ball.x < W * 0.2 && side < 0) || (ball.x > W * 0.8 && side > 0)) ball.vx *= -0.3;
        }
        return false;
      },
      shake(a) {
        const k = clamp(1 - a.t / 1.1, 0, 1);
        T.lean = 0.16 * Math.sin(a.t * TAU * 8) * k;
        T.headRot = -0.22 * Math.sin(a.t * TAU * 8) * k;
        T.happy = k > 0.15;
        return a.t > 1.3;
      },
      yawn(a) {
        const k = Math.sin(Math.PI * clamp(a.t / 1.6, 0, 1));
        T.mouth = 1.2 * k; T.blink = k > 0.35 ? 1 : 0; T.headRot = -0.1 * k; T.headY = -4 * k;
        return a.t > 2;
      },
      bark(a) {
        if (!a.n) a.n = Math.random() < 0.5 ? 1 : 2;
        const b = Math.floor(a.t / 0.42);
        if (b < a.n && a.t - b * 0.42 < 0.16) { T.mouth = 0.95; T.headY = -6; }
        if (b < a.n && a.barked !== b) { a.barked = b; effect("woof"); }
        T.wagAmp = 0.25;
        return a.t > a.n * 0.42 + 0.4;
      },
    };

    // A jump: crouch, up (`power`: how fast, in the logo's pixels a
    // second), land. `twirl`: spinning on the way.
    function jumpScript(a, power, twirl) {
      if (a.phase == null) { a.phase = 0; }
      T.happy = true; T.mouth = 0.6; T.wagAmp = 0.35; T.wagSpeed = 10;
      if (a.phase === 0) { // down first
        T.squash = 0.84; T.low = 0.3;
        if (a.t > 0.16) { a.phase = 1; vy = power; lift = 0.1; if (twirl) spinAngle = 0; }
        return false;
      }
      if (a.phase === 1) { // in the air: stretched, paws hanging
        T.squash = 1.07; T.pawL = -7; T.pawR = -7;
        if (twirl && spinAngle != null) spinAngle += a.dt * TAU / 0.55;
        if (lift <= 0) { a.phase = 2; a.at = a.t; spinAngle = null; }
        return false;
      }
      T.squash = 0.86;
      return a.t - a.at > 0.14;
    }

    // What to do next, as it feels like: lively when rested, sleepy when tired.
    function choose() {
      const tired = 1 - energy;
      const options = [
        ["stand", 3], ["wander", 4], ["trot", 1.5 * energy], ["zoomies", energy > 0.55 ? 0.7 : 0],
        ["sit", 2.2], ["scratch", 0.8], ["nap", tired > 0.55 ? 4 * tired : 0.12], ["jump", 0.9 * energy],
        ["spin", 0.6 * energy], ["bow", 0.6 * energy], ["roll", 0.45], ["sniff", 1.3], ["ball", 1.3 * energy + 0.2],
        ["shake", 0.45], ["yawn", tired > 0.4 ? 0.8 : 0.15], ["bark", 0.12],
      ].filter(([n]) => !act || n !== act.name || n === "stand" || n === "wander");
      let total = 0;
      for (const [, w] of options) total += w;
      let r = Math.random() * total;
      for (const [n, w] of options) { r -= w; if (r <= 0) return n; }
      return "stand";
    }

    // ---- what happens to it ----
    // "levelup": jumps for joy; "alert": barks, with a "!" (a warning —
    // a low bar, a beating; news gets no bark: a death and a level-up are
    // reacted to from the game); "death": plays dead; "hello": a happy
    // hop; "tap" (x): pets or calls it.
    function react(kind, x) {
      roll = 0; spinAngle = null; act = null;
      if (kind === "tap") {
        if (Math.abs(x - p.x) < 60 * S / 0.25) { reaction = { name: "petted", t: 0, dt: 0 }; for (let i = 0; i < 3; i++) setTimeout(() => effect("heart"), i * 180); }
        else reaction = { name: "come", t: 0, dt: 0, to: x };
        return;
      }
      reaction = { name: kind, t: 0, dt: 0 };
      if (kind === "levelup") { for (let i = 0; i < 6; i++) setTimeout(() => effect("star"), i * 110); }
      if (kind === "alert") effect("!");
    }
    const REACTIONS = {
      levelup(a) {
        if (a.hops == null) a.hops = 2;
        if (jumpScript(a, a.hops === 2 ? 640 : 460, a.hops === 2)) { a.hops--; a.phase = null; a.t = 0; }
        return a.hops <= 0;
      },
      alert(a) {
        const b = Math.floor(a.t / 0.38);
        if (b < 2 && a.t - b * 0.38 < 0.15) { T.mouth = 1; T.headY = -7; }
        if (b < 2 && a.barked !== b) { a.barked = b; effect("woof"); }
        T.wagAmp = 0.05;
        return a.t > 1.3;
      },
      death(a) { // over on its side, paws up, x x
        const r = a.t < 0.3 ? 0 : a.t < 0.7 ? smooth((a.t - 0.3) / 0.4) : a.t < 4 ? 1 : 1 - smooth((a.t - 4) / 0.45);
        roll = -1.45 * r;
        T.dead = r > 0.6; T.mouth = r > 0.6 ? 0.7 : 0; T.wagAmp = 0; T.pawL = 10 * r; T.pawR = 6 * r;
        if (a.t > 4.5) { roll = 0; next("shake"); return true; }
        return false;
      },
      hello(a) { return jumpScript(a, 380, false); },
      petted(a) {
        T.happy = true; T.mouth = 0.5; T.wagAmp = 0.4; T.wagSpeed = 12; T.headRot = 0.12 * Math.sin(a.t * 5);
        return a.t > 1.8;
      },
      come(a) {
        if (!a.there) {
          if (goTo(a.to, Math.abs(a.to - p.x) > 90 ? "run" : "trot", 6)) { a.there = true; a.at = a.t; }
          T.wagAmp = 0.3; T.wagSpeed = 9; T.lookX = dir;
          return a.t > 6;
        }
        T.wagAmp = 0.35; T.wagSpeed = 10; T.happy = a.t - a.at < 1.4; T.mouth = 0.45;
        return a.t - a.at > 2.2;
      },
    };

    function effect(kind) {
      const [hx, hy] = me.anchor;
      effects.push({ kind, x: hx + rand(-8, 8), y: hy + rand(-4, 4), t: 0, life: kind === "z" ? 2.6 : kind === "woof" ? 0.9 : kind === "!" ? 1.2 : 1.4, dx: rand(-0.5, 0.6) });
      if (effects.length > 24) effects.shift();
    }

    // ---- every frame ----
    function update(dt) {
      clock += dt;
      Object.assign(T, { lean: 0, squash: 1, low: 0, headRot: 0, headX: 0, headY: 0, pawL: 0, pawR: 0,
        blink: 0, mouth: 0, happy: false, dead: false, lookX: 0, lookY: 0, wagAmp: 0.15, wagSpeed: 5 });
      want = 0;
      // What happened to it first, then the player, then whatever it likes.
      const was = mode;
      mode = reaction ? null : me.talking ? "talking" : me.listening ? "listening" : me.thinking ? "thinking" : null;
      if (reaction) {
        reaction.t += dt; reaction.dt = dt;
        if (REACTIONS[reaction.name](reaction)) { reaction = null; calm = 0.6; }
      } else if (mode) {
        if (!was) { spinAngle = null; roll = 0; if (act && act.name !== "stand") act = null; }
        attend();
      } else {
        if (was) { calm = rand(1, 2.2); act = null; }
        if (calm > 0) calm -= dt;
        else {
          if (!act) next(choose());
          act.t += dt; act.dt = dt;
          if (ACTS[act.name](act)) act = null;
        }
      }
      // Tiring (resting when asleep).
      const effort = speed > 100 ? 0.03 : speed > 45 ? 0.01 : speed > 1 ? 0.004 : 0.0015;
      if (!(act && act.sleeping)) energy = clamp(energy - effort * dt, 0, 1);

      // Getting about: hop by hop.
      const g = GAITS[gait];
      speed = speed + clamp(want - speed, -dt * 320, dt * 260);
      p.x += dir * speed * dt;
      if (p.x < minX || p.x > maxX) { p.x = clamp(p.x, minX, maxX); speed = 0; }
      const moving = clamp(speed / 10, 0, 1);
      if (speed > 0.5) phase += speed / g.step * dt;
      const hopAt = Math.sin(phase * Math.PI); // one hop per step, paw by paw
      // In the air (a jump, a pounce).
      if (lift > 0.05 || vy > 0) { lift += vy * dt; vy -= 2400 * dt; if (lift <= 0) { lift = 0; vy = 0; } }
      // The ball rolls, and is kicked when run into.
      ball.x += ball.vx * dt; ball.a += ball.vx * dt / ball.r; ball.vx *= Math.exp(-1.1 * dt);
      if (Math.abs(ball.vx) < 4) ball.vx = 0;
      if (ball.x < ball.r + 2) { ball.x = ball.r + 2; ball.vx = Math.abs(ball.vx) * 0.6; }
      if (ball.x > W - ball.r - 2) { ball.x = W - ball.r - 2; ball.vx = -Math.abs(ball.vx) * 0.6; }
      const reach = 34 * S / 0.25;
      if (speed > 20 && Math.abs(p.x + dir * reach - ball.x) < ball.r + 4 && Math.sign(ball.x - p.x) === dir) ball.vx = dir * (speed + 40);

      // The pose eases towards what it's after.
      const k = (v, to, r) => ease(v, to, r, dt);
      p.lean = k(p.lean, T.lean + moving * g.sway * hopAt * (gait === "run" ? 0.6 : 1) + (gait === "run" ? dir * 0.1 * moving : 0), 14);
      p.squash = k(p.squash, T.squash * (1 - 0.1 * (p.low || 0)), 16);
      p.low = k(p.low || 0, T.low, 5);
      p.headRot = k(p.headRot, T.headRot, 8);
      p.headX = k(p.headX, T.headX, 7);
      p.headY = k(p.headY, T.headY + 14 * p.low, 7);
      p.lift = (lift + moving * g.hop * Math.abs(hopAt)) * 1;
      p.pawL = k(p.pawL, T.pawL + (moving && hopAt > 0 ? g.paw * hopAt : 0), 18);
      p.pawR = k(p.pawR, T.pawR + (moving && hopAt < 0 ? -g.paw * hopAt : 0), 18);
      p.spin = spinAngle == null ? k(p.spin, 1, 12) : 0.28 + 0.72 * Math.abs(Math.cos(spinAngle));
      p.roll = roll;
      // Breathing; slow and deep asleep.
      const asleep = act && act.sleeping;
      p.breath = 1 + Math.sin(clock * TAU * (asleep ? 0.28 : 0.45)) * (asleep ? 0.016 : 0.008);
      // The face.
      const f = p.face;
      f.lookX = k(f.lookX, mode ? 0 : T.lookX, 6); f.lookY = k(f.lookY, T.lookY, 6);
      f.happy = T.happy; f.dead = T.dead;
      blinkIn -= dt;
      if (blinkIn <= 0) { blinkT = 0; blinkIn = rand(1.8, 5.5); }
      let blink = 0;
      if (blinkT >= 0) { blinkT += dt; blink = blinkT < 0.07 ? blinkT / 0.07 : blinkT < 0.16 ? 1 - (blinkT - 0.07) / 0.09 : 0; if (blinkT > 0.16) blinkT = -1; }
      lid = k(lid, T.blink, 10);
      f.blink = Math.max(blink, lid);
      // The mouth: with the voice when it talks.
      // (Even while it's busy with something else: it talks on the go.)
      let open = T.mouth;
      if (me.talking && !T.dead) open = Math.max(open, talkingMouth(dt));
      f.mouth = ease(f.mouth, open, open > f.mouth ? 40 : 16, dt);
      if (me.talking) p.headY -= f.mouth * 3;
      // The tail.
      wagPhase += dt * TAU * T.wagSpeed / 2;
      wagAmp = k(wagAmp, T.wagAmp, 4);
      p.tail = wagAmp * Math.sin(wagPhase);
      // Effects drift off.
      for (let i = effects.length - 1; i >= 0; i--) {
        const e = effects[i];
        e.t += dt;
        if (e.t > e.life) effects.splice(i, 1);
      }
    }

    // The mouth while it talks: as loud as the voice, when the page can hear
    // it; else syllables, like speech.
    function talkingMouth(dt) {
      const level = typeof me.level === "function" ? me.level() : me.level;
      if (typeof level === "number") return clamp((level - 0.15) * 1.4, 0, 1.05);
      flap.left -= dt;
      if (flap.left <= 0) {
        if (flap.open > 0 && Math.random() < 0.3) { flap.open = 0; flap.left = rand(0.05, 0.12); }
        else { flap.open = Math.random() < 0.12 ? 0 : rand(0.35, 1); flap.left = rand(0.09, 0.2); }
      }
      return flap.open;
    }

    // While the player talks, it thinks, or MapleSyrup talks: it stops and
    // looks at the player.
    function attend() {
      T.wagSpeed = 6;
      if (mode === "talking") { T.wagAmp = 0.22; T.headRot = 0.04 * Math.sin(clock * 2.1); }
      if (mode === "listening") { T.wagAmp = 0.12; T.headRot = 0.14 * Math.sin(clock * 1.3); T.lookY = -0.2; }
      if (mode === "thinking") {
        const side = Math.sin(clock * TAU / 2.4) > 0 ? 1 : -1;
        T.headRot = 0.2 * side; T.headX = 4 * side; T.lookY = -1; T.lookX = 0.5 * side; T.wagAmp = 0.06;
      }
    }

    function draw() {
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.imageSmoothingQuality = "high";
      ctx.drawImage(room, 0, 0, W, H);
      bed(ctx, bedX, ground + 2, S / 0.25 * 0.95, false);
      p.y = ground + 3; p.scale = S;
      // In its bed: on the cushion.
      p.y -= 5 * clamp(1 - Math.abs(p.x - bedX) / (40 * S / 0.25), 0, 1);
      if (ready) me.anchor = drawDog(ctx, img, p);
      bed(ctx, bedX, ground + 2, S / 0.25 * 0.95, true);
      drawBall(ctx, ball.x, ground + 6, ball.r * Math.max(0.8, S / 0.25), ball.a);
      for (const e of effects) drawEffect(e);
    }

    function drawEffect(e) {
      const t = e.t / e.life, k = Math.max(0.8, S / 0.25);
      ctx.save();
      ctx.globalAlpha = t < 0.15 ? t / 0.15 : t > 0.7 ? (1 - t) / 0.3 : 1;
      if (e.kind === "z") {
        ctx.translate(e.x + 16 * k + t * 22 * k + Math.sin(t * 6) * 3, e.y + 4 - t * 30 * k);
        ctx.rotate(-0.2);
        ctx.fillStyle = "#cfe3ff";
        ctx.font = "bold " + Math.round((9 + t * 7) * k) + "px -apple-system, 'Segoe UI', sans-serif";
        ctx.fillText("z", 0, 0);
      } else if (e.kind === "heart") {
        heart(ctx, e.x + e.dx * 30 * t, e.y - t * 34 * k, (1 + t * 0.6) * k);
      } else if (e.kind === "star") {
        ctx.translate(e.x + e.dx * 80 * t, e.y - t * 40 * k + 10);
        ctx.rotate(t * 3);
        ctx.fillStyle = "#ffd76a";
        ctx.beginPath();
        for (let i = 0; i < 8; i++) {
          const r = (i % 2 ? 1.6 : 5) * k, a = (i / 8) * TAU;
          ctx.lineTo(Math.cos(a) * r, Math.sin(a) * r);
        }
        ctx.closePath(); ctx.fill();
      } else if (e.kind === "woof" || e.kind === "!") {
        const text = e.kind === "woof" ? me.bark : "!";
        const pop = t < 0.15 ? 0.6 + 0.4 * (t / 0.15) : 1;
        const bx = clamp(e.x + 34 * k, 30, W - 30), by = Math.max(16, e.y);
        ctx.translate(bx, by); ctx.scale(pop, pop);
        ctx.font = "800 " + Math.round(13 * k) + "px -apple-system, 'Segoe UI', sans-serif";
        const w = ctx.measureText(text).width + 14, hh = 20 * k;
        ctx.fillStyle = e.kind === "!" ? "#ffcf4d" : "#fff7ea";
        ctx.beginPath();
        if (ctx.roundRect) ctx.roundRect(-w / 2, -hh / 2, w, hh, hh / 2); else ctx.rect(-w / 2, -hh / 2, w, hh);
        ctx.fill();
        ctx.fillStyle = "#2a1c10"; ctx.textAlign = "center"; ctx.textBaseline = "middle";
        ctx.fillText(text, 0, 1);
      }
      ctx.restore();
    }

    function frame(now) {
      raf = requestAnimationFrame(frame);
      const dt = last ? Math.min(0.1, (now - last) / 1000) : 0.016;
      // About thirty times a second; slower asleep.
      const every = act && act.sleeping && !effects.length && !mode ? 66 : 32;
      if (now - drew < every - 4) return;
      last = now;
      update(dt);
      if (visible) { draw(); drew = now; }
    }

    function start() {
      resize();
      if (!raf) { last = 0; raf = requestAnimationFrame(frame); }
    }
    function stop() { if (raf) cancelAnimationFrame(raf); raf = 0; }
    // How long a frame takes to work out and draw, in ms (for trying it out).
    me.bench = (n) => { const t0 = performance.now(); for (let i = 0; i < n; i++) { update(1 / 30); draw(); } return (performance.now() - t0) / n; };

    if (window.ResizeObserver) new ResizeObserver(() => resize()).observe(canvas);
    if (window.IntersectionObserver) new IntersectionObserver((e) => { visible = e[0].isIntersecting; }).observe(canvas);
    canvas.addEventListener("pointerdown", (e) => {
      const box = canvas.getBoundingClientRect();
      react("tap", e.clientX - box.left);
    });
    return me;
  }

  root.MSDog = { Life, drawDog, Pose, ART };
})(typeof window !== "undefined" ? window : globalThis);
