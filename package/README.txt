MapleSyrup - the MapleStory companion
=====================================

Start
  1. Double-click MapleSyrup.exe.
  2. Start MapleStory (windowed or borderless windowed).
  3. On your phone (same Wi-Fi as the PC), scan the QR code in MapleSyrup's window.
     The phone warns that the page is "not private": the certificate was made
     on your PC, not by a public authority.
       iPhone:  Show Details -> visit this website -> Visit Website
       Android: Advanced -> Proceed
  4. Tap "Start listening", allow the microphone, and say: "syrup, status".

  If Windows asks whether MapleSyrup may use the network, allow private networks.

Voice commands (say "syrup" first) and phone buttons
  status   level, HP, MP and EXP
  hp / mp / exp / level
  rate     EXP per hour and the time to the next level
  time     how long this session has been running
  mark     save this moment (a screenshot and a line in markers.csv)
  mute / unmute
  help

It also speaks up by itself when HP or MP runs low, when you level up, and
when the game window disappears.

If the phone cannot connect
  Double-click "MapleSyrup (phone over internet).cmd". It links the phone
  through a Cloudflare tunnel: no certificate warning, no firewall question,
  works on mobile data too.

If Windows blocks MapleSyrup.exe ("Smart App Control" / "blocked by your
organization's Device Guard policy")
  The program is not signed. Windows Security -> App & browser control ->
  Smart App Control settings -> Off. (On current Windows 11 it can be turned
  back on later.)

Files
  Each session is kept in "MapleSyrup sessions" next to MapleSyrup.exe:
  log.txt (what was said and heard), markers.csv and mark-NNN.png, and
  mic.wav when started with --record-mic. Nothing is sent anywhere.

More options: open a terminal here and run  MapleSyrup.exe --help
vision_debug.exe is the engine's own debugger (what it sees, frame by frame).
