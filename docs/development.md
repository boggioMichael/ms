# MapleStory Vision & Game State Library

## Overview

This crate provides a professional, production-grade perception pipeline for analyzing MapleStory gameplay from screen captures. It is designed as a reusable foundation for AI decision-making and game state monitoring.

**Key concept**: Every detector returns a [`Detection<T>`] wrapping confidence, reliability, timestamp, and failure reasons. Downstream AI can distinguish "very confident the HP is 50%" from "found a red bar, might be HP" instead of getting binary "found/not found" signals.

See [vision-architecture.md](vision-architecture.md) for the existing detector-oriented design and [ARCHITECTURE_V2.md](ARCHITECTURE_V2.md) for the revised evidence-first architecture.

## Module Layout

Every pixel operation is [Syrup](https://github.com/boggioMichael/syrup)'s
(a Cargo git dependency, pinned): capture, geometry, colour, bars, glyph
reading, template matching, tracking, motion, OCR, quality. This crate
keeps what the pixels mean in MapleStory, and the orchestration.

### The per-frame path

- **`perceive.rs`**: one frame through the companion's eyes — the
  detectors still needed, then the sight — the same function for the
  `maplesyrup` binary and for `vision_bench`.
- **`sight/`**: what MapleSyrup learned about the player's own screen.
  `mod.rs` finds the HUD from the pixels (`find_hud`) and measures its bars
  on every frame (`syrup::bars::BarModel`); `numbers.rs` reads HP, MP and
  EXP in the game's own font (`syrup::glyphs`, learned from labelled
  examples, cross-checked against the bars) — on or just above the bar
  (`Line`), or, for a HUD that prints them higher with the field's name
  and brackets (Classic World: `HP[178/178]`), in a `Window` measured in
  bar heights, with a font of its own (`classic`, its own thresholds) so
  the two HUDs' digits are never averaged; `things.rs` follows what the
  player taught it (`syrup::template` sets, `syrup::tracking`, a stripe of
  the frame swept per frame for newcomers); `teacher.rs` is the vision
  model, asked only when the pixels fail, with backoff.
- **`vision/`**: `snapshot.rs`'s `PerceptionPipeline` runs only the
  detectors asked for (`Detectors`) and says which were not run; the HUD
  geometry detector (`hud_geometry.rs`, `detectors/hud.rs`) runs until the
  sight sees the HUD; `detectors/{motion,dialog,panels,environment,
  combat}.rs` feed the preview window; `types.rs`, `geometry.rs`, `ocr.rs`
  and `quality.rs` re-export Syrup's.
- **`capture.rs`**: which window is the game, and a capture session on it
  (`syrup::capture::Window`; on Windows, Windows.Graphics.Capture with GDI
  as the fallback; `GameCapture::path` says which).
- **`util/`**: `stages.rs`, per-stage timing of the path from its tracing
  spans; `pool.rs`, the vision engine's worker threads.

### The companion

