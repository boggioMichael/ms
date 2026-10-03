## MapleSyrup — your MapleStory buddy that watches the game with you and talks

MapleSyrup watches your MapleStory window and talks with you through your phone like a friend sitting next to you: ask how you're doing, how long until you level, how to get somewhere; it warns you when HP or MP runs low and cheers when you level up. Yohai's dog sits over the game and on your phone.

With an OpenAI API key it talks like ChatGPT in a natural voice, **sees your screen**, finds your HUD by itself (any resolution or layout) and **learns what you teach it** by talking: "see that? that's an Orange Mushroom — tell me when one shows up", "warn me when the boss is under 20%", "I'm level 70", "remember that my boss key is F10".

**New in 0.7: a real gaming buddy.** One short sentence, the point first, no "let me check": it answers right away and checks facts in the background, speaking again only if it was wrong. It's bossy, and you pick how it talks to you on the phone: *Friendly*, *Blunt* (the usual) or *Savage* (swears and roasts you, personally). Your own numbers ("how much HP do I have", "how long to level") are answered instantly, without a model. With an xAI key, **Grok** answers the conversation (faster and freer), with OpenAI as the voice and the backup.

**New in 0.6: it learns as you play.** It keeps a notebook about you — your characters, what you're working towards, how you like it to talk, the names you use, what you did last time — and picks up from there next time. It answers faster, straight from what it knows (a quick best guess rather than a search); correct it ("no, Easy Zakum is level 50") and it keeps the right version for good. It waits less after you stop talking, and adapts: longer if you pause mid-sentence, shorter answers if you talk over long ones, sooner HP warnings after a death it didn't warn you about, or the warnings you ask for ("warn me at 40%"). The phone shows what it knows about you, with Forget on each thing; it all stays on your PC.

It talks like a person, on a live call like ChatGPT's voice mode: speak any language and switch or mix languages mid-sentence, talk over it and it stops and goes with what you said, and it hears you while it talks (its voice comes from your phone or earbuds; or choose the PC speakers).

**It records the session for you:** tap *Record the session* on the phone (or say "start recording") and MapleSyrup saves a video of the whole screen with every sound — the game, its voice and yours — in the session folder, everything in sync.

It speaks your language: the phone page comes in 14 languages (English, עברית, Español, Português, Français, Deutsch, 한국어, 日本語, 简体中文, 繁體中文, ไทย, Tiếng Việt, Bahasa Indonesia, Русский), it listens in the one you pick, and it answers in the language you talk to it.

### Install
1. Download **MapleSyrup-Setup-….exe** below and run it. It installs for your user only (no administrator), adds MapleSyrup to the Start menu and the desktop, and asks for an OpenAI API key (optional; you can add it later).
2. Start MapleSyrup and MapleStory (windowed or borderless windowed).
3. Scan the QR code in MapleSyrup's window with your phone (same Wi-Fi), tap **Start listening**, and just talk.

No installer? **MapleSyrup-…-portable.zip** has the same program: unzip it anywhere and double-click MapleSyrup.exe.

### Good to know
- **Windows 10 (1903) or 11, 64-bit.** Nothing else to install: the voice, the text reading and `curl` come with Windows.
- **The program is not code-signed yet.** Windows SmartScreen may say "Windows protected your PC": click *More info → Run anyway*. If *Smart App Control* is on, Windows blocks unsigned programs entirely; it can be turned off in Windows Security → App & browser control.
- **The phone link** is HTTPS on your local network with a certificate made on your PC, so the phone warns the page is "not private" (iPhone: *Show Details → visit this website*). Allow MapleSyrup on private networks if Windows asks. Or use **MapleSyrup (phone over the internet)** from the Start menu: no warning, works on mobile data.
- **Recording:** the first recording downloads ffmpeg once (about 150 MB). **MapleSyrup (recording and streaming)** in the Start menu records from the start, and shows the dog in OBS for streaming (use *Display Capture*).
- **Privacy:** your OpenAI key stays on your PC (`%APPDATA%\MapleSyrup`). With a key, what you say to MapleSyrup and pictures of your game window are sent to OpenAI to answer you (OpenAI charges your account for it: roughly cents per hour). What it learns about you is kept on your PC only (`memory.json` and `knowledge.json` there; delete them to make it forget). With an xAI key, your words and a small picture of the game go to xAI (Grok) for the regular replies. Without a key, nothing leaves your PC except the phone link on your own network.
- Sessions (a log of what was said, marked moments, recordings, `hud-found.png`) are kept in *Documents\MapleSyrup sessions*.
