"""The phone page against a stand-in PC, in a headless browser: no console
errors, the main screen shows only the essentials, the gear opens Settings,
the voice search narrows the list, the game line opens Details, Hebrew is
right to left; then the turn-taking, with a stand-in speech recognizer, a
stand-in call and a microphone fed from a file: the words so far never
land after the sentence, a sentence cut off by a clip is still sent, a
loud sound over a clip pauses it until the PC's word, a PC started again
is greeted again and its clips play, and on a call MapleSyrup's own lines
are said by the call, never by the phone's own voice: handed over with the
reading behind them and the game as read now, never while the call is
answering (one turn asked for at a time), dropped once stale, and a change
of attitude retunes the call in place. Needs `pip install playwright &&
playwright install chromium`; run from the repository root: `python3
tools/phone_ui_check.py` (or with some of `ui`, `recognition`, `live`,
`loudness` to run those alone; PHONE_PAGE=path checks another copy of the
page, to see a check fail against the page as it was). Screenshots land in
`target/phone-ui/`."""
import json, os, struct, sys, threading, time, http.server, socketserver
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
    "live": True, "recording": {"state": "off"}, "attitude": "savage",
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
# request the page made.
STATE = {"boot": "b1", "clip": 0, "cut": 0, "messages": [], "voice_on": "both"}
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
                body = {"status": STATUS, "mic": {"live": False, "level": 0, "speaking": False}, "voice_on": STATE["voice_on"],
                        "messages": messages, "last_id": len(STATE["messages"]), "uptime": 100.0, "boot": STATE["boot"], "clip": STATE["clip"], "cut": STATE["cut"]}
            self._send(200, json.dumps(body).encode(), "application/json")
        elif path == "/api/clip":
            self._log()
            with LOCK: seconds = CLIP_SECONDS.get(q.get("seq"), CLIP_SECONDS["default"])
            self._send(200, wav(8000, pcm(seconds, 8000)), "audio/wav")
        elif path == "/api/mouth": self._log(); self._send(200, b'{"step_ms": 40, "levels": []}', "application/json")
        elif path == "/api/eyes": self._log(); self._send(200, json.dumps({"snapshot": EYES, "image": None}).encode(), "application/json")
        elif path == "/api/instructions": self._log(); self._send(200, json.dumps({"instructions": INSTRUCTIONS, "attitude": "friendly"}).encode(), "application/json")
        else: self._log(); self._send(404, b"{}", "application/json")
    def do_POST(self):
        n = int(self.headers.get("Content-Length") or 0); raw = self.rfile.read(n)
        body = None
        try: body = json.loads(raw)
        except Exception: pass
        self._log(body)
        path = urlparse(self.path).path
        if path == "/api/live": self._send(200, json.dumps({"key": "ek_test", "url": f"http://127.0.0.1:{port}/sdp", "hint": "", "api": "ga", "attitude": "savage"}).encode(), "application/json")
        elif path == "/sdp": self._send(200, b"v=0\r\no=- 1 1 IN IP4 127.0.0.1\r\n", "application/sdp")
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
    page.on("console", lambda m: errors.append(m.text) if m.type == "error" else None)
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
        STATE.update({"boot": "b1", "clip": 0, "cut": 0, "messages": []})
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
    assert not page.is_visible("#recBtn") and not page.is_visible("#lang")
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
    page.click("#sheetDone")
    page.wait_for_timeout(100)
    assert page.is_hidden("#sheet")
    # The game line opens Details.
    page.click("#strip")
    page.wait_for_timeout(200)
    assert page.is_visible("#paneDetails") and page.is_visible("#game") and page.is_visible("#recBtn")
    assert "6,370 / 6,370" in page.inner_text("#hpVal"), page.inner_text("#hpVal")
    # The workshop card: on, both coders to pick from, what it is doing.
    assert page.is_checked("#workshopBox") and page.is_visible("#workshopBody")
    assert page.input_value("#workshopCoder") == "Claude Code"
    assert page.is_visible("#workshopCoderRow")
    assert "Working: building" in page.inner_text("#workshopState"), page.inner_text("#workshopState")
    assert page.is_disabled("#workshopBuild"), "no second job while one runs"
    page.locator("#workshopCard").screenshot(path=os.path.join(SHOTS, "phone-workshop-card.png"))
    page.screenshot(path=os.path.join(SHOTS, "phone-details.png"), full_page=True)
    # Hebrew: right to left, the new words translated.
    page.click("#tabSettings")
    page.select_option("#lang", "he-IL")
    page.wait_for_timeout(300)
    assert page.get_attribute("html", "dir") == "rtl"
    assert page.inner_text("#tabDetails") == "פרטים" and page.inner_text("#sheetDone") == "סיום"
    assert page.get_attribute("#voiceSearch", "placeholder") == "חפש קול…"
    page.screenshot(path=os.path.join(SHOTS, "phone-settings-he.png"), full_page=True)
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