- **`companion/`**: what MapleSyrup says and when from the numbers alone —
  warnings, a beating, a death, level-ups, EXP/hour, voice commands, the
  `Observation` of a frame. It has no clock (time is passed in), so each
  rule is tested by playing a session through `Companion::observe`. It
  acts only on what is read or learned: the HUD detector's colour-run
  guesses stay out of the `Observation` (`Reliability::Corroborated` or
  nothing). The rules, all in `companion/mod.rs`:
  - **A reading has to hold.** A number read in the game's font is taken
    as it comes; a bar's fill whose readings swing back and forth (three
    turns of `SWING` within `SWINGS_WINDOW`) is a guess, held until it
    settles (`Steadiness`: `SETTLE_SECS`, doubling up to
    `SETTLE_MAX_SECS`). A fall is measured from `held_readings` (a
    one-frame spike left out); a potion's worth back (`POTTED`) answers a
    line only once it has held `HELD_FRAMES`. A death read from the fill
    must last `ZERO_HOLD_SECS`.
  - **Once per fight.** The beating (`FALL_POINTS` within `FALL_SECS`,
    `watch_hp`) is said once per fight — over `FIGHT_OVER_SECS` after the
    last fast fall — and again only unanswered, after `FALL_COOLDOWNS`;
    never with `hp_low` at 0. Each bar's low warning is a `Low`: once per
    fight, again unanswered on the same cadence (dealt from the
    `*_AGAIN` decks, `Low::repeating`), or once more at once when the bar
    goes lower than at the last line. `Low::warned_this_fight` is what the
    death line (`DEATH_WARNED`, also after a beating shouted this fight)
    and `sooner_warning` go by; the mark moves only when HP spent
    `SOONER_BAND_SECS` in the `SOONER_BAND` above it on the way down.
  - **Trust.** After `TRUST_FIGHTS` fights in a row potted within
    `TRUST_POT_SECS` of the line, a fall is watched (`watch`), not
    shouted; it is shouted after all under the player's floor by
    `TRUST_MARGIN` (trust kept if potted in time; the floor follows the
    lowest fall handled in time), under `TRUST_BOTTOM` of the mark however
    the floor stands, or when the potion is late (trust gone). A fight over
    unanswered, and a death, end it too; a death during a watched fall
    moves no mark.
  - **A bar read wrong.** `DOUBT_AFTER_LINES` unanswered lines of one fight
    with EXP gained since the first and `Low::due` returns `Due::Misread`:
    one note (`lines::MISREAD`), then that bar's warnings (and, for HP,
    the beating) wait until it reads above the mark for
    `BELIEVE_AGAIN_SECS`.
  - **The cadence and the hold.** A fight's unanswered low lines go
    `Cadence::First`, `Again`, `Ask` (the second repeat is a question, the
    `*_ASK` decks), times unchanged. In `pace`, after `UNANSWERED_MAX`
    warnings with no sign of life — a word, an HP or MP potion that held,
    `EXP_GAINED`, a level — the rest wait `HOLD_SECS`; the note (at most
    once per `HOLD_TOLD_EVERY`) is `HOLD_HERE` when the player spoke within
    `HOLD_AWAY_SECS`, else `HOLD_AWAY`. A word or a potion ends the hold at
    once; a potion seen as one (a jump, `jumped`, within
    `SEEN_POTION_SECS`) gets one `POTTED_AT_LAST` word when the note was
    said within `POTTED_AT_LAST_SECS`. Otherwise `AFTER_HOLD` come through
    when it runs out. News passes a hold. The main loop calls
    `player_spoke` when the player speaks, holds the taught things' alerts
    with its own, and the coach's looks and stalls (`alerts_held`,
    `Glance::held`).
  - **Shouted or told.** `Kind::Warning` is danger now (a beating, a low
    bar, a taught thing past the player's mark); `Kind::Alert` is news (a
    death, a level-up, a thing seen, the coach's word, "still there?").
    `Kind::kept()` is what outlives a talk-over; the voice shouts only a
    warning (`ai::openai::Delivery::urgent`); the phone and the console
    colour the two apart.
  - **The cards.** `companion::lines` has six or seven cards per attitude
    for each situation (fewer for the decks in `lines::SHORT`: `SOONER`,
    `MISREAD`, `POTTED_AT_LAST`),
    dealt from a `Deck` (`attitude.rs`): every card before any repeats,
    never one twice running, a fight's first line never a "still/again"
    card. Decks are seeded per session (`Companion::new`; `seeded` for
    tests, which replays a session exactly); `Companion::settled(known)`
    shuffles a known player's first round whole, where a new player hears
    each deck's lead (its first card, the most informative) first.
  - **Presence.** `still_there` asks once a session, when the game has sat
    idle `STILL_THERE_SECS` (HP and EXP unmoved, in view, no word) — never
    while dead, during a hold, or within `STILL_THERE_AFTER_WARNING_SECS`
    of a warning. `player_spoke` keeps the silence it ended
    (`SoFar.quiet_before`: `QUIET_BEFORE_SECS` or longer, reported for
    `QUIET_BEFORE_KEPT_SECS`) for the reply's snapshot (`so_far`).
  - **Level-ups.** The level read at the bottom left going one above the
    highest seen for the character (`top_level`), held `LEVEL_HOLD_SECS`;
    one below it is a misread until it has held `LOWER_LEVEL_HOLDS` times
    as long, then taken quietly. The EXP bar's wrap only has the sight
    read the number again.
  - A tone complaint ("don't talk to me this way") drops the attitude to
    friendly (`commands::tone_complaint`).
- **`coach/`**: when MapleSyrup speaks up on its own beyond that. `Coach`
  is fed every frame and returns a `Reason` when a model should look:
  `CloseCall` and `Streak` (reactions: they wait only for the talking to
  stop and `CONSULT_GAP`, and go without the picture,
  `Reason::wants_picture`), `NewScene`, `LevelUp` (the companion's
  verified one, through `Coach::leveled`), `ExpStalled` (seconds of play
  with EXP unmoved, held minutes not counted: `STALL_AFTER`,
  `STALL_AGAIN`), `Look` (neither while `Glance::held`). The main loop
  turns it into `ai::Job::Coach`, and the model answers one line or
  `[silent]`, from what happened and a few example lines in the player's
  attitude (`Reason::describe`, `coach::examples`). Pacing lives here and
  is tested by playing sessions through it (`MIN_GAP`, `CONSULT_GAP`,
  `LOOK_EVERY` growing to `LOOK_AT_MOST`, put off by talk in the last
  `TALK_WINDOW`; `NEW_SCENE_AGAIN`, `NEW_SCENE_HUSH`); a look called off
  by the player's words (`Coach::called_off`) leaves the pace alone.
  `coach::scene` is the frame fingerprint (32×18 cells of brightness, a
  few thousand samples whatever the frame's size) and what a run of them
  says: a cut, a new scene once it settled (not a picture seen in the last
  `SEEN_FOR`), how much is going on.
- **`ai/`**: the model clients, the teaching loop, the tools, the coach's
  look (`coach()`). The worker is two lanes (`spawn_brains`):
  `Worker::send` routes `Job::Speak`, MapleSyrup's own lines, to the mouth
  lane and every other job (replies, looks, the hello) to the thinking
  lane, which owns the conversation — so a warning never queues behind a
  look's model call. The voice is one: `Mouth::floor` is held while a line
  is made, on either lane, so a warning waits at most for the line being
  made and clips never interleave. The mouth lane hands each line it said
  back as `Work::Said`, and the thinking lane puts it in the conversation
  (`Brain::watched`: "a warning", "news"), so the next reply knows its own
  last words. A call-off (`Worker::cancel`) does not stop the line of a
  warning or news (`Job::kept`). Every reply passes `brain::humanise` (in
  `for_speech`): assistant-speak sentences go, and an opener ("Sure
  thing,", "Of course,") goes only when what follows stands as a sentence
  (`stands_alone`: three words, or a verb) — "Sure thing, boss." stays
  whole; sentences glued at a full stop get their space (`unglued`). A
  sentence that only restates the snapshot (the window's state, the level,
  the map, a bar's percent) is left out when nothing was asked about the
  game (`unasked`; kept for a question or "talk to me",
  `wants_an_answer`). A pure greeting is answered from a deck without the
  model (`companion::instant`, `Ask::Hello`), and a request for a language
  (`commands::language_request`) switches its lines and the phone at once
  (`lang_request` in the status).
- **`phone/`**: the phone link (`Hub`, and the page, `page.html`). What
  it is told and when is decided in `bin/maplesyrup/main.rs`, on
  `Outputs`:
  - **The hello** (`Outputs::hello`): one per visit, by whoever will talk —
    `Hello::Call` when the page is about to open a live call
    (`call_greets` in the status, which the page reads), else
    `Hello::Clip`, MapleSyrup's own; `Hello::Quiet` for a page back within
    `HELLO_AGAIN` unless the live toggle changed who greets (`greeted_by`;
    a clip hello not heard yet is withdrawn, `withdraw_hello`). A hello
    left to a call that has not opened within `CALL_GREETS_FOR` is said
    as a clip after all (`hello_overdue`, `hello_late`); a page whose call
    failed to open hands the hello back at once. A call that opens on a
    greeted visit makes the hello final (`call_opened`, `hello_heard`): a
    reload is Quiet. A player it does not know (`Learning::knows_player`)
    hears `TERMS` once a session, `TERMS_AFTER` after the clip hello, or
    from the call's own greeting (`new_player` in the status).
  - **The call** (`Relay`): its own lines go to a live call at most once
    per `RELAY_GAP` (a button's answer at once, outside the relay), the
    newest of each kind waiting for the gap; an
    urgent line (a death, this frame's level-up) goes at once, and a death
    drops the warnings and news that waited. `Outputs::hand` puts the
    reading (`fact`) behind a `Kind::Warning` only, and
    `Hub::post_with_fact` takes `urgent` to the page, which says the lines
    at its next quiet moment (`sayLive`, `flushSay`): a line not urgent
    that waited `SAY_STALE_MS` is dropped and reported (`/api/turn` with
    `what: "dropped"`, logged "[live] not said, too late"); an urgent one
    is said however late. A call whose phone is gone `CALL_LOST_AFTER` is
    lost (`watch_call`, `call_ended`) and the lines go back to its own
    voice — the parked ones too, a warning or news said, the rest logged.
    The page says the call is off only when it goes away (`pagehide`, a
    beacon to `/api/mode`), not when hidden: a glance costs nothing; and
    the status's `on_call` lets a page whose call is on correct a PC that
    took it as lost (`healCall`).
  - **A talk-over** (`Outputs::cut`): a reply or a note is hushed and the
    rest of it dropped; a warning or news plays out on the PC, and its clip
    held for the phone goes after the player's turn. Mute (`silence`)
    stops everything.
  - **The page**: a clip refused before a tap (`NotAllowedError`) is kept
    for it (`playNextClip`, `blocked`); at the tap the hello and the terms
    play, and a clip no longer news is dropped — `/api/state`'s `clips`
    carry kind and age; a warning past `SAY_STALE_MS`, news past
    `NEWS_STALE_MS`. `hearingLanguage` turns the clip-mode recogniser to
    Hebrew after two Hebrew sentences on the call (or one long one) and
    back after two English ones, shown beside the language picker (`#lang`,
    on the main screen) with an × to undo; `/api/state` carries a boot id,
    and a page open across a restart starts over.
- **`app/`**, **`platform/`**, **`observe/`**, **`overlay/`**: the
  screen, the console and voice, the dashboard and preview, the overlay.

### Knowledge Base (`src/knowledge/`)

Structured, non-verbatim MapleStory gameplay knowledge:
- `dialogs.rs`: Dialog classification keywords
- `mechanics.rs`: Rune, portal, farming heuristics
- `monsters.rs`: Behavior profiles for common creatures

### Entry Points

- **`config.rs`**: `AppConfig` global settings
- **`logging.rs`**: Tracing initialization
- **`hud.rs`**: Convenience re-export of HUD detection API (backward compatibility)
- **`bin/maplesyrup/`**: the companion; **`bin/vision_bench.rs`**: the
  per-frame path timed (`bench/README.md`); **`bin/vision_debug.rs`**: the
  live vision debugger.

## Quick Start

### 1. Initialize and Capture

```rust
use ms::{capture::capture_game_window_info, logging::init_tracing};

fn main() {
    init_tracing("info");
    
    if let Some((title, image)) = capture_game_window_info() {
        println!("Captured window: {} ({}x{})", title, image.width(), image.height());
        analyze_frame(&image);
    }
}
```

### 2. Create a Perception Pipeline

```rust
use ms::vision::PerceptionPipeline;

fn analyze_frame(image: &image::RgbaImage) {
    let mut pipeline = PerceptionPipeline::new();
    let state = pipeline.detect(image);
    
    // state contains: HUD, Motion, Dialog, Panels, Environment, Combat
    if state.hud.hp.is_present() {
        let hp = state.hud.hp.value.unwrap();
        println!("HP: {}/{}  (confidence: {})", 
            hp.value.unwrap_or(0),
            hp.percent.unwrap_or(0.0),
            state.hud.hp.confidence
        );
    }
}
```

### 3. Access Detector Output

Every detector output is a `Detection<T>` carrying:

```rust
// HUD metrics
if let Some(metric) = state.hud.hp.value {
    println!("HP: {}%", metric.percent.unwrap_or(0.0));
}

// Moving entities (motion detector)
if state.motion.is_present() {
    for entity in state.motion.value.unwrap() {
        println!("Entity {} at ({}, {}), velocity ({}, {})", 
            entity.id, entity.bounds.x, entity.bounds.y, 
            entity.velocity.0, entity.velocity.1);
    }
}

// Dialog detection
if state.dialog.is_present() {
    let dialog = state.dialog.value.unwrap();
    println!("Dialog: {:?}", dialog.kind);
}

// Environment
if state.footholds.is_present() {
    for edge in state.footholds.value.unwrap() {
        println!("Platform at y={}", edge.bounds.y);
    }
}

// Combat intensity
println!("Combat: {:?}", state.combat_intensity.value.unwrap().intensity);
```

## Detector Reference

| Detector | Input | Output | Confidence Scaling | Known Limitations |
|----------|-------|--------|-------------------|-------------------|
| HUD | RGBA frame | `HudReading` (metrics + markers) | High (geometry + OCR) or Medium (geometry-only) | OCR can fail on unusual fonts |
| Motion | RGBA frame | `Vec<MovingEntity>` | Medium-High (stable tracks are more confident) | Identifies moving blobs, not sprite types |
| Dialog | RGBA frame | `DialogReading` (bounds + kind + text) | Medium-High if OCR succeeds, Medium if geometry-only | Depends on OCR reliability |
| Minimap | RGBA frame | `MinimapReading` | Low-Medium (heuristic-only) | Proportional search, might miss non-standard skins |
| Chat Log | RGBA frame | `ChatLogReading` | Low-Medium (text density heuristic) | Might false-positive on other text regions |
| Icon Row | RGBA frame | `IconRowReading` (vec of icon slots) | Low-Medium (saturated blob count) | Cannot identify which buff/skill each icon represents |
| Footholds | RGBA frame | `Vec<PlatformEdge>` | Low (luminance gradient heuristic) | Reports candidate edges, not verified walkable graph |
| Combat Intensity | Temporal | `CombatReading` | Medium (smoothed history) | Requires multiple frames to warm up |

## Testing

Run all tests (about 420: the library's, the `maplesyrup` binary's, and
the integration tests under `tests/`):

```sh
cargo test
```

Run only unit tests:

```sh
cargo test --lib
```

Run only integration tests:

```sh
cargo test --test hp_bar_integration
```

Run a specific detector's tests:

```sh
cargo test vision::detectors::hud::tests::
```

The phone page, against a stand-in PC in a headless Chromium (`pip install
playwright && playwright install chromium`; from the repository root):

```sh
python3 tools/phone_ui_check.py              # all five scenarios
python3 tools/phone_ui_check.py live hello   # some of them
```

`ui` (the main screen, Settings, the toggle's hello), `recognition` and
`loudness` (turn-taking: a stand-in recognizer, a microphone fed from a
file), `live` (a stand-in call: the relay, urgency and drops, a warning's
colour and bark, the recogniser's Hebrew, the call off when the page is
hidden) and `hello`, which runs under the browser's own autoplay policy
(no clip before a tap, as on a phone; the other four allow autoplay).
`PHONE_PAGE=<path>` runs them against another copy of the page, to see a
check fail against the page as it was.

To read an evening rather than imagine one: `examples/evening.rs` plays
ninety scripted minutes through the companion and the coach and prints
everything they would say (`MM:SS  [kind]  text`; a consult prints its
reason and the example lines, no model text) and a summary — lines per
kind, warnings per ten minutes, the longest silence, the distinct cards.
The arguments are the attitude and the deck seed (blunt, 7 by default):

```sh
cargo run --release --offline --example evening
cargo run --release --offline --example evening -- savage 20261008
```

## Releasing

A release is a version in `Cargo.toml` and a tag `v<version>` on the commit
that carries it; the two must agree, since the program reports
`CARGO_PKG_VERSION` to the updater and the pipeline refuses a tag that says
otherwise.

1. Set the version in `Cargo.toml`, write the "New in" paragraph in
   `installer/release-notes.md`, commit, push.
2. Tag it (`git tag v0.9.0 && git push origin v0.9.0`). The
   `MapleSyrup standalone` workflow builds the Windows package, lays the
   files out under their version's names (`MapleSyrup-Setup-0.9.0.exe`,
   `MapleSyrup-0.9.0-portable.zip`, `MapleSyrup-0.9.0.exe`, `SHA256SUMS`),
   writes `manifest.json` (the version, the date, the commit, the notes,
   each file's size and SHA-256 and where it will be served from) and signs
   it with the release key (`manifest.json.sig`, Ed25519), and makes a
   **draft** GitHub release with all of that. Nothing is public yet.
3. Try the draft's files. Then, in Actions, run `MapleSyrup standalone` by
   hand **on the tag** with *publish* ticked (and a line of notes, shown on
   the phone): the GitHub release is made public and the files, the manifest
   and its signature are committed to the company site's repository under
   its downloads folder (versioned names, plus `MapleSyrup-Setup.exe` and
   `MapleSyrup-portable.zip` with fixed names for the site's links). From
   then on every MapleSyrup that looks at the channel fetches the new
   version.

The workflow needs, in the repository's settings:

| what | where | holds |
|---|---|---|
| `RELEASE_SIGNING_KEY` | secret | the Ed25519 private key, PEM (`openssl genpkey -algorithm ed25519`); its public half is `update::PUBLIC_KEY` in `src/update.rs` — change both together, and know that a program built with the old key will never take a manifest signed with the new one |
| `SITE_TOKEN` | secret | a token that may push to the site's repository (a fine-grained personal access token with *Contents: read and write* on that repository alone) |
| `SITE_REPO` | variable | the site's repository, `owner/name` |
| `SITE_URL` | variable | where the site is served (default `https://datta-syrup.ai`) |
| `SITE_DOWNLOADS` | variable | the folder in the site's repository served as `/downloads/` (default `downloads`) |

Without `SITE_TOKEN` and `SITE_REPO` the manifest points at the GitHub
release's own files instead, and *publish* only makes the release public.

### How the updater works (`src/update.rs`)

The way Android updates its APEX modules, scaled to one program:

- **The channel.** `https://datta-syrup.ai/downloads/manifest.json` and
  `manifest.json.sig` beside it. The program looks 45 s after it starts and
  every hour after that (later after a failure), and whenever the phone
  asks. `MAPLESYRUP_UPDATE_URL` points it elsewhere (a `file://` URL will do
  for a test of the whole way; `tests/update_channel.rs`).
- **Verified, or nothing.** The signature must be the built-in key's over
  exactly the manifest's bytes; the fetched program must have the manifest's
  size and SHA-256 and start like a Windows program. Anything else is
  dropped and said in the log.
- **Staged.** The program is fetched into `%APPDATA%\MapleSyrup\updates\`
  and `staged.json` written. The phone shows "0.9.1 is ready: it installs
  the next time MapleSyrup starts", with *Update now*.
- **Activated at the next start, atomically.** Before anything else,
  `update::at_start` copies the staged program beside the running one, then
  renames the running one to `MapleSyrup.old.exe` and the new one into its
  place (a running program can be renamed on Windows; a rename on one volume
  is atomic, so there is no moment without a program), writes
  `pending.json`, starts the new program in a console of its own and leaves.
- **Committed, or rolled back.** The new version counts its starts in
  `pending.json`; after `HEALTHY_AFTER` (90 s) of running it commits: the
  kept program is deleted. A version that is started `BOOTS_BEFORE_ROLLBACK`
  (2) times without committing is put back at the start after them: the
  kept program returns to its place, the version goes into `blocked.json`
  and is never offered again (the one after it will be), and the kept
  program is started.
- **The player's say.** Settings on the phone: *Updates itself when a new
  version is out* (kept in `memory.json` as `updates`), *Update now* (the
  staged program put in place at once and MapleSyrup restarted);
  `--no-update` or `MAPLESYRUP_NO_UPDATE=1` for a session without any of it.
  Everything the updater does is in `updates/log.txt` and the session log
  (`[update]` lines).

## The workshop (`src/workshop.rs`)

MapleSyrup rewriting itself, on one PC, for the player who runs it there.
Off unless the player turns it on (Details on the phone, kept in
`memory.json` as `workshop`; or `--workshop` for a session).

- **Asked** by voice ("change yourself: …", "rewrite yourself so that …",
  "תשנה את עצמך: …"; `workshop::request`), by the conversation model (the
  `change_your_code` tool, offered only while the workshop is on), or by
  typing it on the phone (`/api/workshop`). "Undo the last change" reverts
  the workshop's last commit and builds again.
- **The checkout**: `%USERPROFILE%\GitHub\ms`, `MAPLESYRUP_REPO` or
  `--repo`. It must be clean. The work is on `local/<pc>`; the first time,
  that branch is made from the commit the running program was built from
  (`MS_COMMIT`, baked in by `build.rs`; fetched if the checkout lacks it),
  so the change is to the program the player is running, not to whatever
  master has become.
- **The coder**: Claude Code (`claude -p … --permission-mode acceptEdits
  --allowedTools "Read,Edit,…,Bash(cargo *)"`) or Codex CLI (`codex exec
  --full-auto`), whichever is on the PATH (`.cmd` shims included), the
  first found unless the player picked one. It gets the instruction, the
  end of the session's log, and the rules: small and local, keep the build
  and the tests green, nothing in `.github/` or `installer/`, not the
  updater's key or channel, no git. It leaves a sentence in
  `WORKSHOP_NOTE.txt`, which becomes the commit's body and what the player
  hears.
- **The checks**: something changed; nothing out of bounds (`.github/`,
  `installer/`, the `PUBLIC_KEY`/`CHANNEL` constants); `cargo build
  --release --bin maplesyrup` and `cargo test --release --lib`, at
  below-normal priority with half the cores, so the game keeps its frames.
  A failure throws the coder's changes away (`git checkout -- . && git
  clean -fd`; the tree was clean before) and says why; the logs stay under
  `%APPDATA%\MapleSyrup\workshop\<time>\`.
- **Kept and staged**: `git commit` as "MapleSyrup workshop"; the program
  copied to `updates\MapleSyrup-local-<commit>.exe` and staged for the
  updater with `local: true` and the version `<running>+local.<commit>`
  (activated at the next start whatever its number; blocked like any
  other if it does not come up twice; "Update now" works). While the
  workshop is on, the channel's releases are not taken.
- **Never**: a push, a change to master, a release. The workshop has no
  remote to speak of; the branch is the PC's. If the player wants a change
  upstream, they push the branch themselves and open a pull request.

Tests: `tests/workshop_pipeline.rs` runs the whole way on a checkout of its
own with a stand-in coder and a stand-in cargo (scripts): the branch from
the running commit, the change built, tested, committed and staged; a
change out of bounds and one failing the tests thrown away; a dirty
checkout left alone; undo.

## Performance Notes

- **Motion detector**: ~5-10ms per frame (frame diff + tracking)
- **Dialog/panel detection**: ~2-3ms per frame (geometry-only, no OCR)
- **HUD detection with OCR**: ~150-250ms per frame (mostly Tesseract subprocess)
- **Memory**: One frame stored (motion detector baseline), no unbounded buffers

For 1366×767 @ 50 FPS, the system is designed to process one full frame per captured frame without accumulating latency.

## Configuration

All runtime settings are in `src/config.rs`:

```rust
use ms::config::{AppConfig, get_global, set_global};

let mut config = AppConfig::default();
config.save_dir = "out".into();  // Change output directory
set_global(config);
```

## Logging

Initialize structured logging early:

```rust
use ms::logging::init_tracing;

init_tracing("debug");  // or "info", "warn", "error"
```

Respects `RUST_LOG` environment variable.

## Extending with New Detectors

1. Create `src/vision/detectors/my_detector.rs`
2. Define input type and output type
3. Implement `detect(&self, image: &RgbaImage) -> Detection<Output>`
4. Use shared helpers from `crate::vision::geometry` and `crate::vision::temporal`
5. Add comprehensive unit tests
6. Declare in `src/vision/detectors/mod.rs`
7. Add to `PerceptionPipeline` in `src/vision/snapshot.rs`
8. Document in [vision-architecture.md](vision-architecture.md)

## Integration with Game State AI

Downstream AI modules can consume `WorldState`:

```rust
pub fn decide_next_action(state: &ms::vision::WorldState) -> Action {
    if state.combat_intensity.value.map(|c| c.intensity) == CombatIntensity::Heavy {
        return Action::Defensive;
    }
    
    if state.dialog.is_present() {
        return Action::HandleDialog(state.dialog.value.unwrap().kind);
    }
    
    // ... continue with other state checks
}
```

The perception pipeline is designed to be the single source of truth for what the AI "sees" on screen, with all observations carrying confidence and reliability metadata for grounded decision-making.

## References

- Detailed design: [vision-architecture.md](vision-architecture.md)
- HUD detection tests: [tests/hp_bar_integration.rs](../tests/hp_bar_integration.rs)
- API documentation: Doc comments in each `src/` module

