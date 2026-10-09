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
  2. Start MapleStory (windowed or borderless windowed). MapleSyrup's dog (the
     chow chow from the logo) and a small HP/MP/EXP panel appear at the top
     right of the game.
  3. On your phone (same Wi-Fi as the PC), scan the QR code in MapleSyrup's window.
     The phone warns that the page is "not private": the certificate was made
     on your PC, not by a public authority.
       iPhone:  Show Details -> visit this website -> Visit Website
       Android: Advanced -> Proceed
  4. Tap "Start listening", allow the microphone, and just talk to it:
     "hey, how am I doing?", "how long until I level?", "mark that!".
     It says hello once per visit (a page reloaded within half an hour is
     the same visit): the live call's hello when a call is coming, else its
     own (on the phone, heard once you have tapped). After its own hello, a
     player it does not know yet hears what it does ("I yell when your HP
     drops. Ask me stuff.").

  If Windows asks whether MapleSyrup may use the network, allow private networks.

How it talks
  Like a gamer friend on voice chat, not an assistant: one short sentence,
  the point first, no "let me check", and it tells you what to do. Pick how
  it talks to you on the phone (Settings -> "How it talks to you"):
    Friendly  warm and fun, no swearing
    Blunt     the usual: bossy, cocky, teases you, swears a little
    Savage    swears and roasts you personally (you asked for it); it still
              won't go after who you are (religion, origin, gender and so on)
  Your own numbers are answered at once, without asking a model: "how much HP
  do I have", "what level am I", "how long to level" (English or Hebrew).
  When it isn't sure of a game fact, it says its best guess right away and
  checks it in the background; it speaks again only if it was wrong.
  Grok: with an xAI key, Grok answers the conversation (quick, and freer with
  its words). Put the key in a file named xai-key.txt next to MapleSyrup.exe;
  it moves itself into %APPDATA%\MapleSyrup. OpenAI still makes the voice,
  the look-ups and the live call, and answers whenever Grok can't.

  Its voice: with an ElevenLabs key, pick the voice it speaks in on the phone
  (Settings -> Voice: the voices on your ElevenLabs account, or OpenAI's).
  Put the key in a file named elevenlabs-key.txt next to MapleSyrup.exe; it
  moves itself into %APPDATA%\MapleSyrup. The voice you pick says everything
  except a live call, which speaks in its own voice (the call stays on when
  you pick a voice; turn it off under "Live call on the phone" if you want
  the picked voice for replies too).
  Hebrew is spoken by ElevenLabs' Eleven v4; when your account can't use it,
  or ElevenLabs is down or out of credit, OpenAI's voice says the line.

  With an OpenAI key, talking to it is a live call, like ChatGPT's voice mode:
  its voice comes from the phone (or earbuds on the phone), and the phone
  listens while it talks. Speak any language, switch or mix languages
  mid-sentence (Hebrew and English, say) and it follows you, with no setting.
  Talk over it and it stops and goes with what you said. It sees your screen
  while MapleStory is the window in front, and says its own lines (low HP,
  a level-up) on the call too, in the language you're speaking: handed to
  the call at most every 20 seconds, with the number only for HP and MP; a
  death or a level-up goes at once and is said even when it comes late. If
  the phone sleeps and the call drops, MapleSyrup notices within seconds
  and its lines go back to its own voice. While it talks, MapleStory's
  sound is turned down.
  Prefer its voice on the PC speakers? Choose "PC" under "Replies spoken on"
  on the phone: then it works as below.

  Like talking to a friend: it answers everything you say, knowing what is on
  your screen (HP, MP, EXP, level, your EXP per hour). If you talk to your
  stream's chat instead, it stays quiet.
  It answers as soon as you stop talking, and its voice starts while the
  rest of the answer is still being made. It answers from what it knows
  rather than stopping to look things up, even when it isn't completely
  sure ("I think..."): when it gets something wrong, just correct it, and
  it remembers (see "It learns as you play"). Talk over it and it stops and
  listens ("wait", or just ask something else); a warning, or news such as
  a death, still plays out on the PC. Pause mid-sentence and go on, and it
  waits for the rest instead of answering half of it. The phone keeps
  listening while it talks on the PC and tells your voice from its own.
  While it speaks, MapleStory's sound is turned down for a moment.
  The OpenAI account needs credit (platform.openai.com/settings/organization/billing);
  without it MapleSyrup says so and goes on with simple answers and the
  Windows voice. It also speaks up by itself when HP or MP runs low, when you
  take a beating, when you die and when you level up (a level-up is the level
  at the bottom left going up by one — nothing else counts): a warning is
  shouted, a death or a level-up told.
  A warning is said once per fight. Pot after it and the beating is not
  shouted again in that fight, and the low bar not until the fight is over
  or the bar goes lower; a low bar nothing answers is said again after 12,
  30, 60 and 120 seconds. After four warnings that nothing answers (not a
  word, no potion, no EXP) it says it will wait, and holds the rest for ten
  minutes or until you are back. Pot within three seconds of the shout three
  fights running and it stops shouting "back off": it watches instead, and
  shouts when HP goes deeper than you usually let it or the potion is late;
  after a late potion, no potion or a death, it shouts again until you have
  handled three more. A reading has to hold: one odd frame is neither a
  potion nor a fall, and a bar it can only guess at is not warned from. A
  bar that reads low for minutes while your EXP keeps coming is a bar it is
  reading wrong: it says so once ("I'm reading your HP wrong") and holds
  that bar's warnings until it reads above the warning level again.
  "Don't talk to me this way" makes it friendly.
  On the phone you can choose where replies are spoken (PC, phone, both, off)
  and switch to "only after 'syrup'" for streaming.

The dog
  At the top of the phone page MapleSyrup's dog lives in a box of its own. It
  hops about, runs, jumps, spins, rolls over, sniffs around, scratches, plays
  with its ball and naps in its bed, as it feels like it. It looks at you
  while you talk, tilts its head while it thinks, and talks: its mouth opens
  as far as the voice is loud. It barks at danger, jumps for joy when you
  level up, and plays dead when your character dies. Tap the box and it comes
  running; tap the dog to pet it. On the PC it sits beside the panel over
  the game and talks there too.

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
  It can also look MapleStory questions up on the web, when you ask it to
  ("look it up") or when it has no idea (start it with --no-web to stop that).

It learns as you play
  The more you play together, the better it gets. Every few minutes, and when
  it starts (for the sessions before), it looks back on what was said and
  keeps a notebook: you and your characters, what you're working towards,
  how you like it to talk, the names you use (so it hears them right), and
  what you did lately, so next time it picks up from there.
  Your corrections teach it: "no, Easy Zakum is level 50" is kept for good,
  and it trusts that over what it thought. What it looked up is kept too, so
  the same question is answered at once next time.
  It adapts to how you talk: it answers quickly after you stop talking, and
  waits a little longer if you often pause mid-sentence and go on; on a live
  call it waits longer if it keeps jumping in before you finish; if you often
  talk over long answers, it keeps them shorter.
  Warnings too: "warn me at 40%", "no more MP warnings", "warn me like
  before". If you die with no warning in that fight while your HP went down,
  it warns you sooner from then on (up to half the bar), and says so.
  The phone shows what it knows about you ("What I know about you"), with a
  Forget button on each thing. It all stays on your PC, in
  %APPDATA%\MapleSyrup\memory.json and knowledge.json: delete them to make it
  forget everything.

Languages
  The phone page speaks English, Hebrew, Spanish, Portuguese, French, German,
  Korean, Japanese, Chinese (simplified and traditional), Thai, Vietnamese,
  Indonesian and Russian. It follows the phone's language; pick another on the
  main screen, under "Start listening". That is also the language the phone
  listens in; when a live call hears you speak Hebrew, the phone listens in
  Hebrew off the call too, and says so (picking a language puts it back).
  MapleSyrup answers in the language you speak to it and says its own lines
  (warnings, level ups) in yours too. The installer speaks yours when Inno
  Setup has it.

Recording the session
  Tap "Record the session" on the phone (or just say "start recording"; tap
  or say "stop recording" to stop). MapleSyrup records a video of the whole
  screen with every sound: the game, MapleSyrup's voice (on the PC or on a
  live call on the phone), and you from the phone's microphone. The dog and
  the panel are in it. It is saved as "recording HH-MM-SS.mp4" in the session
  folder; the phone says when it is saved. "MapleSyrup (recording and
  streaming)" from the Start menu records from the start.
  The first time, MapleSyrup downloads ffmpeg (about 150 MB, kept in
  %APPDATA%\MapleSyrup\ffmpeg) to do the recording; it uses the graphics
  card's video encoder when there is one. Each sound is put where it was
  heard, so the voices match the picture. If MapleSyrup is closed while
  recording, it finishes the file first; if it can't, the "(unfinished)" file
  still plays.
  To check recording on a PC: MapleSyrup.exe --record-test (a few seconds of
  the screen with a flash and a tone, which must line up).

