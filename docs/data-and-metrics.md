# Data and metrics

> **Not legal advice.** This page says what the code does and lists the questions the owner needs a
> lawyer to answer before anything is sold or sent. It is written by engineers, not lawyers.

MapleSyrup keeps two things apart:

1. **Stats** — numbers about each session, kept on the player's PC for MapleSyrup's own product
   metrics (how long people play, how often warnings come, how fast replies are, what breaks).
2. **Sharing** — the same numbers, coarsened and made ready to share with partners, who may buy
   them. **Off unless the player turns it on.** Separate from the stats, withdrawable at any time,
   with "Delete it".

**Nothing is sent anywhere today.** There is no endpoint. The code writes files on the player's PC
and stops there; an upload is a documented `TODO(upload)` in `src/metrics.rs`, not code.

## What is collected, and why

One record per session (`SessionStats` in `src/metrics.rs`), from what the companion already
works out while it runs: aggregates and game facts only.

| Field | What it is | Why |
|---|---|---|
| `day` | the day the session started (no time) | sessions per day, retention |
| `minutes`, `game_minutes` | minutes MapleSyrup ran; minutes the game was seen | engagement |
| `levels_gained`, `level_start`, `level_end` (+ `_band`) | levels, and their bands — of the last character played (see `characters`) | who plays it, how far |
| `characters` | how many characters the levels followed in the session (0: no level was read, or a record kept before there was this count) | so that two characters in one session never read as "167 → 9" |
| `job` | the class: a name from a closed list ("Night Lord"; see below), or `other` | who plays it |
| `hud` | `modern` or `classic` (Classic World) | which HUD reader to improve |
| `deaths`, `warnings` (`hp_low`, `mp_low`, `beating`, `taught`), `close_calls` | counts | are the warnings useful, too many |
| `potions_answered` | share of low-HP/MP warnings a potion answered | are the warnings heeded |
| `exp_per_hour` | EXP per hour, in percent of a level | progress |
| `maps` | the game's map names, and how many times each was come to | which content |
| `sentences`, `replies`, `reply_ms_median`, `instant_answers` | how many sentences the player said (never what), replies heard and the median time to their first words, answers given without a model | conversation quality and speed |
| `call_minutes`, `clip_minutes` | minutes on a live call; minutes with the phone connected without one | which mode people use (and its cost) |
| `coach_looks`, `coach_lines` | the coach's looks at the game and the lines it said | coaching quality and cost |
| `attitude` | friendly, blunt or savage | which tone people pick |
| `language` | the phone's language, without the region (`he`, not `he-IL`) | which languages to support |
| `version`, `commit` | the app's version and build | what breaks where |
| `windows`, `screen` | Windows major version (`10`, `11`); the screen's size class (`1080p`, `1440p`, `4K`…) | what to test on |
| `ai_errors`, `voice_errors` | how many model and voice failures | reliability |
| `session` | a random id for the session, so its snapshot is replaced, not counted twice | bookkeeping |

**Never collected:** the character's name; the player's words, voice or any transcript; a
screenshot, a crop or any picture; the notebook (`memory.json`, `knowledge.json`) or anything the
player taught; file paths (the Windows user name is in them); the PC's name; keys; IP addresses;
the phone's browser (user agent). The hooks the main loop calls take no text of the player's at
all. A test builds a session whose inputs hold a name, a transcript and a path with a Hebrew
Windows user name in it, and checks that none of them reaches a record, an export or a file.

