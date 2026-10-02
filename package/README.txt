MapleSyrup - the MapleStory companion
=====================================

Start
  1. Start MapleSyrup: from the Start menu or the desktop icon if you used the
     installer, or double-click MapleSyrup.exe from the zip.
     The first time, it asks for an OpenAI API key (platform.openai.com/api-keys)
     so it can talk like ChatGPT in a natural voice and see your screen. Paste
     it and press Enter (it is kept on this PC only, in %APPDATA%\MapleSyrup), or
     press Enter to go without (simple answers, Windows voice). A key can also
     be put in a file called openai-key.txt next to MapleSyrup.exe.
  2. Start MapleStory (windowed or borderless windowed). Yohai's dog and a
     small HP/MP/EXP panel appear at the top right of the game.
  3. On your phone (same Wi-Fi as the PC), scan the QR code in MapleSyrup's window.
     The phone warns that the page is "not private": the certificate was made
     on your PC, not by a public authority.
       iPhone:  Show Details -> visit this website -> Visit Website
       Android: Advanced -> Proceed
  4. Tap "Start listening", allow the microphone, and just talk to it:
     "hey, how am I doing?", "how long until I level?", "mark that!".

  If Windows asks whether MapleSyrup may use the network, allow private networks.

How it talks
  It answers everything you say, knowing what is on your screen (HP, MP, EXP,
  level, your EXP per hour). If you talk to your stream's chat instead, it
  stays quiet. It starts speaking as soon as its first sentence is ready.
  While it speaks, MapleStory's sound is turned down for a moment, and the
  phone stops listening so it does not hear itself.
  The OpenAI account needs credit (platform.openai.com/settings/organization/billing);
  without it MapleSyrup says so and goes on with simple answers and the
  Windows voice. It also speaks up by itself when HP or MP runs low or you level up.
  On the phone you can choose where replies are spoken (PC, phone, both, off)
  and switch to "only after 'syrup'" for streaming.

It sees your screen, and learns it
  With each thing you say, the model gets a picture of the game, so you can
  ask about anything on screen. When MapleStory first shows up, MapleSyrup
  asks a vision model where your level, HP, MP and EXP bars and minimap are
  (any resolution or UI layout), then measures the bars itself on every
  frame and checks them against the game's numbers every couple of minutes.
  You can see what it found in hud-found.png in the session folder.

  Teach it by talking:
    "see that? that's an Orange Mushroom - tell me when one shows up"
    "that's the boss's HP bar, warn me when it's under 20%"
    "I'm level 61" / "that's not my HP"      (corrections)
    "remember that my boss menu key is F10"  (kept for good)
  Each thing it learns keeps a picture (and gets more as it sees it in other
  poses). The phone lists what it has learned, with a Forget button.
  It all lives in %APPDATA%\MapleSyrup\learned (about-me.txt next to it).
  It can also look things up on the web for MapleStory questions
  (start it with --no-web to stop that).

Recording or streaming it
  Start "MapleSyrup (recording and streaming)" from the Start menu (in the zip:
  double-click "MapleSyrup (recording).cmd"): the dog and the panel then show
  up in recordings (normally they keep out of OBS and screenshots), and what
  the phone's microphone hears is kept as mic.wav in the session folder.
  Record the whole screen, not just the game: OBS "Display Capture" (OBS's
  "Game Capture" and Windows' Win+Alt+R see only the game window), or the
  Snipping Tool's video (Win+Shift+R) with Sound and Microphone on.

If the phone cannot connect
  Start "MapleSyrup (phone over the internet)" from the Start menu (in the zip:
  double-click "MapleSyrup (phone over internet).cmd"). It links the phone
  through a Cloudflare tunnel: no certificate warning, no firewall question,
  works on mobile data too.

If Windows blocks MapleSyrup.exe ("Smart App Control" / "blocked by your
organization's Device Guard policy")
  The program is not signed, and each new version is a file Windows has not
  seen before. Smart App Control cannot allow a single program:
  Windows Security -> App & browser control -> Smart App Control settings -> Off.
  (Since the April 2026 update it can be turned on again from the same place;
  MapleSyrup is then blocked again.)

Privacy
  Your OpenAI key stays on this PC. With a key, what you say to MapleSyrup and
  pictures of your game window are sent to OpenAI to answer you (OpenAI bills
  your account: roughly cents per hour). Without a key, nothing leaves your PC
  except the phone link on your own network.

Files
  Each session is kept in "MapleSyrup sessions" (in Documents when installed,
  next to MapleSyrup.exe from the zip):
  log.txt (what was said and heard), markers.csv and mark-NNN.png, and
  mic.wav when started with --record-mic.
  Optional: write a few lines about yourself in %APPDATA%\MapleSyrup\about-me.txt
  (your name, your class, what you're working towards) and it will know them.

More options: open a terminal here and run  MapleSyrup.exe --help
vision_debug.exe is the engine's own debugger (what it sees, frame by frame).