Streaming it
  Start "MapleSyrup (recording and streaming)" from the Start menu (in the zip:
  double-click "MapleSyrup (recording).cmd"): the dog and the panel then show
  up in OBS (normally they keep out of captures, except while MapleSyrup
  records), and what the phone's microphone hears is kept as mic.wav in the
  session folder. In OBS use "Display Capture" ("Game Capture" sees only the
  game window).

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

Updates
  MapleSyrup updates itself. Now and then it looks at datta-syrup.ai for a
  new version, fetches it in the background and installs it the next time
  you start MapleSyrup (or right away: Settings on the phone -> Update now).
  It only takes a version signed by the release key built into it, and keeps
  the version you had beside it until the new one has run for a while; a
  version that does not come up twice is rolled back, and the one after it
  is tried instead. Turn it off in Settings on the phone, or start MapleSyrup
  with --no-update.

Workshop (off unless you turn it on)
  MapleSyrup can rewrite itself, on this PC only. Turn the workshop on in
  Details on the phone, then say "change yourself: make the HP warning
  shorter" (or type it there). A coding agent installed on this PC — Claude
  Code or Codex CLI, whichever you have — changes the source in the
  checkout at %USERPROFILE%\GitHub\ms (or where --repo says), the program is
  built and its tests run (a few minutes; the game keeps priority), and the
  new version installs the next time MapleSyrup starts, with the old one
  kept to go back to. "Undo the last change" takes it back. The work stays
  on a branch of this PC's own: nothing is pushed, nothing is published, and
  the site's updates are paused while the workshop is on. It needs git, the
  Rust toolchain and a coding agent on this PC; the first build fetches the
  dependencies and takes longer.

