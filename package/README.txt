MapleSyrup - the MapleStory companion
=====================================

Start
  1. Double-click MapleSyrup.exe.
     The first time, it asks for an OpenAI API key (platform.openai.com/api-keys)
     so it can talk like ChatGPT in a natural voice. Paste it and press Enter
     (it is kept on this PC only, in %APPDATA%\MapleSyrup), or press Enter to
     go without (simple answers, Windows voice). A key can also be put in a
     file called openai-key.txt next to MapleSyrup.exe.
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

If the phone cannot connect
  Double-click "MapleSyrup (phone over internet).cmd". It links the phone
  through a Cloudflare tunnel: no certificate warning, no firewall question,
  works on mobile data too.

If Windows blocks MapleSyrup.exe ("Smart App Control" / "blocked by your
organization's Device Guard policy")
  The program is not signed, and each new version is a file Windows has not
  seen before. Smart App Control cannot allow a single program:
  Windows Security -> App & browser control -> Smart App Control settings -> Off.
  (Since the April 2026 update it can be turned on again from the same place;
  MapleSyrup is then blocked again.)

Files
  Each session is kept in "MapleSyrup sessions" next to MapleSyrup.exe:
  log.txt (what was said and heard), markers.csv and mark-NNN.png, and
  mic.wav when started with --record-mic.
  Optional: write a few lines about yourself in %APPDATA%\MapleSyrup\about-me.txt
  (your name, your class, what you're working towards) and it will know them.

More options: open a terminal here and run  MapleSyrup.exe --help
vision_debug.exe is the engine's own debugger (what it sees, frame by frame).
