"""The phone page against a stand-in PC, in a headless browser: no console
errors, the main screen shows only the essentials, the gear opens Settings,
the voice search narrows the list, the game line opens Details, Hebrew is
right to left. Needs `pip install playwright && playwright install chromium`;
run from the repository root: `python3 tools/phone_ui_check.py`. Screenshots
land in `target/phone-ui/`."""
import json, os, threading, http.server, socketserver, sys
from playwright.sync_api import sync_playwright

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(HERE)
ROOT = os.path.join(REPO, "src", "phone")
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

class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def _send(self, code, body, ctype):
        self.send_response(code); self.send_header("Content-Type", ctype); self.send_header("Content-Length", str(len(body))); self.end_headers(); self.wfile.write(body)
    def do_GET(self):
        path = self.path.split("?")[0]
        if path == "/": self._send(200, open(ROOT + "/page.html", "rb").read(), "text/html; charset=utf-8")
        elif path == "/dog.js": self._send(200, open(ROOT + "/dog.js", "rb").read(), "application/javascript")
        elif path == "/dog-parts.png": self._send(200, open(os.path.join(REPO, "assets", "companion", "dog-parts.png"), "rb").read(), "image/png")
        elif path == "/api/state": self._send(200, json.dumps({"seq": 1, "status": STATUS, "lines": [], "voice_on": "both"}).encode(), "application/json")
        else: self._send(404, b"{}", "application/json")
    def do_POST(self):
        n = int(self.headers.get("Content-Length") or 0); self.rfile.read(n)
        self._send(200, b'{"ok":true}', "application/json")

socketserver.TCPServer.allow_reuse_address = True
srv = socketserver.TCPServer(("127.0.0.1", 0), Handler)
port = srv.server_address[1]
threading.Thread(target=srv.serve_forever, daemon=True).start()

errors = []
with sync_playwright() as p:
    browser = p.chromium.launch()
    page = browser.new_page(viewport={"width": 390, "height": 844}, device_scale_factor=2)
    page.on("console", lambda m: errors.append(m.text) if m.type == "error" else None)
    page.on("pageerror", lambda e: errors.append(str(e)))
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
    page.screenshot(path=os.path.join(SHOTS, "phone-details.png"), full_page=True)
    # Hebrew: right to left, the new words translated.
    page.click("#tabSettings")
    page.select_option("#lang", "he-IL")
    page.wait_for_timeout(300)
    assert page.get_attribute("html", "dir") == "rtl"
    assert page.inner_text("#tabDetails") == "פרטים" and page.inner_text("#sheetDone") == "סיום"
    assert page.get_attribute("#voiceSearch", "placeholder") == "חפש קול…"
    page.screenshot(path=os.path.join(SHOTS, "phone-settings-he.png"), full_page=True)
    browser.close()
srv.shutdown()
if errors:
    print("console errors:", *errors, sep="\n  "); sys.exit(1)
print("phone UI: ok")
