"""The phone page against a stand-in PC, in a headless browser: no console
errors, the main screen shows only the essentials (the language picker
among them), the gear opens Settings, the voice search narrows the list,
the live-call toggle flipped before Listen says hello again, sharing play
stats starts off (on posts it, the view is JSON with no name in it,
"Delete it" posts the delete and says "Deleted" only on the PC's ok — a
delete the PC fails says so, in the page's words, in English and Hebrew,
as does a turn-on refused for the age box), the game line opens Details
(the last sessions in a table, "Levels up"), Hebrew is right to left (and
at 320 px the game line loses its end, not the level); then the
turn-taking, with a stand-in speech
recognizer, a stand-in call and a microphone fed from a file: the words so
far never land after the sentence, a sentence cut off by a clip is still
sent, a loud sound over a clip pauses it until the PC's word, a PC started
again is greeted again and its clips play, a page reloaded mid-visit is not
greeted twice, and on a call MapleSyrup's own lines are said by the call,
never by the phone's own voice: the call's hello says what it does to a
player the PC does not know, lines are handed over with the reading behind
them and the game as read now, never while the call is answering (one turn
asked for at a time), a warning dropped once stale (and the PC told), a
death or a level-up said however late, a warning's row red and news's amber
(the dog barks at a warning only), a change of attitude retunes the call in
place, Hebrew heard on the call sets the recogniser's language (two
sentences, or one long one; two English ones set it back; shown beside the
picker, with a × back), a language the player asked the PC for out loud is
taken once per request as if picked in the picker, a glance at another app
tells the PC nothing, a PC
that lost the call is told it is on, the page leaving tells it the call is
off, and a call that fails to open hands its hello back; and, under the
browser's own autoplay policy (a phone's: no clip before a tap), the hello
clip made before the tap is kept for it, and the terms with it, but not a
stale warning or stale news. Needs `pip install playwright &&
playwright install chromium`; run from the repository root: `python3
tools/phone_ui_check.py` (or with some of `ui`, `recognition`, `live`,
`loudness`, `hello` to run those alone; PHONE_PAGE=path checks another
copy of the page, to see a check fail against the page as it was).
Screenshots land in `target/phone-ui/`."""
import json, os, re, struct, sys, threading, time, http.server, socketserver
from urllib.parse import parse_qs, urlparse
from playwright.sync_api import sync_playwright

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(HERE)
ROOT = os.path.join(REPO, "src", "phone")
# The page under test (PHONE_PAGE: another copy of it, to see a check fail
# against the page as it was).
PAGE = os.environ.get("PHONE_PAGE") or os.path.join(ROOT, "page.html")
SHOTS = os.path.join(REPO, "target", "phone-ui")
os.makedirs(SHOTS, exist_ok=True)
STATUS = {
    "game": {"state": "seen", "detail": "MapleStory"},
    "hp": {"percent": 100.0, "current": 6370, "max": 6370, "read": True},
    "mp": {"percent": 98.4, "current": 3516, "max": 3574, "read": True},
    "exp": {"percent": 18.99, "current": None, "max": None, "read": True},
    "level": 152, "name": "WanWanBoggio", "job": None,
    "progress": {"seconds": 1200.0, "exp_per_hour": 1.2, "seconds_to_level": 7200.0, "levels_gained": 0, "marks": 0},
    "muted": False, "dead": False, "fps": 10.0, "wake": "syrup", "always_listen": True, "coach": True,
    "update": {"version": "0.9.0", "state": "staged", "detail": "0.9.1", "latest": "0.9.1", "auto": True, "checked_secs_ago": 5, "notes": ""},
    "workshop": {"on": True, "coder": "Claude Code", "coders": ["Claude Code", "Codex"], "repo": "C:\\Users\\me\\GitHub\\ms", "working": "building", "working_secs": 95, "queued": 0, "last": None},
    "speaking": False, "speaking_pc": False, "thinking": False, "ai": "grok-4.3",
    "learned": {"things": [], "hud": True, "level": 152, "level_from": "screen"},
    "live": True, "call_greets": True, "recording": {"state": "off"}, "attitude": "savage",
    "voices": [
        {"id": "v1", "name": "Rachel", "about": "female · calm · american"},
        {"id": "v2", "name": "Adam", "about": "male · deep · american"},
        {"id": "v3", "name": "Antoni", "about": "male · well-rounded"},
        {"id": "v4", "name": "Bella", "about": "female · soft"},
        {"id": "v5", "name": "Domi", "about": "female · strong"},
        {"id": "v6", "name": "Josh", "about": "male · young · deep"},
    ],
    "voice": "v2", "settle_ms": 550, "memory": None, "warn": {"hp": 30.0, "mp": 15.0},
}
# How long the page waits for the PC's word on a loud sound before the
# clip goes on, and a little more.
PAUSE_SLACK = 2000
# The stand-in PC's state, as the page polls it (/api/state), and every
# request the page made. `clips`: what a clip is and when it was made, by
# its number (a clip not in it is listed by nobody, as a PC from before
# would).
STATE = {"boot": "b1", "clip": 0, "cut": 0, "messages": [], "voice_on": "both", "clips": {}}
REQUESTS = []
LOCK = threading.Lock()