Privacy
  Your OpenAI key stays on this PC (a live call uses a key that works for a few
  minutes only, made for it). With a key, what you say to MapleSyrup (on a
  live call, your voice itself) and pictures of your game window are sent to
  OpenAI to answer you; pictures only while MapleStory is the window in front.
  To learn, every few minutes it sends the text of the conversation (not the
  pictures) to OpenAI to update its notebook; what it learns is kept on your
  PC only. With an xAI key, what you say and a small picture of the game go to
  xAI (Grok) for the regular replies. With an ElevenLabs key, what it says
  goes to ElevenLabs to be spoken. Your keys stay on this PC
  (%APPDATA%\MapleSyrup).
  OpenAI bills your account: a live call costs more than the PC voice (very
  roughly a dollar or two per hour of play, depending on how much you talk). Without a key, nothing leaves your PC
  except the phone link on your own network.
  Recordings are made and kept on your PC only, and only when you ask for one.

Files
  Each session is kept in "MapleSyrup sessions" (in Documents when installed,
  next to MapleSyrup.exe from the zip):
  log.txt (what was said and heard), markers.csv and mark-NNN.png, the
  recordings (recording HH-MM-SS.mp4), and mic.wav when started with
  --record-mic.
  Optional: write a few lines about yourself in %APPDATA%\MapleSyrup\about-me.txt
  (your name, your class, what you're working towards) and it will know them.

More options: open a terminal here and run  MapleSyrup.exe --help
vision_debug.exe is the engine's own debugger (what it sees, frame by frame).