**The class is never free text.** What the screen shows beside the class, or what the player says
it is, can hold anything: the character's name, a guild tag, a chat line, a place, a girlfriend's
account. So a record, its file and the export hold only a name from a closed list — `CLASSES` in
`src/metrics.rs`: MapleStory's classes and their advancements, modern and Classic World (Beginner;
Warrior, Fighter, Crusader, Hero; Page, White Knight, Paladin; Spearman, Dragon Knight or
Berserker, Dark Knight; Magician, Wizard, Mage and Arch Mage, with F/P and I/L; Cleric, Priest,
Bishop; Bowman, Hunter, Ranger, Bowmaster; Crossbowman, Sniper, Marksman; Pathfinder; Thief,
Assassin, Hermit, Night Lord; Bandit, Chief Bandit, Shadower; Dual Blade and its five
advancements; Pirate, Brawler, Marauder, Buccaneer; Gunslinger, Outlaw, Corsair; Cannoneer and its
advancements; Jett; the Cygnus Knights, the Heroes, the Resistance, Nova, Flora, Anima, and the
rest) — or `other`. A text is matched whole, case, spaces, hyphens and the kind of apostrophe aside;
each class also has its other spellings (an older name such as "Crossbow Master", the elements
spelled out, the players' short forms such as "NL") and its Hebrew transliterations ("נייט לורד" is
"Night Lord"). "Night Lord [Guild: …]" or "Night Lord" with the name beside it is `other`, not the
text. A class read before is not replaced by `other` (a misread after it, the player's words).
Records kept before the list are shown, shared and written again with their class made a name or
`other`.

**A map's name** is kept on this PC only (the export has how many), when it looks like the game's:
no path — a backslash (`C:\…`), a `/` or `~` first, or a `/` with no space on either side
(`Users/me`); the game's own " / ", as in "Victoria Road / Ellinia", is a map — no `@`, no link,
not too long, and not containing the character's name.

**Two characters in one session** (a main, then an alt): a session is one run of MapleSyrup — its
minutes, words, replies and errors are the run's — but its levels and its class are a character's.
Another character (a level taken with another name than the last one's, or a lower level: what the
companion itself calls "another character") starts the levels and the class over, and `characters`
counts them: the record is the last character's levels and class, never "167 → 9" of two. A new
record per character was the other choice; it would count one evening as two sessions (skewing
sessions per day and minutes per session) and split its minutes, words and errors at a moment the
game does not mark (the level is taken a few seconds after the switch).

## Where it is kept

All under the settings folder, `%APPDATA%\MapleSyrup\metrics\`:

| File | What | When |
|---|---|---|
| `sessions.jsonl` | one record per line, the newest last | at each session's end |
| `current-<session>.json` | the session under way — its own file, so that two copies of MapleSyrup running at once never write over each other's | every minute; it joins `sessions.jsonl` when the session ends, or — after a crash or a console window closed with the X, which skips the program's own end — at the next start (each start takes every snapshot but its own; one from before there was one per session, `current.json`, too). It is deleted only once its record has landed: when `sessions.jsonl` cannot be written, it is left, as the session ended, and the next start keeps it |
| `share.json` | `{"on": true, "since": "2026-10-09", "id": "<uuid>"}` | only while sharing is on: written when it is turned on, deleted when it is turned off (no file is off) |
| `share-export.json` | what would be shared | only while sharing is on: built when it is turned on, rebuilt at each session's end |
| `*.partial` | a file being written (each is written beside its place, then put there) | for a moment; a write that fails deletes its own, and "Delete it" deletes any |

**Retention:** the last 365 sessions; older ones are dropped. The export holds the sessions since
the day sharing was turned on, and is deleted when it is turned off. Deleting the `metrics` folder
by hand is safe: the stats start again. A line of `sessions.jsonl` that cannot be read (cut off by
a power cut) costs that line, not the others; a `sessions.jsonl` that cannot be read at all is
never written over.

## The consent model

- **Separate.** The stats are the product's own and stay on the PC. Sharing is a second, separate
  choice, in its own card at the end of Settings on the phone.
- **Opt-in, off by default.** Nothing is made ready to share until the player turns the toggle on.
  The card says plainly what is shared, that it may be sold to companies that study gamers, and
  what is never shared. When MapleSyrup starts with sharing on, the console says so in one line.
- **Adults only.** The toggle stays greyed out until the player ticks "I'm 18 or older" above it,
  and the PC refuses to turn sharing on without that confirmation (`POST /api/share` answers 400
  to an `on` without `adult: true`), so an old page or a script cannot skip it. The confirmation is
  asked each time sharing is turned on and is not kept. Turning sharing off never needs it.
- **See before and after.** "See what would be shared" shows the export as JSON. While sharing is
  off it shows a preview built from the last sessions, with no id (made and written nowhere), so
  the player sees exactly what they would be agreeing to.
- **Only from consent on.** The export holds the sessions from the day sharing was turned on.
- **Withdrawable, and deletable — never silently failing.** Turning it off — or "Delete it" —
  deletes the choice (`share.json`: no file is off), the export with the install id, and any
  half-written file (`*.partial`, which may hold the id), then looks: if anything is still there
  (another program holding a file, on Windows an antivirus or a backup tool), the PC answers 500
  with the code `not_deleted` and the files left, and the phone says so — "Not deleted: a file of
  it is still on the PC…", in the page's own language — and shows the toggle as the PC has it. It
  says "Deleted" only on the PC's ok. "Delete it" again tries again; while sharing is off, an export
  left behind is also deleted at the next session's end. Turned on again, a new id is made,
  unlinkable to the old one.
- **The phone's words, not the PC's.** What goes wrong is answered as a code (`adult_only`,
  `bad_request`, `no_stats`, `no_id`, `not_saved`, `not_deleted`); the page has the words for each,
  in English and Hebrew (other languages fall back to English), so a Hebrew page never shows an
  English sentence.
- **A random install id.** A UUID v4 from the operating system's randomness (through `ring`,
  already a dependency), made only when sharing is turned on. It identifies an installation, not a
  person, and lets a future server delete one player's records on request (see below).

## The allow-list

What the export may contain — `EXPORT_FIELDS` in `src/metrics.rs`; a test holds every export to it,
and another every record to `RECORD_FIELDS`:

`format`, `install_id`, `app_version`, and per session: `week` (ISO week, not the day),
`minutes`, `game_minutes`, `levels_gained`, `level_start_band`, `level_end_band` (1–10, 11–30,
31–60, 61–100, 101–140, 141–200, 201+; never the level), `characters` (a count), `job` (a name
from the closed list, or `other`), `hud`, `deaths`, `warnings`
(`hp_low`, `mp_low`, `beating`, `taught`), `close_calls`, `potions_answered` (to a tenth),
`exp_per_hour` (to a tenth), `maps` and `map_visits` (counts: never which maps), `sentences`,
`replies`, `reply_ms_median` (to a tenth of a second), `instant_answers`, `call_minutes`,
`clip_minutes`, `coach_looks`, `coach_lines`, `attitude`, `language`, `version`, `windows`,
`screen`, `ai_errors`, `voice_errors`.

Kept off the export even though the record has them: the day, the exact levels, the maps' names,
the session id and the commit.

## Aggregation before any sale

The export is per installation, under a persistent id: **pseudonymous, not anonymous** (in GDPR's
terms it is still personal data). What is sold should not be the export, but aggregates made from
many exports:

- a **k-anonymity floor**: a figure is sold only when it is computed over **at least 50 players**
  (distinct install ids) — never a single player's records, never a small group's;
- no install ids, no per-session rows, in anything sold;
- cells under the floor suppressed or merged (a rare class on a rare map in one week is one
  player);
- coarser time (a month rather than a week) where a figure would otherwise fall under the floor.

The toggle reads "Share my play stats (no name, words, voice or screen) with MapleSyrup's
partners" — not "anonymous": the export is pseudonymous, and calling it anonymous would promise
more than it is. Whether the aggregates sold from it may be called anonymous is a question for the
lawyer.

## What needs the owner and a lawyer before anything is sent or sold

1. **An endpoint**, on `datta-syrup.ai`: HTTPS only; what it stores and for how long; who can read
   it; who operates it. Uploads only while sharing is on, only the allow-listed fields.
2. **Deletion upstream.** Once uploads exist, "Delete it" must first ask the server to delete the
   records under the install id, then delete the id locally (today there is nothing remote to
   delete).
3. **A privacy policy and updated terms** that say what is collected, why, for how long, that it may
   be sold, to which kinds of buyers, and how to withdraw and delete — linked from the card.
4. **Israel's Privacy Protection Law, 5741-1981, and its Amendment 13** (in force since August
   2025): the duty to inform when collecting; whether consent through the toggle is enough for a
   sale; whether a database whose purpose includes passing data on to others must be registered
   or notified (Amendment 13 narrowed registration, but databases kept to pass information on to
   others are among those that may still need it); whether a privacy protection officer is needed;
   data security duties.
5. **GDPR, for players in the EU**: consent as the lawful basis (freely given, specific, informed,
   as easy to withdraw as to give); the right to erasure; a representative in the EU (Article 27)
   for a controller outside it; transfers of data out of the EU.
6. **Age.** MapleStory has young players. Sharing can be turned on only after the player says they
   are 18 or older (see the consent model), but that is a self-declaration: whether it is enough —
   or whether a stronger check is needed before any data about a player is sold, given GDPR's
   Article 8 (13 to 16 by country) and Israeli law on minors — needs the lawyer's answer.
7. **Agreements with buyers**: a data processing or data sharing agreement — the floor above, no
   attempt to re-identify, no onward sale, deletion on request, security.
8. **A register of databases**, if the lawyer finds one is required.

## In the code

- `src/metrics.rs` — the record, the export, the allow-lists, the class list (`CLASSES`), the
  files, the consent; its tests.
- `src/bin/maplesyrup/main.rs` — one-line hooks where things happen (a frame, a sentence, a
  reply's first words, a failure, every quarter second for the phone and the map), the record
  written every minute and at the end, and the console line.
- `src/phone/mod.rs` — `GET /api/stats`, `GET /api/share`, `POST /api/share {on, adult}`,
  `POST /api/share/delete`; a change answers `{"ok": true, "share": …}`, or an error code (500
  `{"error": "not_deleted", "left": ["share-export.json"]}` when a file is still there).
- `src/phone/page.html` — the card at the end of Settings (its words for each error code), and the
  last sessions in Details ("Levels up": the levels gained, not the level).