def live_checks(browser):
    STATUS["live"] = True
    STATUS["attitude"] = "savage"
    reset_pc()
    with LOCK: STATE["voice_on"] = "phone"
    page = browser.new_page(viewport={"width": 390, "height": 844})
    watch(page)
    page.add_init_script(FAKES)
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
    def line(id, kind, text, fact=None):
        m = {"id": id, "t": 100.0 + id, "kind": kind, "text": text, "speak": True}
        if fact: m["fact"] = fact
        with LOCK: STATE["messages"].append(m)
    # The greeting asks for a turn; the call takes it and is done.
    wait_for(lambda: len(sent("response.create")) == 1, 4, "the greeting asked for no turn")
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
    line(2, "alert", "Back off, you're getting shredded.", "HP 11% (read 0 s ago), MP about 40% (estimated 0 s ago)")
    wait_for(lambda: items_with("shredded"), 4, "the call was not handed the warning")
    text = items_with("shredded")[-1]["item"]["content"][0]["text"]
    assert "not the player" in text and "HP 11% (read 0 s ago)" in text, text
    assert "The game right now" in text and "Character: level 152." in text, text
    assert len(sent("response.create")) == 3, sent("response.create")
    # A response under way (the call answering the player): the next line
    # waits, and no second turn is asked for until the response is done.
    event({"type": "response.created"})
    line(3, "alert", "Pot now, HP's at 9%.", "HP 9.0% (read 0 s ago)")
    page.wait_for_timeout(1500)
    assert not items_with("Pot now") and len(sent("response.create")) == 3, "a turn was asked for while one was under way"
    event({"type": "response.done", "response": {"output": []}})
    wait_for(lambda: items_with("Pot now") and len(sent("response.create")) == 4, 4, "the line did not go once the response was done")
    assert items_with("Pot now")[-1]["item"]["content"][0]["text"].count("Its reading") == 1
    # A line that waited too long (the call talked on for a while) is not
    # said as news when the talking stops.
    event({"type": "response.created"}); event({"type": "output_audio_buffer.started"})
    line(4, "alert", "Level 153! Nice.")
    page.wait_for_timeout(6500)
    event({"type": "output_audio_buffer.stopped"}); event({"type": "response.done", "response": {"output": []}})
    page.wait_for_timeout(1500)
    assert not items_with("Level 153") and len(sent("response.create")) == 4, "a stale line was said as news"
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
    # The PC started again while the call is on: it hears of the call.
    t2 = time.time()
    with LOCK: STATE.update({"boot": "b2", "clip": 0, "messages": []})
    wait_for(lambda: any(r["body"] == {"live": True} for r in requests_since(t2, "/api/mode")), 4, "the restarted PC was not told of the call")
    assert requests_since(t2, "/api/hello"), "no hello to the restarted PC"
    STATUS["attitude"] = "savage"
    page.close()

def browser_args(mic_file):
    """A browser with a microphone fed from `mic_file`, and clips allowed to
    play without a tap."""
    return ["--use-fake-ui-for-media-stream", "--use-fake-device-for-media-stream",
            f"--use-file-for-fake-audio-capture={mic_file}", "--autoplay-policy=no-user-gesture-required"]

chosen = set(sys.argv[1:]) or {"ui", "recognition", "live", "loudness"}
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
srv.shutdown()
if errors:
    print("console errors:", *errors, sep="\n  "); sys.exit(1)
print("phone UI: ok")