def pcm(seconds, rate, amplitude=0):
    """`seconds` of silence, or of a square wave of this amplitude."""
    frames = int(seconds * rate)
    if not amplitude: return bytes(frames * 2)
    return b"".join(struct.pack("<h", amplitude if (i // 20) % 2 else -amplitude) for i in range(frames))

def wav(rate, data):
    return (b"RIFF" + struct.pack("<I", 36 + len(data)) + b"WAVEfmt " + struct.pack("<IHHIIHH", 16, 1, 1, rate, rate * 2, 2, 16)
            + b"data" + struct.pack("<I", len(data)) + data)

# The spoken lines the stand-in PC hands out: silent, this long.
CLIP_SECONDS = {"default": 8}
# What the stand-in PC reads off the game for a call (/api/eyes), and the
# call's instructions once the attitude has changed (/api/instructions).
EYES = "The MapleStory window is open and in view.\nCharacter: level 152.\nHP 11%, MP about 40%."
INSTRUCTIONS = "You are MapleSyrup. Your attitude: friendly."
# Whether the stand-in PC can start a call (/api/live): off, a call that
# fails to open.
LIVE = {"opens": True}
# The stand-in PC's play stats: whether the player shares (off unless
# turned on), how many of the next deletions it fails (a file of it still on
# the PC: 500 and the code, as the PC answers), a session as the export has
# it (the fields of `metrics::EXPORT_FIELDS`), and the last sessions for
# Details. (`available`: whether the PC offers sharing at all — the real
# build does not yet; the page's flow is checked as it would be once it
# does, then the card as it is now.)
SHARE = {"on": False, "available": True}
SHARE_FAILS = {"left": 0}
SHARED_SESSION = {"week": "2026-W41", "minutes": 95, "game_minutes": 90, "levels_gained": 2, "level_start_band": "141-200",
                  "level_end_band": "141-200", "characters": 1, "job": "Night Lord", "hud": "modern", "deaths": 1,
                  "warnings": {"hp_low": 3, "mp_low": 1, "beating": 2, "taught": 0}, "close_calls": 1, "potions_answered": 0.7,
                  "exp_per_hour": 1.2, "maps": 4, "map_visits": 9, "sentences": 40, "replies": 38, "reply_ms_median": 1200,
                  "instant_answers": 5, "call_minutes": 60, "clip_minutes": 20, "coach_looks": 30, "coach_lines": 9, "attitude": "savage",
                  "language": "he", "version": "0.9.0", "windows": "11", "screen": "4K", "ai_errors": 0, "voice_errors": 0}
SESSIONS = [{"day": "2026-10-09", "minutes": 95, "levels_gained": 2, "deaths": 1, "warnings": {"hp_low": 3, "mp_low": 1, "beating": 2, "taught": 0}},
            {"day": "2026-10-08", "minutes": 40, "levels_gained": 0, "deaths": 0, "warnings": {}}]
def share_view():
    on = SHARE["on"]
    return {"available": SHARE["available"], "on": on, "since": "2026-10-09" if on else None, "preview": not on,
            "export": {"format": 1, "install_id": "6f1c2a9e-3b7d-4c55-9a0e-2d8f1b7c4e31" if on else None, "app_version": "0.9.0",
                       "sessions": [SHARED_SESSION]}}

class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def _send(self, code, body, ctype):
        try:
            self.send_response(code); self.send_header("Content-Type", ctype); self.send_header("Content-Length", str(len(body))); self.end_headers(); self.wfile.write(body)
        except ConnectionError: pass  # (the page went away)
    def _log(self, body=None):
        url = urlparse(self.path)
        with LOCK:
            REQUESTS.append({"method": self.command, "path": url.path, "query": {k: v[0] for k, v in parse_qs(url.query).items()}, "body": body, "t": time.time()})
    def do_GET(self):
        url = urlparse(self.path); path = url.path; q = {k: v[0] for k, v in parse_qs(url.query).items()}
        if path == "/": self._send(200, open(PAGE, "rb").read(), "text/html; charset=utf-8")
        elif path == "/dog.js": self._send(200, open(ROOT + "/dog.js", "rb").read(), "application/javascript")
        elif path == "/dog-parts.png": self._send(200, open(os.path.join(REPO, "assets", "companion", "dog-parts.png"), "rb").read(), "image/png")
        elif path == "/api/state":
            self._log()
            if q.get("wait", "0") != "0": time.sleep(0.1)  # (no long poll: a short one)
            with LOCK:
                since = int(q.get("since", "0") or 0)
                messages = [m for m in STATE["messages"] if m["id"] > since]
                clips = [{"seq": seq, "kind": kind, "age_ms": int((time.time() - made) * 1000)} for seq, (kind, made) in sorted(STATE["clips"].items())]
                body = {"status": STATUS, "mic": {"live": False, "level": 0, "speaking": False}, "voice_on": STATE["voice_on"],
                        "messages": messages, "last_id": len(STATE["messages"]), "uptime": 100.0, "boot": STATE["boot"], "clip": STATE["clip"],
                        "clips": clips, "cut": STATE["cut"]}
            self._send(200, json.dumps(body).encode(), "application/json")
        elif path == "/api/clip":
            self._log()
            with LOCK: seconds = CLIP_SECONDS.get(q.get("seq"), CLIP_SECONDS["default"])
            self._send(200, wav(8000, pcm(seconds, 8000)), "audio/wav")
        elif path == "/api/mouth": self._log(); self._send(200, b'{"step_ms": 40, "levels": []}', "application/json")
        elif path == "/api/eyes": self._log(); self._send(200, json.dumps({"snapshot": EYES, "image": None}).encode(), "application/json")
        elif path == "/api/instructions": self._log(); self._send(200, json.dumps({"instructions": INSTRUCTIONS, "attitude": "friendly"}).encode(), "application/json")
        elif path == "/api/share": self._log(); self._send(200, json.dumps(share_view()).encode(), "application/json")
        elif path == "/api/stats": self._log(); self._send(200, json.dumps({"sessions": SESSIONS, "share": {"on": SHARE["on"]}}).encode(), "application/json")
        else: self._log(); self._send(404, b"{}", "application/json")
    def do_POST(self):
        n = int(self.headers.get("Content-Length") or 0); raw = self.rfile.read(n)
        body = None
        try: body = json.loads(raw)
        except Exception: pass
        self._log(body)
        path = urlparse(self.path).path
        if path == "/api/live" and not LIVE["opens"]: self._send(200, b'{"error": "no key"}', "application/json")
        elif path == "/api/live": self._send(200, json.dumps({"key": "ek_test", "url": f"http://127.0.0.1:{port}/sdp", "hint": "", "api": "ga", "attitude": "savage"}).encode(), "application/json")
        elif path == "/sdp": self._send(200, b"v=0\r\no=- 1 1 IN IP4 127.0.0.1\r\n", "application/sdp")
        elif path in ("/api/share", "/api/share/delete"):
            # As the PC answers: a code for what went wrong (the page has the
            # words), never ok while a file of it is left.
            on = (body or {}).get("on") if path == "/api/share" else False
            if not isinstance(on, bool): self._send(400, b'{"error": "bad_request"}', "application/json")
            elif on and (body or {}).get("adult") is not True: self._send(400, b'{"error": "adult_only"}', "application/json")
            elif not on and SHARE_FAILS["left"] > 0:
                SHARE_FAILS["left"] -= 1
                self._send(500, json.dumps({"error": "not_deleted", "left": ["share-export.json"]}).encode(), "application/json")
            else: SHARE["on"] = on; self._send(200, json.dumps({"ok": True, "share": {"on": on}}).encode(), "application/json")
        else: self._send(200, b'{"ok":true}', "application/json")

socketserver.ThreadingTCPServer.allow_reuse_address = True
srv = socketserver.ThreadingTCPServer(("127.0.0.1", 0), Handler)
srv.daemon_threads = True
port = srv.server_address[1]
threading.Thread(target=srv.serve_forever, daemon=True).start()

# Stand-ins for what a headless browser has not got: speech recognition
# (the test hands it results), a call (its data channel opens on its own
# and keeps what was sent), and the phone's own voice (kept, not spoken).
FAKES = """
(() => {
  class FakeRecognition {
    constructor() { this.onresult = null; this.onerror = null; this.onend = null; this.started = 0; this.aborted = 0; this.running = false; window.__rec = this; }
    start() { if (this.running) throw new Error("already started"); this.running = true; this.started++; }
    stop() { this.running = false; setTimeout(() => this.onend && this.onend(), 0); }
    abort() { this.aborted++; if (!this.running) return; this.running = false; setTimeout(() => this.onend && this.onend(), 0); }
    result(index, transcript, isFinal) {
      const results = [];
      for (let i = 0; i < index; i++) { const r = [{ transcript: "", confidence: 1 }]; r.isFinal = true; results.push(r); }
      const r = [{ transcript, confidence: 1 }]; r.isFinal = !!isFinal; results.push(r);
      if (this.onresult) this.onresult({ resultIndex: index, results });
    }
  }
  window.SpeechRecognition = FakeRecognition; window.webkitSpeechRecognition = FakeRecognition;
  window.__spoken = [];
  if (window.speechSynthesis) {
    window.speechSynthesis.speak = (u) => { if (u.text.trim()) window.__spoken.push(u.text); };
    window.speechSynthesis.cancel = () => {};
  }
  window.__dcSent = [];
  class FakeChannel {
    constructor() { this.readyState = "connecting"; this.onopen = null; this.onmessage = null; this.onclose = null; window.__dc = this; }
    send(s) { window.__dcSent.push(JSON.parse(s)); }
    close() { this.readyState = "closed"; }
  }
  class FakeConnection {
    constructor() { this.connectionState = "new"; this.ontrack = null; this.onconnectionstatechange = null; this.dc = null; }
    addTrack() {}
    createDataChannel() { this.dc = new FakeChannel(); return this.dc; }
    async createOffer() { return { type: "offer", sdp: "v=0\\r\\n" }; }
    async setLocalDescription() {}
    async setRemoteDescription() {
      this.connectionState = "connected";
      setTimeout(() => { if (this.dc && this.dc.readyState === "connecting") { this.dc.readyState = "open"; if (this.dc.onopen) this.dc.onopen(); } }, 0);
    }
    close() { this.connectionState = "closed"; if (this.dc) this.dc.readyState = "closed"; }
  }
  window.RTCPeerConnection = FakeConnection;
})();
"""

errors = []
def watch(page):
    def console(m):
        if m.type != "error": return
        # (The stand-in PC refuses a share change on purpose — 500 for a
        # file left, 400 for the age box — and the browser logs each answer
        # it loads: those are what is checked, not errors.)
        if m.text.startswith("Failed to load resource") and urlparse((m.location or {}).get("url", "")).path in ("/api/share", "/api/share/delete"): return
        errors.append(m.text)
    page.on("console", console)
    page.on("pageerror", lambda e: errors.append(str(e)))

def requests_since(t0, path=None):
    with LOCK:
        return [r for r in REQUESTS if r["t"] >= t0 and (path is None or r["path"] == path)]

def wait_for(check, seconds, what):
    end = time.time() + seconds
    while time.time() < end:
        if check(): return
        time.sleep(0.05)
    raise AssertionError(what)

def reset_pc():
    with LOCK:
        STATE.update({"boot": "b1", "clip": 0, "cut": 0, "messages": [], "clips": {}})
        CLIP_SECONDS.clear(); CLIP_SECONDS["default"] = 8
        REQUESTS.clear()

def ui_checks(browser):
    page = browser.new_page(viewport={"width": 390, "height": 844}, device_scale_factor=2)
    watch(page)
    page.goto(f"http://127.0.0.1:{port}/?k=test")
    page.wait_for_timeout(1500)
    # The main screen: the essentials only.
    assert page.is_hidden("#sheet"), "the sheet starts hidden"
    assert page.is_visible("#listen") and page.is_visible("#strip") and page.is_visible("#muteBtn")
    assert not page.is_visible("#game"), "the game card is in Details"
    assert not page.is_visible("#recBtn")
    # The language is on the main screen (the recogniser's language: a
    # Hebrew speaker on an en-US phone must see where to say so).
    assert page.is_visible("#lang"), "the language picker belongs on the main screen"
    strip = page.inner_text("#stripText")
    assert "Lv 152" in strip and "HP 100%" in strip and "EXP 19%" in strip, strip
    page.screenshot(path=os.path.join(SHOTS, "phone-main.png"), full_page=True)
    # The gear opens Settings; the voices are listed with the chosen one marked.
    page.click("#gear")
    page.wait_for_timeout(200)
    assert page.is_visible("#sheet") and page.is_visible("#paneSettings") and not page.is_visible("#paneDetails")
    assert page.is_visible("#voiceSearch"), "six voices: a search"
    items = page.query_selector_all("#voiceList li")
    assert len(items) == 7, len(items)
    assert page.get_attribute("#voiceList li.sel", "data-id") == "v2"
    assert page.is_checked("#coachBox")
    # The version row: this version, the one staged, and the button to take it now.
    assert page.inner_text("#updateVersion") == "0.9.0"
    assert "0.9.1" in page.inner_text("#updateState") and "next time" in page.inner_text("#updateState"), page.inner_text("#updateState")
    assert page.is_visible("#updateBtn") and page.is_checked("#updateBox")
    page.locator("#updateCard").screenshot(path=os.path.join(SHOTS, "phone-update-card.png"))
    page.fill("#voiceSearch", "female soft")
    page.wait_for_timeout(100)
    shown = [li.get_attribute("data-id") for li in page.query_selector_all("#voiceList li:not(.hidden)")]
    assert shown == ["v2", "v4"], shown  # the chosen one stays, plus the match
    page.screenshot(path=os.path.join(SHOTS, "phone-settings.png"), full_page=True)
    page.click("#voiceList li[data-id=v4]")
    page.wait_for_timeout(100)
    assert page.get_attribute("#voiceList li.sel", "data-id") == "v4"
    # Picking a voice of its own leaves the live call on (it speaks in its
    # own voice; the picked one is for the PC's replies and alerts).
    assert page.is_checked("#liveCall")
    # The live-call toggle flipped before Listen: the PC decided who says
    # hello from the toggle as it was (the call about to open, or a clip
    # of its own); the page says hello again with the new value, so that
    # it decides again — else the hello left to a call turned off is never
    # heard.
    t_flip = time.time()
    page.uncheck("#liveCall")
    wait_for(lambda: any((r["body"] or {}).get("live") is False for r in requests_since(t_flip, "/api/hello")), 3, "the toggle flipped before Listen did not say hello again")
    t_flip = time.time()
    page.check("#liveCall")
    wait_for(lambda: any((r["body"] or {}).get("live") is True for r in requests_since(t_flip, "/api/hello")), 3, "the toggle flipped back did not say hello again")
    # Play stats, the last card: sharing is off unless turned on, and says
    # plainly what is shared, that it may be sold, and what never is.
    # Turned on, it posts /api/share {on: true}; what would be shared is
    # JSON with no name in it; "Delete it" posts the delete and turns it off.
    page.locator("#shareCard").scroll_into_view_if_needed()
    assert page.is_visible("#shareCard") and not page.is_checked("#shareBox"), "sharing must start off"
    card = page.inner_text("#shareCard")
    assert "sold to companies that study gamers" in card and "Never shared" in card and "character's name" in card, card
    # For adults only: the toggle waits for "I'm 18 or older", and says so
    # to the PC with the "on".
    assert page.is_disabled("#shareBox"), "sharing could be turned on without the age box"
    t_share = time.time()
    page.check("#adultBox")
    page.check("#shareBox")
    wait_for(lambda: any((r["body"] or {}).get("on") is True and (r["body"] or {}).get("adult") is True for r in requests_since(t_share, "/api/share")), 3, "turning sharing on did not post /api/share {on: true, adult: true}")
    page.click("#shareSee")
    page.wait_for_selector("#shareView:not([hidden])", timeout=3000)
    shown = json.loads(page.inner_text("#shareView"))
    def keys_of(value):
        if isinstance(value, dict): return [k for k, v in value.items() for k in [k] + keys_of(v)]
        if isinstance(value, list): return [k for v in value for k in keys_of(v)]
        return []
    assert shown.get("install_id") and shown.get("sessions"), shown
    assert not [k for k in keys_of(shown) if "name" in k], keys_of(shown)
    assert page.is_checked("#shareBox")
    page.locator("#shareCard").screenshot(path=os.path.join(SHOTS, "phone-share-card.png"))
    # The PC fails the next "Delete it" (a file of it is still there: 500,
    # once): the page says so in its own words, never "Deleted", and the
    # toggle shows what the PC has — still sharing.
    SHARE_FAILS["left"] = 1
    t_fail = time.time()
    page.click("#shareDelete")
    wait_for(lambda: requests_since(t_fail, "/api/share/delete"), 3, "Delete it did not post /api/share/delete")
    wait_for(lambda: page.inner_text("#shareState"), 3, "a delete the PC failed said nothing")
    state = page.inner_text("#shareState")
    assert state.startswith("Not deleted: a file of it is still on the PC") and not state.startswith("Deleted"), state
    wait_for(lambda: page.is_checked("#shareBox"), 3, "after a failed delete the toggle did not show the PC still sharing")
    assert SHARE["on"] and not SHARE_FAILS["left"]
    page.locator("#shareCard").screenshot(path=os.path.join(SHOTS, "phone-share-not-deleted.png"))
    t_delete = time.time()
    page.click("#shareDelete")
    wait_for(lambda: requests_since(t_delete, "/api/share/delete"), 3, "Delete it did not post /api/share/delete")
    wait_for(lambda: page.inner_text("#shareState").startswith("Deleted: sharing is off"), 3, ("the PC's ok did not say Deleted", page.inner_text("#shareState")))
    assert not page.is_checked("#shareBox") and page.is_hidden("#shareView") and not SHARE["on"]
    page.click("#sheetDone")
    page.wait_for_timeout(100)
    assert page.is_hidden("#sheet")
    # The game line opens Details.
    page.click("#strip")
    page.wait_for_timeout(200)
    assert page.is_visible("#paneDetails") and page.is_visible("#game") and page.is_visible("#recBtn")
    # The last sessions, from the PC's stats: a row each.
    wait_for(lambda: len(page.query_selector_all("#sessionsBody tr")) == 2, 3, "the sessions table did not fill")
    assert page.is_hidden("#sessionsEmpty")
    cells = [td.inner_text() for td in page.query_selector_all("#sessionsBody tr:first-child td")]
    assert cells[1:] == ["95", "2", "1", "6"], cells
    # (The levels gained, not the level.)
    assert page.inner_text("#sessionsCard th[data-t=col_levels]") == "Levels up"
    page.locator("#sessionsCard").screenshot(path=os.path.join(SHOTS, "phone-sessions-card.png"))
    assert "6,370 / 6,370" in page.inner_text("#hpVal"), page.inner_text("#hpVal")
    # The workshop card: on, both coders to pick from, what it is doing.
    assert page.is_checked("#workshopBox") and page.is_visible("#workshopBody")
    assert page.input_value("#workshopCoder") == "Claude Code"
    assert page.is_visible("#workshopCoderRow")
    assert "Working: building" in page.inner_text("#workshopState"), page.inner_text("#workshopState")
    assert page.is_disabled("#workshopBuild"), "no second job while one runs"
    page.locator("#workshopCard").screenshot(path=os.path.join(SHOTS, "phone-workshop-card.png"))
    page.screenshot(path=os.path.join(SHOTS, "phone-details.png"), full_page=True)
    # Hebrew (picked on the main screen): right to left, the new words
    # translated.
    page.click("#sheetDone")
    page.select_option("#lang", "he-IL")
    page.wait_for_timeout(300)
    assert page.get_attribute("html", "dir") == "rtl"
    # On a 320 px phone the game line does not fit: in Hebrew too it loses
    # its end, never the level (its text keeps its own direction).
    page.set_viewport_size({"width": 320, "height": 700})
    page.wait_for_timeout(200)
    strip = page.evaluate("""() => {
      const el = document.querySelector("#stripText"), text = el.firstChild, box = el.getBoundingClientRect();
      const range = document.createRange(), at = text.data.indexOf("Lv 152");
      range.setStart(text, at); range.setEnd(text, at + "Lv 152".length);
      const lv = range.getBoundingClientRect();
      return { overflows: el.scrollWidth > el.clientWidth, box: [box.left, box.right], level: [lv.left, lv.right] };
    }""")
    assert strip["overflows"], ("the strip must be too narrow for its text here", strip)
    assert strip["box"][0] - 0.5 <= strip["level"][0] and strip["level"][1] <= strip["box"][1] + 0.5, ("at 320 px in Hebrew the level is cut off", strip)
    page.screenshot(path=os.path.join(SHOTS, "phone-main-320-he.png"), full_page=True)
    page.set_viewport_size({"width": 390, "height": 844})
    page.click("#gear")
    page.wait_for_timeout(200)
    assert page.inner_text("#tabDetails") == "פרטים" and page.inner_text("#sheetDone") == "סיום"
    assert page.inner_text("#shareCard h2") == "סטטיסטיקות משחק" and not page.is_checked("#shareBox")
    assert page.get_attribute("#voiceSearch", "placeholder") == "חפש קול…"
    page.screenshot(path=os.path.join(SHOTS, "phone-settings-he.png"), full_page=True)
    # Without the age box the toggle cannot be turned on — but a share
    # already on (from the PC) shows on and can always be turned off.
    page.uncheck("#adultBox")
    assert page.is_disabled("#shareBox"), "sharing could be turned on without the age box"
    SHARE["on"] = True
    def shown_on():
        page.click("#tabDetails"); page.click("#tabSettings"); page.wait_for_timeout(150)
        return page.is_checked("#shareBox")
    wait_for(shown_on, 6, "a share already on did not show on")
    assert page.is_enabled("#shareBox"), "a share already on could not be turned off"
    t_off = time.time()
    page.uncheck("#shareBox")
    wait_for(lambda: any((r["body"] or {}).get("on") is False for r in requests_since(t_off, "/api/share")), 3, "turning sharing off did not post /api/share {on: false}")
    wait_for(lambda: page.is_disabled("#shareBox"), 3, "once off, the toggle did not wait for the age box again")
    assert not SHARE["on"]
    # The Hebrew words (w32 §2 rows 21–23; the levels gained, not the level).
    assert page.inner_text("#shareCard [data-t=share_adult]") == "גילי 18 ומעלה"
    assert "הקלאס שלך" in page.inner_text("#shareCard") and "המקצוע" not in page.inner_text("#shareCard")
    assert page.text_content("th[data-t=col_levels]") == "+רמות"
    page.click("#shareSee")
    page.wait_for_selector("#shareView:not([hidden])", timeout=3000)
    assert page.inner_text("#shareNote") == "השיתוף כבוי. אם הוא היה דלוק, הסשנים האחרונים שלך היו נראים כך:", page.inner_text("#shareNote")
    page.click("#shareSee")
    # Turned on without the age box (only a race reaches the PC so: forced
    # here): its refusal in the page's Hebrew, not the PC's words.
    page.evaluate("document.querySelector('#shareBox').disabled = false")
    t_race = time.time()
    page.click("#shareBox")
    wait_for(lambda: any((r["body"] or {}).get("adult") is False for r in requests_since(t_race, "/api/share")), 3, "the forced turn-on was not posted")
    wait_for(lambda: page.inner_text("#shareState") == "קודם צריך לסמן „גילי 18 ומעלה”.", 3, ("the age box's refusal not in the page's Hebrew", page.inner_text("#shareState")))
    wait_for(lambda: not page.is_checked("#shareBox") and page.is_disabled("#shareBox"), 3, "a refused turn-on left the toggle on")
    assert not SHARE["on"]
    # A delete the PC fails, in Hebrew: said so, never "נמחק"; then done.
    SHARE["on"] = True
    wait_for(shown_on, 6, "a share already on did not show on")
    SHARE_FAILS["left"] = 1
    page.click("#shareDelete")
    wait_for(lambda: page.inner_text("#shareState").startswith("לא נמחק:"), 3, ("a failed delete not said in Hebrew", page.inner_text("#shareState")))
    wait_for(lambda: page.is_checked("#shareBox"), 3, "after a failed delete the toggle did not show the PC still sharing")
    page.locator("#shareCard").screenshot(path=os.path.join(SHOTS, "phone-share-not-deleted-he.png"))
    page.click("#shareDelete")
    wait_for(lambda: page.inner_text("#shareState").startswith("נמחק:"), 3, ("the PC's ok did not say נמחק", page.inner_text("#shareState")))
    assert not page.is_checked("#shareBox") and not SHARE["on"]
    # As this build is: sharing not offered (no approved basis yet) — the
    # card says so, in Hebrew here, and has no switch, age box or export.
    SHARE["available"] = False
    def unavailable_shown():
        page.click("#tabDetails"); page.click("#tabSettings"); page.wait_for_timeout(150)
        return page.is_visible("#shareUnavailable")
    wait_for(unavailable_shown, 6, "a PC that does not offer sharing still showed the switch")
    assert page.is_hidden("#shareControls") and page.is_hidden("#shareBox") and page.is_hidden("#adultBox")
    assert "בדיקת זכויות" in page.inner_text("#shareUnavailable"), page.inner_text("#shareUnavailable")
    page.locator("#shareCard").screenshot(path=os.path.join(SHOTS, "phone-share-unavailable-he.png"))
    SHARE["available"] = True
    page.close()

def listening_page(browser):
    """A page listening with the stand-in recognizer (no live call), told
    to wait a long moment after the words stop before it takes a sentence
    as said (so that only the test's own events send one)."""
    STATUS["live"] = False
    STATUS["settle_ms"] = 1500
    reset_pc()
    page = browser.new_page(viewport={"width": 390, "height": 844})
    watch(page)
    page.add_init_script(FAKES)
    page.goto(f"http://127.0.0.1:{port}/?k=test")
    page.wait_for_timeout(800)
    page.click("#listen")
    started = time.time()
    # (The tap plays a silent clip to unlock the phone's sound, which holds
    # recognition for a moment: wait for it to be listening again.)
    page.wait_for_timeout(1200)
    wait_for(lambda: page.evaluate("!!window.__rec && window.__rec.running"), 5, "recognition did not start")
    return page, started

def recognition_checks(browser):
    page, _ = listening_page(browser)
    with LOCK: CLIP_SECONDS["1"] = 1.5
    # The words so far go to the PC as they come (throttled); the sentence
    # follows. A throttled post must not land after it: the PC would take
    # the sentence's own words for more talking.
    t0 = time.time()
    page.evaluate('window.__rec.result(0, "am I talking to", false)')
    page.wait_for_timeout(30)
    page.evaluate('window.__rec.result(0, "am I talking to right", false)')
    page.wait_for_timeout(20)
    page.evaluate('window.__rec.result(0, "am I talking to right", true)')
    page.wait_for_timeout(500)
    posts = [(r["path"], r["body"]["text"]) for r in requests_since(t0) if r["path"] in ("/api/hearing", "/api/heard")]
    assert ("/api/heard", "am I talking to right") in posts, posts
    heard_at = posts.index(("/api/heard", "am I talking to right"))
    assert all(p != "/api/hearing" for p, _ in posts[heard_at + 1:]), posts
    # A sentence being written when a clip starts is sent as it is before
    # recognition stops, and recognition is stopped while the clip plays.
    t1 = time.time()
    page.evaluate('window.__rec.result(1, "where am I", false)')
    page.wait_for_timeout(100)
    with LOCK: STATE["clip"] = 1
    wait_for(lambda: any(r["body"].get("text") == "where am I" for r in requests_since(t1, "/api/heard")), 4, "the sentence cut off by the clip was not sent")
    wait_for(lambda: page.evaluate("window.__rec.aborted > 0"), 2, "recognition kept running under the clip")
    assert page.evaluate("!window.msVoice.paused"), "the clip plays"
    assert not requests_since(t1, "/api/interrupt"), "a silent microphone is no talk-over"
    # The PC is started again (a new boot): the page says hello again and
    # takes its clips from the start.
    page.wait_for_timeout(300)
    t2 = time.time()
    with LOCK: STATE.update({"boot": "b2", "clip": 0, "messages": []})
    wait_for(lambda: requests_since(t2, "/api/hello"), 3, "no hello to the restarted PC")
    hello = requests_since(t2, "/api/hello")[0]["body"]
    assert hello.get("lang") and "agent" in hello, hello
    wait_for(lambda: any(r["query"].get("since") == "0" and r["query"].get("clip") == "0" for r in requests_since(t2, "/api/state")), 3, "the poll did not start over")
    with LOCK: STATE["clip"] = 1
    wait_for(lambda: any(r["query"].get("seq") == "1" for r in requests_since(t2, "/api/clip")), 4, "the restarted PC's first clip never played")
    page.close()

def loudness_checks(p):
    """The microphone fed from a file: quiet, loud, quiet, loud."""
    mic = os.path.join(SHOTS, "mic-loud.wav")
    with open(mic, "wb") as f:
        f.write(wav(48000, pcm(5, 48000) + pcm(8, 48000, 24000) + pcm(5, 48000) + pcm(40, 48000, 24000)))
    browser = p.chromium.launch(args=browser_args(mic))
    page, mic_started = listening_page(browser)
    def at(seconds):
        """Wait until the microphone's file is this far."""
        wait = mic_started + seconds - time.time()
        if wait > 0: time.sleep(wait)
    def interrupts(t): return len(requests_since(t, "/api/interrupt"))
    # A clip starts while the room is quiet; the loud sound comes a few
    # seconds into it: the clip pauses and the PC is told, and with no cut
    # from the PC (no words followed) it plays on from where it was, and
    # that loudness does not trip it again.
    t0 = time.time()
    at(1.5)
    with LOCK: STATE["clip"] = 1
    wait_for(lambda: page.evaluate("!window.msVoice.paused"), 4, "the clip did not start")
    wait_for(lambda: interrupts(t0), 9, "the loud sound never paused the clip")
    assert page.evaluate("window.msVoice.paused"), "the clip pauses while the PC waits for words"
    wait_for(lambda: page.evaluate("!window.msVoice.paused"), 3, "the clip did not go on after the PC's silence")
    assert interrupts(t0) == 1, "the same loudness tripped the clip again"
    wait_for(lambda: page.evaluate("window.msVoice.ended"), 10, "the clip did not play to its end")
    assert interrupts(t0) == 1
    # The next clip in the next quiet moment, and this time the PC answers
    # the pause with a cut (the player's words followed): stopped for good.
    t1 = time.time()
    at(14)
    with LOCK: STATE["clip"] = 2
    wait_for(lambda: page.evaluate("!window.msVoice.paused"), 4, "the second clip did not start")
    wait_for(lambda: interrupts(t1), 9, "the loud sound never paused the second clip")
    with LOCK: STATE["cut"] = 1
    wait_for(lambda: page.evaluate("window.msVoice.paused && !window.msVoice.getAttribute('src')"), 3, "the PC's cut did not stop the clip")
    page.wait_for_timeout(PAUSE_SLACK)
    assert page.evaluate("window.msVoice.paused && !window.msVoice.getAttribute('src')"), "a cut clip must not go on"
    assert interrupts(t1) == 1
    # A clip starting in a steady sound: its first moments set its floor,
    # and it plays through.
    t2 = time.time()
    with LOCK: STATE["clip"] = 3
    wait_for(lambda: page.evaluate("!window.msVoice.paused"), 4, "the third clip did not start")
    wait_for(lambda: page.evaluate("window.msVoice.ended"), 12, "the third clip did not play through the steady sound")
    assert not requests_since(t2, "/api/interrupt"), "a steady sound from the clip's start is no talk-over"
    page.close()
    browser.close()

# What the dog was told to react to, in order.
DOG_SPY = """
window.__reacts = [];
const hookDog = setInterval(() => {
  const d = window.msDog;
  if (!d) return;
  clearInterval(hookDog);
  const react = d.react;
  d.react = (kind, x) => { window.__reacts.push(kind); return react.call(d, kind, x); };
}, 5);
"""

def live_checks(browser):
    STATUS["live"] = True
    STATUS["attitude"] = "savage"
    STATUS["new_player"] = True
    reset_pc()
    with LOCK: STATE["voice_on"] = "phone"
    page = browser.new_page(viewport={"width": 390, "height": 844})
    watch(page)
    page.add_init_script(FAKES)
    page.add_init_script(DOG_SPY)
    page.goto(f"http://127.0.0.1:{port}/?k=test")
    page.wait_for_timeout(800)
    t0 = time.time()
    page.click("#listen")
    wait_for(lambda: any(r["body"] == {"live": True} for r in requests_since(t0, "/api/mode")), 5, "the call did not open")
    # What the page sent down the call's data channel, and the call's
    # events fed to it (the stand-in call answers nothing on its own).
    def sent(kind): return [e for e in page.evaluate("window.__dcSent") if e["type"] == kind]
    def items_with(text): return [e for e in sent("conversation.item.create") if text in json.dumps(e)]
    def event(ev): page.evaluate("(ev) => window.__dc.onmessage({ data: JSON.stringify(ev) })", ev)
    def line(id, kind, text, fact=None, urgent=False):
        m = {"id": id, "t": 100.0 + id, "kind": kind, "text": text, "speak": True}
        if fact: m["fact"] = fact
        if urgent: m["urgent"] = True
        with LOCK: STATE["messages"].append(m)
    # The greeting asks for a turn; the call takes it and is done. To a
    # player the PC does not know (status.new_player) it says what it does
    # too, as the clip hello's terms would.
    wait_for(lambda: len(sent("response.create")) == 1, 4, "the greeting asked for no turn")
    greeting = items_with("opened the call")
    assert len(greeting) == 1 and "say you'll shout if their HP drops and they can ask you anything" in greeting[0]["item"]["content"][0]["text"], greeting
    event({"type": "response.created"}); event({"type": "response.done", "response": {"output": []}})
    # On the call, one of MapleSyrup's own lines (the answer to a button) is
    # handed to the call to say, never to the phone's own voice.
    line(1, "reply", "Marked. That's mark 1.")
    wait_for(lambda: items_with("Marked"), 4, "the call was not handed the line")
    assert page.evaluate("window.__spoken") == [], page.evaluate("window.__spoken")
    wait_for(lambda: len(sent("response.create")) == 2, 2, "no turn asked for the line")
    event({"type": "response.created"}); event({"type": "response.done", "response": {"output": []}})
    # A watcher line goes with the reading the PC sent behind it and the
    # game as read right now (the call's own picture is older), as one
    # message not from the player, and one turn is asked for after it.
    line(2, "warning", "Back off, you're getting shredded.", "HP 11% (read 0 s ago), MP about 40% (estimated 0 s ago)")
    wait_for(lambda: items_with("shredded"), 4, "the call was not handed the warning")
    text = items_with("shredded")[-1]["item"]["content"][0]["text"]
    assert "not the player" in text and "HP 11% (read 0 s ago)" in text, text
    assert "The game right now" in text and "Character: level 152." in text, text
    assert len(sent("response.create")) == 3, sent("response.create")
    # The dog barks at a warning (as the voice shouts it).
    assert page.evaluate("window.__reacts") == ["alert"], page.evaluate("window.__reacts")
    # A response under way (the call answering the player): the next line
    # waits, and no second turn is asked for until the response is done.
    event({"type": "response.created"})
    line(3, "warning", "Pot now, HP's at 9%.", "HP 9.0% (read 0 s ago)")
    page.wait_for_timeout(1500)
    assert not items_with("Pot now") and len(sent("response.create")) == 3, "a turn was asked for while one was under way"
    event({"type": "response.done", "response": {"output": []}})
    wait_for(lambda: items_with("Pot now") and len(sent("response.create")) == 4, 4, "the line did not go once the response was done")
    assert items_with("Pot now")[-1]["item"]["content"][0]["text"].count("Its reading") == 1
    # The call talks on for a while. A warning that waited behind its voice
    # that long is not news any more: not said when the talking stops, and
    # the PC is told it was not. The one line that matters (a level-up, a
    # death: urgent) waits for the voice to stop, then is said however late,
    # and says how late.
    t_late = time.time()
    event({"type": "response.created"}); event({"type": "output_audio_buffer.started"})
    line(4, "warning", "Move, you're melting.", "HP 40% (read 0 s ago)")
    line(5, "alert", "Level 153! Nice.", urgent=True)
    page.wait_for_timeout(6500)
    assert not items_with("Level 153") and len(sent("response.create")) == 4, "a line was said over the call's own voice"
    event({"type": "output_audio_buffer.stopped"}); event({"type": "response.done", "response": {"output": []}})
    wait_for(lambda: items_with("Level 153"), 4, "the level-up was not said once the talking stopped")
    text = items_with("Level 153")[-1]["item"]["content"][0]["text"]
    assert "melting" not in text, text
    assert re.search(r"not the player, \d+ s ago: Level 153", text), text
    assert len(sent("response.create")) == 5, sent("response.create")
    # News is told, not barked: the dog takes a level-up and a death from
    # the game itself; the three warnings so far were three barks.
    assert page.evaluate("window.__reacts") == ["alert"] * 3, page.evaluate("window.__reacts")
    # A warning's row (a beating, a low bar) is red, news's row (a level-up)
    # amber: the eye tells a shout from news as the ear does.
    rows = page.evaluate("""() => {
      const look = (sel) => { const el = document.querySelector(sel); return el && getComputedStyle(el).borderColor + " " + getComputedStyle(el).color; };
      return { warning: look("#log li.warning"), alert: look("#log li.alert") };
    }""")
    assert rows["warning"] and rows["alert"] and rows["warning"] != rows["alert"], rows
    def rgb(text): return [int(x) for x in re.search(r"rgb\((\d+), (\d+), (\d+)\)\s*$", text).groups()]
    r, g, b = rgb(rows["warning"])
    assert r > g and g - b < 30, ("a warning's text must be red", rows)
    r, g, b = rgb(rows["alert"])
    assert r > g > b and g - b > 30, ("news's text must be amber", rows)
    wait_for(lambda: any(r["body"] == {"what": "dropped", "text": "Move, you're melting."} for r in requests_since(t_late, "/api/turn")), 2, "the PC was not told of the line that was not said")
    assert not any((r["body"] or {}).get("text") == "Level 153! Nice." for r in requests_since(t_late, "/api/turn")), "the level-up was reported as not said"
    event({"type": "response.created"}); event({"type": "response.done", "response": {"output": []}})
    # The attitude changed on the PC (the picker here, or the player
    # objecting to the tone): the call gets its instructions again and goes
    # on, not started over.
    t1 = time.time()
    STATUS["attitude"] = "friendly"
    wait_for(lambda: sent("session.update"), 4, "the call was not retuned to the new attitude")
    update = sent("session.update")[-1]["session"]
    assert update == {"type": "realtime", "instructions": INSTRUCTIONS}, update
    assert requests_since(t1, "/api/instructions") and not requests_since(t1, "/api/live"), "the call was started over"
    page.wait_for_timeout(600)
    assert len(sent("session.update")) == 1, "retuned more than once"
    # The player speaks Hebrew on the call (the phone is set to en-US): the
    # recogniser clip mode uses follows them — not on one short word (an
    # English speaker's "תודה"), on two Hebrew sentences in a row or one
    # long one — said once on the screen as it switches, and shown beside
    # the picker while it differs from it; two English sentences in a row
    # switch it back.
    def heard(text):
        event({"type": "conversation.item.input_audio_transcription.completed", "transcript": text})
        page.wait_for_timeout(200)
    def hearing(): return page.evaluate("(() => { const b = document.querySelector('#hearing'); return b.hidden ? null : b.textContent; })()")
    def rec_lang(): return page.evaluate("localStorage.getItem('ms.recLang')")
    def told(): return page.evaluate("""[...document.querySelectorAll("#log li.info")].filter((li) => li.textContent === "Hearing Hebrew now.").length""")
    heard("תודה")
    assert (page.inner_text("#note"), hearing(), rec_lang() or "") == ("", None, ""), ("one Hebrew word switched the recogniser", page.inner_text("#note"), hearing(), rec_lang())
    heard("מה המצב")
    assert page.inner_text("#note") == "Hearing Hebrew now.", page.inner_text("#note")
    assert (hearing(), rec_lang(), told()) == ("Hearing: עברית ×", "he-IL", 1), (hearing(), rec_lang(), told())
    heard("ok what's my hp")
    assert hearing() == "Hearing: עברית ×", "one English sentence switched it back"
    heard("and my mp")
    assert (hearing(), rec_lang() or "", page.inner_text("#note")) == (None, "", ""), (hearing(), rec_lang(), page.inner_text("#note"))
    heard("מה הרמה שלי עכשיו")
    assert (hearing(), rec_lang(), told()) == ("Hearing: עברית ×", "he-IL", 2), (hearing(), rec_lang(), told())
    heard("ואיפה אני")
    assert told() == 2, "a second Hebrew sentence said it again"
    t_clip = time.time()
    page.click("#gear"); page.uncheck("#liveCall"); page.click("#sheetDone")
    wait_for(lambda: any(r["body"] == {"live": False} for r in requests_since(t_clip, "/api/mode")), 4, "the call did not give way to clip mode")
    wait_for(lambda: page.evaluate("!!window.__rec && window.__rec.running"), 5, "recognition did not start in clip mode")
    assert page.evaluate("window.__rec.lang") == "he-IL", page.evaluate("window.__rec.lang")
    # Picked by hand, the picker wins again.
    page.select_option("#lang", "en-US")
    wait_for(lambda: page.evaluate("!!window.__rec && window.__rec.running && window.__rec.lang === 'en-US'"), 5, "the picker did not take the recogniser back")
    assert hearing() is None and not rec_lang(), (hearing(), rec_lang())
    # The player asked the PC out loud ("talk to me in Hebrew"): its status
    # asks the page to take Hebrew, by number — taken as if picked in the
    # picker (the picker, right to left, the recogniser, kept, the PC told)
    # once: the same number on the next polls is nothing, even after the
    # picker is changed by hand; the next number is taken again.
    def rec_on(lang): return page.evaluate(f"!!window.__rec && window.__rec.running && window.__rec.lang === '{lang}'")
    t_lang = time.time()
    STATUS["lang_request"] = {"lang": "he-IL", "seq": 1}
    wait_for(lambda: rec_on("he-IL"), 5, "the language asked for did not take the recogniser")
    assert page.input_value("#lang") == "he-IL", page.input_value("#lang")
    assert page.get_attribute("html", "dir") == "rtl"
    assert page.evaluate("localStorage.getItem('ms.lang')") == "he-IL"
    page.wait_for_timeout(1500)
    assert [r["body"] for r in requests_since(t_lang, "/api/lang")] == [{"lang": "he-IL"}], [r["body"] for r in requests_since(t_lang, "/api/lang")]
    page.select_option("#lang", "en-US")
    wait_for(lambda: rec_on("en-US"), 5, "the picker did not take the recogniser back")
    page.wait_for_timeout(1500)
    assert page.input_value("#lang") == "en-US", "the same request was taken twice"
    t_lang = time.time()
    STATUS["lang_request"] = {"lang": "he-IL", "seq": 2}
    wait_for(lambda: rec_on("he-IL"), 5, "the next request was not taken")
    STATUS["lang_request"] = {"lang": "en-US", "seq": 3}
    wait_for(lambda: rec_on("en-US") and page.get_attribute("html", "dir") == "ltr", 5, "English asked for did not take the page back")
    assert [r["body"]["lang"] for r in requests_since(t_lang, "/api/lang")] == ["he-IL", "en-US"]
    STATUS.pop("lang_request")
    # Back on a call for the rest.
    t_back = time.time()
    page.click("#gear"); page.check("#liveCall"); page.click("#sheetDone")
    wait_for(lambda: any(r["body"] == {"live": True} for r in requests_since(t_back, "/api/mode")), 5, "the call did not reopen")
    # A glance at another app (the page hidden a moment) is not the end of
    # the call: no word to the PC (the line waiting for the call's gap went
    # nowhere, and a death meanwhile was shown and never said); a lock is
    # the PC's to notice, when the phone stops asking. Back in front, no word
    # while the PC has the call on (status.on_call).
    def visibility(state):
        page.evaluate("""(state) => {
          Object.defineProperty(document, "visibilityState", { get: () => state, configurable: true });
          document.dispatchEvent(new Event("visibilitychange"));
        }""", state)
    STATUS["on_call"] = True
    t_hide = time.time()
    visibility("hidden")
    page.wait_for_timeout(1000)
    assert not requests_since(t_hide, "/api/mode"), ("a glance at another app told the PC about the call", [r["body"] for r in requests_since(t_hide, "/api/mode")])
    t_show = time.time()
    visibility("visible")
    page.wait_for_timeout(1000)
    assert not requests_since(t_show, "/api/mode"), ("the page back in front told a PC that has the call on", [r["body"] for r in requests_since(t_show, "/api/mode")])
    # The PC took the call as lost while the page's is on (the link down a
    # while with the page in front; another page's hello): the page tells
    # it the call is on — once, not every poll.
    t_heal = time.time()
    STATUS["on_call"] = False
    # (Within 5 s of the call's own "on": up to that long.)
    wait_for(lambda: requests_since(t_heal, "/api/mode"), 6, "the page did not tell a PC that lost its call that the call is on")
    page.wait_for_timeout(2500)
    modes = [r["body"] for r in requests_since(t_heal, "/api/mode")]
    assert modes == [{"live": True}], modes
    STATUS["on_call"] = True
    # The PC started again while the call is on: it hears of the call —
    # after the hello (to the PC a page that says hello has no call until
    # it says so).
    t2 = time.time()
    with LOCK: STATE.update({"boot": "b2", "clip": 0, "messages": []})
    wait_for(lambda: any(r["body"] == {"live": True} for r in requests_since(t2, "/api/mode")), 4, "the restarted PC was not told of the call")
    hellos = requests_since(t2, "/api/hello")
    assert hellos, "no hello to the restarted PC"
    modes = [r for r in requests_since(t2, "/api/mode") if r["body"] == {"live": True}]
    assert modes[0]["t"] >= hellos[0]["t"], "the call was reported before the hello"
    # The hello carries the live-call toggle: the PC leaves the hello to
    # the call when one is coming, and says it itself when not.
    assert hellos[0]["body"].get("live") is True, hellos[0]["body"]
    STATUS["attitude"] = "savage"
    # The page leaving (closed, navigated away): its last word is that the
    # call is off — a beacon, since nothing else runs by then.
    t_gone = time.time()
    page.goto("about:blank")
    wait_for(lambda: any(r["body"] == {"live": False} for r in requests_since(t_gone, "/api/mode")), 3, "the page leaving did not tell the PC its call is off")
    page.close()
    # A page reloaded mid-visit: the PC says no second hello, and the call
    # it opens says none either (status.call_greets is false).
    STATUS["call_greets"] = False
    reset_pc()
    with LOCK: STATE["voice_on"] = "phone"
    page = browser.new_page(viewport={"width": 390, "height": 844})
    watch(page)
    page.add_init_script(FAKES)
    page.goto(f"http://127.0.0.1:{port}/?k=test")
    page.wait_for_timeout(800)
    t3 = time.time()
    page.click("#listen")
    wait_for(lambda: any(r["body"] == {"live": True} for r in requests_since(t3, "/api/mode")), 5, "the reloaded page's call did not open")
    page.wait_for_timeout(600)
    sent_now = page.evaluate("window.__dcSent")
    assert not [e for e in sent_now if e["type"] == "response.create" or "opened the call" in json.dumps(e)], "a reloaded page's call said hello again"
    STATUS["call_greets"] = True
    page.close()
    # The next evening, in clip mode, the recogniser still in Hebrew from a
    # call: shown beside the picker (it was not: every English sentence came
    # out as Hebrew word salad, the picker saying English); its × goes back
    # to the picker's, for good. (On a 320 px phone, within the screen.)
    page = browser.new_page(viewport={"width": 320, "height": 700})
    watch(page)
    page.add_init_script(FAKES)
    page.add_init_script("try { if (!sessionStorage.getItem('w')) { sessionStorage.setItem('w', '1'); localStorage.setItem('ms.recLang', 'he-IL'); localStorage.setItem('ms.live', '0'); localStorage.setItem('ms.lang', 'en-US'); } } catch (e) {}")
    page.goto(f"http://127.0.0.1:{port}/?k=test")
    page.wait_for_timeout(800)
    assert hearing() == "Hearing: עברית ×", hearing()
    assert page.evaluate("document.documentElement.scrollWidth <= innerWidth"), "the badge widened the page"
    page.screenshot(path=os.path.join(SHOTS, "phone-hearing-320.png"))
    page.click("#listen")
    wait_for(lambda: page.evaluate("!!window.__rec && window.__rec.running"), 5, "recognition did not start")
    assert page.evaluate("window.__rec.lang") == "he-IL", page.evaluate("window.__rec.lang")
    page.click("#hearing")
    wait_for(lambda: page.evaluate("!!window.__rec && window.__rec.running && window.__rec.lang === 'en-US'"), 5, "the × did not take the recogniser back to the picker's")
    assert hearing() is None and not rec_lang(), (hearing(), rec_lang())
    page.reload()
    page.wait_for_timeout(800)
    assert hearing() is None, "the × was not for good"
    page.close()
    # A player the PC knows: the call's hello says nothing of what it does.
    STATUS["new_player"] = False
    reset_pc()
    with LOCK: STATE["voice_on"] = "phone"
    page = browser.new_page(viewport={"width": 390, "height": 844})
    watch(page)
    page.add_init_script(FAKES)
    page.goto(f"http://127.0.0.1:{port}/?k=test")
    page.wait_for_timeout(800)
    page.click("#listen")
    wait_for(lambda: [e for e in page.evaluate("window.__dcSent") if "opened the call" in json.dumps(e)], 5, "the known player's call did not greet")
    greeting = [e for e in page.evaluate("window.__dcSent") if "opened the call" in json.dumps(e)]
    assert len(greeting) == 1 and "shout" not in json.dumps(greeting[0]), greeting
    page.close()
    # A call that fails to open at the tap (no key, OpenAI down): the hello
    # the PC left to it is handed back — the page says hello again with the
    # call off, so the PC says its own now, not a minute after the page
    # opened. A page the PC greeted already (call_greets false): no hello.
    LIVE["opens"] = False
    for greets in (True, False):
        STATUS["call_greets"] = greets
        reset_pc()
        with LOCK: STATE["voice_on"] = "phone"
        page = browser.new_page(viewport={"width": 390, "height": 844})
        watch(page)
        page.add_init_script(FAKES)
        page.goto(f"http://127.0.0.1:{port}/?k=test")
        page.wait_for_timeout(800)
        t4 = time.time()
        page.click("#listen")
        def handed_back(): return [r for r in requests_since(t4, "/api/hello") if (r["body"] or {}).get("live") is False]
        wait_for(lambda: page.evaluate("!!window.__rec && window.__rec.running"), 5, "the call that failed did not give way to the regular mode")
        if greets:
            wait_for(handed_back, 3, "the call that failed to open did not hand its hello back")
        else:
            page.wait_for_timeout(1000)
            assert not handed_back(), "a page greeted already said hello again when its call failed"
        page.close()
    LIVE["opens"] = True
    STATUS["call_greets"] = True

# What the phone's voice element played, in order, from its `playing`
# events: the silent clip a tap unlocks sound with ("unlock"), and the
# PC's clips by number ("seq 1").
PLAYED = """
window.__played = [];
const hook = setInterval(() => {
  const v = window.msVoice;
  if (!v) return;
  clearInterval(hook);
  v.addEventListener("playing", () => {
    const s = (v.currentSrc || "").replace(/.*seq=/, "seq ").replace(/^blob:.*/, "unlock");
    if (window.__played[window.__played.length - 1] !== s) window.__played.push(s);
  });
}, 5);
"""

def hello_checks(p):
    """A phone will not play a clip before a tap (the browser's own autoplay
    policy, as on iOS and Android; every other scenario here allows it). In
    clip mode the PC's hello is made at page load, a second or so before
    the player taps Listen: the clip is kept for the tap — not lost, and not
    fetched again every poll meanwhile — and plays after the tap's silent
    unlocking clip, before the next clip; whether the tap comes three
    seconds or a moment after the clip."""
    STATUS["live"] = False
    browser = p.chromium.launch(args=["--use-fake-ui-for-media-stream", "--use-fake-device-for-media-stream"])
    for tap_after_ms in (3000, 300):
        reset_pc()
        with LOCK: CLIP_SECONDS["default"] = 1
        page = browser.new_page(viewport={"width": 390, "height": 844})
        watch(page)
        page.add_init_script(FAKES)
        page.add_init_script(PLAYED)
        t0 = time.time()
        page.goto(f"http://127.0.0.1:{port}/?k=test")
        wait_for(lambda: requests_since(t0, "/api/hello"), 5, "no hello")
        # The PC's hello clip is ready about a second after the hello.
        page.wait_for_timeout(1000)
        with LOCK: STATE["clip"] = 1
        page.wait_for_timeout(tap_after_ms)
        fetched = [r["query"].get("seq") for r in requests_since(t0, "/api/clip")]
        assert fetched == ["1"], f"the hello clip was fetched {len(fetched)} times before the tap: {fetched}"
        assert page.evaluate("window.__played") == [], "a clip played before any tap"
        page.click("#listen")
        # (The tap's silent clip, then the hello, a second long.)
        page.wait_for_timeout(2500)
        # The next clip, after the tap, plays as any would.
        with LOCK: STATE["clip"] = 2
        wait_for(lambda: "seq 2" in page.evaluate("window.__played"), 5, "the clip after the tap did not play")
        played = page.evaluate("window.__played")
        assert played == ["unlock", "seq 1", "seq 2"], f"tap {tap_after_ms} ms after the hello clip: played {played}"
        fetched = [r["query"].get("seq") for r in requests_since(t0, "/api/clip")]
        assert fetched.count("1") <= 2 and fetched.count("2") == 1, fetched
        page.close()
    # Everything made before the tap waits for it — but not as news when it
    # is stale: the hello and the terms play; a warning made longer before
    # than the call's own rule (6 s) does not, nor news over a minute old;
    # and of what is left, far behind, only the latest. (`made`: each clip's
    # kind and how many seconds before the tap it was made.)
    def before_the_tap(made, expected, label):
        reset_pc()
        with LOCK: CLIP_SECONDS["default"] = 1
        page = browser.new_page(viewport={"width": 390, "height": 844})
        watch(page)
        page.add_init_script(FAKES)
        page.add_init_script(PLAYED)
        t0 = time.time()
        page.goto(f"http://127.0.0.1:{port}/?k=test")
        wait_for(lambda: requests_since(t0, "/api/hello"), 5, "no hello")
        page.wait_for_timeout(1000)
        tap = time.time() + 1
        with LOCK:
            for seq, (kind, ago) in enumerate(made, 1):
                STATE["clips"][seq] = (kind, tap - ago)
            STATE["clip"] = len(made)
        page.wait_for_timeout(1000)
        page.click("#listen")
        wait_for(lambda: page.evaluate("window.__played").count("seq " + str(expected[-1])) and page.evaluate("window.msVoice.ended"), 4 + 1.5 * len(expected), f"{label}: played {page.evaluate('window.__played')}")
        page.wait_for_timeout(500)
        played = page.evaluate("window.__played")
        assert played == ["unlock"] + [f"seq {n}" for n in expected], f"{label}: played {played}"
        page.close()
    before_the_tap([("info", 12), ("info", 11), ("warning", 10)], [1, 2], "the hello, the terms and a warning made 10 s before the tap")
    before_the_tap([("info", 40), ("info", 39), ("warning", 30), ("alert", 90), ("alert", 20), ("warning", 0)], [1, 2, 5, 6],
                   "six clips before the tap, the hello first")
    browser.close()

def browser_args(mic_file):
    """A browser with a microphone fed from `mic_file`, and clips allowed to
    play without a tap."""
    return ["--use-fake-ui-for-media-stream", "--use-fake-device-for-media-stream",
            f"--use-file-for-fake-audio-capture={mic_file}", "--autoplay-policy=no-user-gesture-required"]

chosen = set(sys.argv[1:]) or {"ui", "recognition", "live", "loudness", "hello"}
with sync_playwright() as p:
    quiet = os.path.join(SHOTS, "mic-quiet.wav")
    with open(quiet, "wb") as f:
        f.write(wav(48000, pcm(60, 48000)))
    browser = p.chromium.launch(args=browser_args(quiet))
    if "ui" in chosen: ui_checks(browser)
    if "recognition" in chosen: recognition_checks(browser)
    if "live" in chosen: live_checks(browser)
    browser.close()
    if "loudness" in chosen: loudness_checks(p)
    if "hello" in chosen: hello_checks(p)
srv.shutdown()
if errors:
    print("console errors:", *errors, sep="\n  "); sys.exit(1)
print("phone UI: ok")
