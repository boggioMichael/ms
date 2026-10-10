# CURRENT_STATE_AUDIT — MapleSyrup (`boggioMichael/ms`) and Syrup (`boggioMichael/syrup`)

> **This branch's base point.** `claude/data-program` was made on 2026-10-10 from `claude/human` at
> **f9b6047** (PR #19's head: a superset of the audited **03303a9** plus five commits made after the audit —
> e74aeb7 and cdc11fd (a fill sliver is no reading; the stats' cross-process lock; the reply filters),
> a351a9f (sharing *not offered* until an approved basis — this audit's §5), f9b6047 (the Privacy text
> corrected from this audit's §4.3) — 514 tests passing, CI green on cdc11fd), then **merged with
> `origin/master` 6d36a0e** (docs only: `docs/launch-plan/`, no overlapping path). Syrup stays pinned at
> **54dad1e** (`claude/engine`). Nothing was merged wholesale and no existing work was replaced; the data
> program's code lives behind its own module on this branch. The audit below was written against 03303a9;
> where a later commit changed a finding it says so in a note.


Auditor: w42 (read-only). Date: 2026-10-10, written 05:37–05:56 UTC (hard stop 07:05).
Method: ms read only through git objects (`git show`, `git log`, `git diff`, and `git archive 03303a9` into
`/home/claude/audit-src/ms-human/` for grepping); nothing in either repository was changed, built or switched.
Syrup archived from `54dad1e` into `/home/claude/audit-src/syrup-engine/`, built with
`CARGO_TARGET_DIR=/home/claude/audit-target`. Every claim below cites a commit, a file:line at a named commit,
a CI run id, or a test run. "Unverified" means exactly that.

Legend for the table: **WORKS (tested)** = a test that ran, or a CI run, exercises it ·
**EXISTS (code, untested here)** = code is present and wired, but no test/run I could see proves it works ·
**DOCUMENTED ONLY** = text claims it, no code found · **MISSING** = neither.

---

## 0. Recommended base point (summary)

- **MapleSyrup: branch from `03303a9` (`claude/human`)** — the commit, not the moving branch. It is a strict
  superset of every other code branch (`claude/maplesyrup-standalone`, `claude/syrup-engine`,
  `claude/hud-lines`: 0 commits each that `claude/human` lacks). The only things it lacks are the 6 docs-only
  commits on `origin/master` (16 new files under `docs/launch-plan/`, 1,263 lines, zero path overlap with
  `claude/human`'s changes) and the 3 stale Sept commits on `feature/ui` (Yohai, PR #8 into
  `llm-knowledge-architecture`, a one-commit proposal branch; 128 commits behind). Evidence: manager ran the ms suite at `03303a9` this
  morning, **505/505 passed** (manager's run, cited, not re-run by me); CI on `03303a9` is **green on both
  workflows** (run 38027967314 "CI" success; run 38027963441 "MapleSyrup standalone" success, incl. Tests,
  Self-test, Recording test, Installer test); its parent `b28cdfb` is green too (38003325059, 38003321404). Merge `origin/master` (6d36a0e) into the new branch first — a docs-only,
  conflict-free catch-up by path analysis (no merge was executed).
  - Why not `master`: `origin/master` is **37 commits behind** `claude/human` (27,604 inserted lines in 36
    files: the two days of fixes and the play stats `src/metrics.rs` are only on `claude/human`/PR #19).
  - Caveat: two workers are committing to `claude/human` right now; anything after `03303a9` is not audited.
- **Syrup: `54dad1e` (`claude/engine`)** — the rev ms pins in `Cargo.toml:23` on all three of `origin/master`,
  `claude/human`, `claude/hud-lines`. Syrup's branches are one linear stack; `claude/engine` contains every
  commit of every other branch, and **`main` is 129 commits behind it**. None of the stack is merged (PRs #1→#2→#3
  are all open, chained `intents-2 → main`, `eitan-runtime → intents-2`, `engine → eitan-runtime`).

### Top findings (details and evidence in the sections below)

1. **Both default branches are stale.** ms `master` lacks 37 commits of code (incl. the play stats, Classic
   World HUD, two days of fixes) that live only on unmerged PR #19; syrup `main` lacks 129 commits — the whole
   engine ms depends on sits on an unmerged three-PR stack. (§1)
2. **More leaves the PC than the Privacy text says.** *(Since f9b6047 the Privacy text in `package/README.txt` and `README.md` says all of this.)* The notebook (`memory.json` facts, `about-me.txt`,
   `knowledge.json` lessons) is sent with **every** reply to OpenAI or xAI, with a game frame; the updater calls
   datta-syrup.ai hourly **by default** even with no key ("Without a key, nothing leaves your PC" is false);
   outside a call, the phone browser's speech recognition sends the voice to Google/Apple; the workshop sends
   session-log excerpts to the coding agent's vendor; the "phone over the internet" mode relays mic audio through
   Cloudflare. None of these is in "Privacy". Recordings hold the whole screen and all PC sound. (§4)
3. **The play-stats consent does not meet the new rules.** *(Since a351a9f: sharing is not offered at all — the PC refuses an "on", withdraws an earlier choice at start, and the card says why; the purpose-based model replaces it in this program.)* One boolean; a date, not a receipt; no text version,
   source or scope; the 18+ confirmation is not kept; withdrawal deletes the only consent record; no purposes;
   no per-title rights manifest; no approved-basis gate (no upload exists yet — the one thing in its favour,
   with the allow-lists and verified deletion). (§5)
4. **Product boundaries hold in source.** Zero hits in either repo for game-memory reading, injection, hooks,
   input synthesis or keylogging. Disclose: it lowers the game's volume through the audio session, and "the game"
   is any window whose title contains "maplestory" — a non-game window so titled would be captured and sent. (§3, §4.3)
5. **Episodes cannot be built from today's data.** No structured goals, no action provenance (impossible by
   design without input capture or self-report), no persisted per-frame state, no advice↔outcome links, no
   build/world detection; logs are free text with local wall-clock seconds. The existing local files
   (`log.txt`, `mic.wav`, recordings, `memory.json`) must not be backfilled into the program. (§6)
6. Syrup the library is clean of services (no analytics, no game, no user DB), but its README overclaims:
   "no model files" (syrup-runtime bundles YuNet ONNX by default), "pinned to a tag" (no tags exist), "MapleSyrup
   has no vision code of its own" (8,899 lines remain), performance tables untested in CI; native-intent mode
   needs crates.io and a Rust toolchain at runtime. ms README says "Download from Releases" — there are none. (§2)

---

## 1. Branches

### 1.1 MapleSyrup (`boggioMichael/ms`)

Refs after `git fetch origin` (05:37Z). Local `master` is stale (9b4fe23, "behind 93"); all numbers use
`origin/master` = `6d36a0e`.

| Branch | Head | Date (+0300) | Last subject |
|---|---|---|---|
| `origin/master` | 6d36a0e | 2026-10-09 00:28 | docs(launch-plan): link each task to its issue (#21-#35) |
| `claude/maplesyrup-standalone` | 8fa0187 | 2026-10-03 08:51 | feat: its own voice (ElevenLabs) and the dog comes alive |
| `claude/syrup-engine` | 9cd0669 | 2026-10-04 14:48 | fix(companion, sight): only number-backed bars from the detector… |
| `claude/hud-lines` | e2db1f5 | 2026-10-08 10:43 | fix(sight): a line that will not learn says why… |
| `claude/human` | 03303a9 | 2026-10-10 08:34 | fix: a cursor resting on a bar is no reading — MP and close calls too… |
| `origin/feature/ui` | b0f0099 | 2026-09-16 07:53 | feat(ui): add animated voice companion overlay (Yohai Simhony) |

`git rev-list --count A..B` (row A, column B = commits in B that A lacks):

| A \ B | master | standalone | syrup-engine | hud-lines | human | feature/ui |
|---|---|---|---|---|---|---|
| master | 0 | 0 | 0 | 1 | **37** | 3 |
| standalone | 64 | 0 | 57 | 59 | 95 | 3 |
| syrup-engine | 7 | 0 | 0 | 2 | 38 | 3 |
| hud-lines | 6 | 0 | 0 | 0 | 36 | 3 |
| human | **6** | 0 | 0 | 0 | 0 | 3 |
| feature/ui | 97 | 33 | 90 | 92 | 128 | 0 |

What each holds that others do not:
- **`claude/human` (03303a9)**: 37 commits not in master (e2db1f5 … 03303a9, 2026-10-08 10:43 → 2026-10-10
  08:34). Those that matter for the data program: `72c97ff` "feat: play stats on the PC; sharing them only by
  opt-in, 18+, with see and delete"; `09274f8` "fix(stats): no free text in the export; withdrawal never fails
  silently; no session lost"; `4288187` "feat(sight): the Classic World HUD's numbers, read in the game's own
  font"; `abb5316` "tools: examples/evening.rs — one evening, replayed and printed"; `672300a`
  "fix(companion): … corrections kept"; plus ~25 companion/voice/phone behaviour fixes (Hebrew lines, warning
  lane, live-call fixes). Open as **PR #19** into master.
- **`claude/hud-lines` (e2db1f5)**: 1 commit not in master (`src/sight/numbers.rs`, +101/−4). Fully contained
  in `claude/human`. Open as PR #18 — redundant once #19 merges.
- **`claude/syrup-engine` (9cd0669)**: nothing that master lacks (merged as PR #17, 2026-10-05).
- **`claude/maplesyrup-standalone` (8fa0187)**: nothing that master lacks (merged as PRs #11–#16, 2026-10-02/03).
- **`origin/master` (6d36a0e)**: 6 commits not in `claude/human`, all `docs(launch-plan)` (PR #20) — 16 files
  under `docs/launch-plan/`, no overlap with the 36 files `claude/human` changed since merge-base `3748d58`.
- **`feature/ui` (b0f0099)**: 3 commits (7dd344b "Document proposed LLM & knowledge architecture", 7e4c6b3
  "Save initial AI agent and terminal chat foundation", b0f0099 "animated voice companion overlay"), Yohai,
  2026-09-09…16; PR #8 open into `llm-knowledge-architecture`; 128 commits behind `claude/human`. Stale; not a
  candidate.
- Other remote refs are CI artefacts, not work: `build-output/claude/human/{linux,windows}` (built from
  `d78108d`, i.e. stale relative to 03303a9), `build-output/claude/syrup-engine/windows`, `bo-windows`,
  `bo-master-windows`, `build-win`, `out/*`.
- **Branches not in the local refspec** (seen with `git ls-remote origin`, objects fetched without creating refs;
  "notInHuman" = `git rev-list --count 03303a9..<sha>`): none holds recent unmerged work.
  `docs/launch-plan` 5b87449 (2026-10-09) — 4 not in human, 0 not in master (merged as #20).
  `demo/real-recording` 1134511 (2026-09-30) — 7 not in master: the pre-squash history of PR #10 (merged).
  `demo/real-recording-output` 997ef8a — CI bot output (run 36643081386), the evidence the README cites.
  `vision-framework` 77d7d3c (2026-08-04, 2 commits), `llm-knowledge-architecture` 7dd344b (2026-09-09, Yohai, 1
  commit: a proposal doc), `agents/internal-data-model-design`, `agents/llm-platform-architecture-design`,
  `agents/maplesyrup-capture-framework-setup`, `agents/tbi-readme-api-design-integration` (2026-08-05, 1 commit
  each: early design docs/scaffolds). Fully merged: `agents/debugging-toolkit-for-vision-systems`,
  `agents/overlay-architecture-development`, `agents/perception-architecture-redesign-maplestory`,
  `agents/vision-framework-architecture`, `agents/vision-system-expansion`, `cleanup/remove-fabricated-docs`,
  `copilot/*` (both = initial commit), `mvp`, `poc`. The August `agents/*` one-commit branches were not read.

Merge history of `origin/master` (first-parent): 1984acb initial (2026-08-12) → #1/#3/#4 agents/vision
(2026-08-11) → #7 mvp (2026-09-28) → #9 "remove role-play team docs and unsupported benchmark claims"
(2026-09-30) → #10 recorded runs + Windows CI → #11 0.4.0 "first working version" → #12 0.5.0 live call +
session recording → #13 Hebrew user folder → #14 0.6.0 learns as you play → #15 0.7.0 → #16 0.8.0 ElevenLabs
voice → #17 syrup engine (2026-10-05) → #20 launch-plan docs (2026-10-09).

CI per head (`gh api repos/boggioMichael/ms/actions/runs?branch=…`): master 6d36a0e success (37847084767 CI,
37847084686 standalone); claude/human 03303a9 **success** (38027967314 CI, completed 05:42Z; 38027963441
standalone, completed 05:40Z — was in_progress when first queried at 05:38Z), b28cdfb success (38003325059,
38003321404), 72c97ff success (37965979046, 37965973949); hud-lines e2db1f5
success (37745579483, 37745558064); syrup-engine 9cd0669 success (37200000868, 37199997495); standalone
8fa0187 success (37101345234, 37101177060).

**Most complete implementation: `claude/human` @ 03303a9.** `master` is behind by 37 commits of code.

### 1.2 Syrup (`boggioMichael/syrup`)

| Branch | Head | Date | Last subject |
|---|---|---|---|
| `main` | 265ade5 | 2026-08-17 | chore: rename the crate to syrup (Michael Boggio) |
| `claude/cv-engine` | 6c6b041 | 2026-09-27 | feat(glyphs): min_gap keeps segmented digits whole |
| `claude/intents` | b39695d | 2026-09-27 | fix(intent): let native calls run concurrently… |
| `claude/intents-2` | e694e80 | 2026-09-29 | fix(minesweeper-coach): coach.cmd tries the gnullvm toolchain… |
| `claude/eitan-runtime` | 8995d89 | 2026-10-02 | docs: compiled operations in the README and the changelog |
| `claude/engine` | 54dad1e | 2026-10-04 | fix(glyphs): one font at two sizes keeps a template per size |

| A \ B | main | intents | intents-2 | eitan-rt | engine | cv-engine |
|---|---|---|---|---|---|---|
| main | 0 | 24 | 99 | 106 | **129** | 20 |
| intents | 0 | 0 | 75 | 82 | 105 | 0 |
| intents-2 | 0 | 0 | 0 | 7 | 30 | 0 |
| eitan-rt | 0 | 0 | 0 | 0 | 23 | 0 |
| engine | 0 | 0 | 0 | 0 | 0 | 0 |
| cv-engine | 0 | 4 | 79 | 86 | 109 | 0 |

Linear: `main ⊂ cv-engine ⊂ intents ⊂ intents-2 ⊂ eitan-runtime ⊂ engine`. Nothing has been merged to
`main` since 2026-08-17. PRs (REST): #1 `claude/intents-2 → main` open; #2 `claude/eitan-runtime →
claude/intents-2` open; #3 `claude/engine → claude/eitan-runtime` open. **`main` is behind by 129 commits and
does not contain the engine ms depends on.** Other remote refs: `build-cache/vendor`, `build-output/claude/engine/*`,
`gh-pages`, `remote-run*`, `review-logs/*`, `review/eitan-verify`, `review/port-verify` (not audited).

CI (`actions/runs?branch=…`): engine 54dad1e success (37153606097, 37153602543); eitan-runtime 8995d89 success
(36990508846; an earlier 9d7126b failed, 36987677238); intents-2 e694e80 success (36490482032); main 265ade5
success (32070370416, 2026-08-17); `claude/intents`, `claude/cv-engine`: no runs returned.

---

## 2. The table — exists / works / documented only / missing

**Test evidence used** (all "WORKS" below means *passes tests on fixtures/fakes*, never "verified against the live
game or live providers"):
- **ms@03303a9**: manager's run this morning, **505/505 passed** (cited, not re-run by me). CI on 03303a9:
  run **38027967314 "CI" success** (windows-latest: fmt, clippy `-D warnings`, `cargo test --release`, with
  Tesseract installed) and run **38027963441 "MapleSyrup standalone" success** (windows job: Format, Clippy,
  Build, **Tests**, **Self-test of the built program**, **Recording test (fetches ffmpeg the way a first
  recording does)**, Package, Installer, **Installer test (install, run, uninstall)**, Publish — all `success`;
  linux job "ffmpeg and Xvfb (recording tests)" + Checks `success`; `release` job skipped). Per-test names with
  results exist only for **d78108d** (an earlier `claude/human` commit): `build-output/claude/human/windows:logs/test.log`
  — **404 ok, 0 failed** (lib 344, maplesyrup bin 11, `eleven_fake` 5, `hp_bar_integration` 1, `hud_accuracy` 4,
  `openai_fake` 28, `phone_link` 4, `probe` 1, `syrup_parity` 1, `update_channel` 2, `workshop_pipeline` 3).
  Test names below are `fn` names present at 03303a9 (file:line), hence in the 505 run.
- **syrup@54dad1e**: CI runs **37153606097** and **37153602543** success — jobs `check` on ubuntu/windows/macos
  (`cargo clippy --workspace -D warnings`, `cargo test --workspace`, `cargo test -p syrup --lib capture::tests`),
  `wayland`, `python` ×3 OS (`pytest python/tests`), `minesweeper-coach` ×2, `example`, `recipes` — all success.
  **My own run** (archive of 54dad1e, `cargo test --release --offline`): the vendored crates do **not** cover
  the workspace (`rqrr` missing for `crates/syrup-runtime`; `clap 4.6.6` locked but `4.6.5` vendored), so I
  excluded `syrup-runtime` from the workspace and let cargo re-resolve the lockfile against the vendor dir —
  i.e. **not the exact locked dependency set**. Result: see §2.3.

### 2.1 MapleSyrup (ms@03303a9)

| Capability | Status | Evidence |
|---|---|---|
| Capture — game window | **WORKS (tested)** for window choice; capture itself via syrup | `src/capture.rs` (241 lines) tests `the_exact_title_wins_over_lookalikes_above_it` :225, `a_decorated_title_is_taken_when_nothing_is_exact` :231, `no_game_no_pick` :237. Pixel capture is syrup's (`syrup/src/capture/{windows,x11,portal,pipewire,macos}.rs`), tested in syrup CI `capture::tests` on 3 OSes. ms CI self-test (Windows, d78108d, `build-output/claude/human/windows:logs/self-test.log`): "capture: 3 windows listed, no MapleStory among them" — enumeration only, no game frame captured in CI. Against real MapleStory: **not verifiable here**. |
| Capture — full screen | **EXISTS, recorder only** | Perception captures the game window only. The recorder grabs **the whole primary display with the cursor**: `src/app/recorder.rs:363-377` (`ddagrab=output_idx=0…draw_mouse=1`, fallback `gdigrab -i desktop`). |
| Capture — recording | **WORKS (tested, CI) — GDI path only** | `src/app/recorder.rs` (1,687 lines), 9 tests e.g. `ffmpeg_is_told_to_record_the_screen_and_the_sound_on_one_clock` :1455; CI 38027963441 step "Recording test (fetches ffmpeg…)" success; linux "ffmpeg and Xvfb (recording tests)" success. The d78108d log (`…/windows:logs/record-test.log`) shows what that means: "recording: the screen through GDI, **without the PC's sound** (no speakers…)", 149 frames at 29.3/s, "in sync: the tone +39 ms from the flash", also in a Hebrew-named folder; **the GPU `ddagrab` path failed on the runner** ("Failed to create D3D11VA device"). GPU capture and loopback sound: untested in CI. |
| Perception — HUD bars | **WORKS (tested on fixtures)** | `tests/hud_accuracy.rs`: `hp_bar_reads_full_on_a_400_of_400_capture` :36, `mp_bar_reads_nearly_full_on_a_1291_of_1351_capture` :46, `exp_bar_reads_about_a_third_full` :56, `bars_are_not_all_reported_as_identical` :66; `tests/hp_bar_integration.rs::finds_hp_bar_in_resource_photo`; `sight::the_hud_is_found_from_the_pixels_alone` (`src/sight/mod.rs:1576`); `tests/syrup_parity.rs::the_fixture_reads_as_it_did_before_the_migration`. CI self-test (d78108d): "vision: HP 100%, MP 95%, EXP 38% found on the screenshot in 48 ms (0 of 3 backed by a number)" and the companion then answers "I can see the game, but not your HP, MP or EXP bars" — by design since `9cd0669` (only number-backed bars are believed), but it shows bars alone do not reach the player. |
| Perception — numbers (game font) | **WORKS (tested on fixtures)** | `src/sight/numbers.rs` 9 tests (e.g. `the_font_is_learned_from_labels_read_every_frame_and_kept` :1451, `a_line_that_will_not_learn_says_the_most_telling_reason_and_keeps_its_picture` :1424); `tests/classic_hud.rs` 5 tests (`every_label_the_teacher_gives_for_the_classic_hud_is_learned_and_read_every_frame` :174, `one_font_learns_both_huds_in_either_order_and_reads_each` :276). Classic World HUD added in `4288187` (2026-10-09). |
| Perception — OCR | **WORKS (tested), not on the companion path** | `src/vision/hud_ocr.rs` 12 tests (`provenance_traces_a_value_back_to…`, `carried_forward_values_report_their_origin_frame` :420, `absent_text_is_distinguished_from_invalid_text` :413). `src/vision/snapshot.rs` `Detectors.hud_text`: "for the preview; the companion reads the numbers in the game's font instead (`sight::numbers`)". |
| Perception — learned sight | **WORKS (tested on fixtures)** | `src/sight/mod.rs` (2,177 lines) 14 tests (`the_hud_is_learned_measured_kept_and_checked` :1476, `a_layout_lacking_both_bars_gets_them_as_a_pair_and_asks_the_model_only_in_time` :1713); `src/sight/things.rs` 5 tests (`an_object_is_taught_once_and_found_again_and_alerts` :1216). Stored under `<settings>/learned/` (`src/ai/tools.rs:483`). |
| Perception — teacher model | **EXISTS; request/answer shaping tested; live accuracy unverified** | `src/sight/teacher.rs` (480 lines) sends frame/crops to an OpenAI vision model (`:188-270`); tests `answers_are_read_and_checked` :392, `questions_carry_pictures_and_a_strict_schema` :430 (no live model). |
| `src/game_state.rs` / `GameState` | **EXISTS, demo-only (not on the live path)** | Only user: `src/bin/demo_video.rs:1` (`use ms::game_state::GameState`). Tests `serializes_to_json` :471, `display_string_is_readable` :559. |
| `WorldState` | **WORKS (live path)** | `src/vision/snapshot.rs:25` (fields `hud`, `motion`, `dialog`, `minimap`, `chat_log`, `icon_row`, `footholds`, `combat_intensity`, each a `Detection<T>`) → `Observation::from_world` (`src/companion/observation.rs:83`) → `src/perceive.rs:20-25`. Comment at snapshot.rs: "The companion consumes the HUD alone". |
| `GameStateV2` (versioned, provenance) | **DOCUMENTED ONLY** | `docs/perception-architecture-redesign.md:141` ("GameStateV2 fully replaces…", :209). No code. |
| Confidence / uncertainty values | **WORKS (unit), uncalibrated** | `Detection<T>{value, confidence, source, failure_reason}` (syrup `src/detection.rs:93-101`); ms `src/vision/types.rs`; tests `a_trusted_reading_wins_over_the_bar` (`observation.rs:177`), `an_untrusted_reading_falls_back_to_the_bar_estimate` :193, `a_bar_the_detector_only_guessed_at_is_not_an_observation` :209, `levels_come_only_from_trusted_plausible_reads` :259. **No calibration evidence** (grep for brier/calibration curve/reliability diagram: 0 hits) — confidences are heuristic scores, not probabilities. |
| Notebook — `memory.json` | **WORKS (unit)** | `src/ai/memory.rs:34` (`FILE`); tests `it_is_kept_and_read_back` :1053, `the_notebook_comes_back_whole_and_its_lessons_go_to_knowledge` :951, `what_it_learned_is_told_shown_and_forgotten_on_the_phone` :1066, `it_looks_back_when_there_is_enough_new_talk_without_asking_every_minute` :1147. Updated by a model call (§4.1). |
| Notebook — `knowledge.json` | **WORKS (unit)** | `src/ai/knowledge.rs` tests `something_looked_up_answers_the_same_question_next_time` :324, `a_correction_replaces_what_it_corrects_and_wins` :340, `it_is_kept_between_sessions` :392. |
| Notebook — "remember" | **WORKS (code + unit)** | `remember_fact` tool appends to `about-me.txt` (`src/ai/tools.rs:307-320`); `memory.rs:587` `TOLD`. |
| User corrections | **WORKS (unit)** | `correct_reading` tool (`src/ai/tools.rs:300-306`); `knowledge.rs:340` test; commit `672300a` "corrections kept". |
| Voice — phone link | **WORKS (tested)** | `src/phone/*` (HTTPS on LAN, self-signed cert, QR); `tests/phone_link.rs`: `the_page_and_the_api_work_over_tls` :19, `a_certificate_other_than_ours_is_refused_by_the_pinned_client` :69, `plain_http_on_the_tls_port_is_sent_to_https` :76, `the_local_door_for_a_tunnel_serves_plain_http` :87. Speech-to-text outside a call is the **phone browser's Web Speech API** (`src/phone/page.html:2053`). |
| Voice — live call | **EXISTS; PC side unit-tested; the WebRTC call itself untested in CI** | `src/ai/live.rs` (539 lines): phone↔OpenAI Realtime over WebRTC, PC mints a short-lived key; tests `a_call_is_told_who_it_is_what_it_knows_and_what_was_said` :403, `a_call_takes_functions_only` :529 (instructions/tools only). `tools/phone_ui_check.py` exists (900 lines); not seen in the CI steps. |
| Voice — models | **EXISTS (strings in code), behaviour tested against fakes** | OpenAI text `gpt-6-luna`, `gpt-6.1-sol`, `gpt-5-mini`, `gpt-4.1-mini`, `gpt-4o-mini` (`src/ai/openai.rs:24-28`); TTS `gpt-4o-mini-tts` (:39); realtime `gpt-realtime`, `gpt-realtime-mini`, `gpt-4o-realtime-preview` (`live.rs:38-40`); transcribe `gpt-4o-mini-transcribe` (`live.rs:44`); xAI `grok-4.3` (`src/ai/mod.rs:165`); ElevenLabs `eleven_v4`, `eleven_v4_turbo`, `eleven_flash_v2_5`, `eleven_multilingual_v2` (`eleven.rs:27-29,196`). `tests/openai_fake.rs` 34 tests, `tests/eleven_fake.rs` 5 tests — fake servers; I did not verify these model names exist at the providers. |
| Local recordings | **WORKS (tested, CI)** | see "Capture — recording"; files in `MapleSyrup sessions\…` (`package/README.txt:269-274`). |
| Play stats (`src/metrics.rs`) | **WORKS (unit), local only** | 2,555 lines; 17 tests incl. `a_record_and_its_export_never_hold_the_name_the_words_or_a_path` :1677, `every_record_and_every_export_keeps_to_its_allow_list` :1775, `sharing_is_off_until_turned_on_and_off_deletes_the_export_and_the_id` :1851, `turning_sharing_off_deletes_every_file_of_it_and_never_fails_silently` :2075, `the_class_is_a_name_from_the_list_or_other_never_the_words` :2319; phone: `the_stats_and_the_sharing_choice_answer_from_the_pc_and_sharing_starts_off` (`src/phone/mod.rs:1544`). Commits `72c97ff`, `09274f8` — **only on `claude/human`**, not in master. |
| Play stats — upload/endpoint | **MISSING** (by design) | `src/metrics.rs:30-35` `TODO(upload)`; `docs/data-and-metrics.md:14`. |
| Workshop (self-modifying) | **WORKS (tested with a fake coder); real agents unverified** | `src/workshop.rs` (1,080 lines) 2 unit tests; `tests/workshop_pipeline.rs`: `a_change_is_coded_built_tested_committed_and_staged_on_this_pcs_branch` :196, `a_change_out_of_bounds_or_failing_the_tests_is_thrown_away` :285, `the_workshop_refuses_what_it_cannot_do_and_says_why` :353 (3 ok on Windows CI at d78108d). |
| Updater | **WORKS (tested locally/CI); channel never used** | `src/update.rs` (1,420 lines) 11 tests incl. `a_manifest_is_believed_only_with_the_release_keys_signature` :1117, `a_version_that_does_not_come_up_is_rolled_back_and_never_offered_again` :1231; `tests/update_channel.rs` 2 tests; CI "Installer test" success. **ms has 0 GitHub releases and 0 tags** (`gh api repos/boggioMichael/ms/releases`, `/tags` → empty); the `release` job runs only on `v*` tags or manual publish (`.github/workflows/standalone.yml`, release job `if:`) and was `skipped` on 03303a9. `https://datta-syrup.ai/downloads/manifest.json` was **unreachable from this sandbox** (proxy 403) — whether a signed manifest is live: unverified. |

### 2.2 Syrup (syrup@54dad1e)

| Item | Status | Evidence |
|---|---|---|
| Runtime dependencies of the library (root crate `syrup`) | **Clean** | `Cargo.toml`: `image`, `libloading`, optional `serde`; per-OS capture: `windows` (WGC/GDI/OCR features), `x11rb`, `zbus`, `objc2*`. |
| Runtime dependencies of `crates/syrup-runtime` | **Clean of services; ships a trained model** | `rqrr`, `sha2`, `serde_json`, `libloading`, optional `tract-onnx` (**default feature `face-yunet`**: `include_bytes!("../../models/face_detection_yunet_2023mar.onnx")`, `crates/syrup-runtime/src/providers/yunet.rs:19`); `tesseract` provider shells out to a Tesseract binary (`providers/tesseract.rs:19`); compiles generated code with `rustc`. |
| Analytics server / telemetry | **None** | No `http(s)://` endpoint in `src/` or `crates/` except the repository URL and `https://rustup.rs` in an error hint (`crates/syrup-runtime/src/compiler.rs:36`). |
| A specific game | **None in the library** | `grep -il "maplestory|nexon|minesweeper"` over `src/` and `crates/` `.rs`: 0 files. Game knowledge lives in examples (`example/minesweeper-coach`) and in ms. |
| A user database | **None** | No DB crate in either manifest; no network code in the library. |
| Things in the repo that are **not** the library | **Note** | `example/` holds TheLip product code: `thelip-server` ("behind a tunnel", README:96), `inference-api`, an iOS app, a Chrome extension whose `manifest.json:9` requests `https://*/*` host permissions, `docs/index.html:904` calls `https://api.thelip.ai`. Out of scope for the vision engine but in the same repo and licence. |

### 2.3 Syrup README claims — tested vs descriptive

| Claim (README @54dad1e) | Kind | Verdict |
|---|---|---|
| "name the function you need and the library builds it" (:3-4); intents compiled to a native library (:24-31) | tested | CI `cargo test --workspace` success on 3 OSes (37153606097). **My local run of `tests/intent_native.rs`: 2 passed, 2 failed** (`find_face_runs_as_a_shared_library_and_agrees_with_in_process`, `a_tracked_icon_keeps_its_picture_and_its_memory_inside_the_library`), both `BuildFailed`: the generated crate is built by a nested `cargo build` in `~/.cache/syrup/intents/<name>-<hash>/`, which tried to update the crates.io index (`Could not resolve host: index.crates.io`). Environmental (no network here), **but it shows a real runtime boundary: native-intent mode needs crates.io (or a warm cargo cache) and a Rust toolchain on the user's machine on first use of a name.** In-process mode (default) does not. |
| Root library tests | tested (by me) | Root crate only, re-resolved lockfile, `--release --offline`: lib **187 passed, 0 failed, 2 ignored**; `tests/synthetic_screen.rs` 3/3; `tests/wayland_capture.rs` 1/1 and `tests/x11_capture.rs` 1/1 (0.00 s each — almost certainly self-skipping with no display server; treat as not exercised). Doc-tests not run. `syrup-runtime` not built (vendoring gap) → CI evidence only. |
| "No input synthesis, no window manipulation, no process inspection" (:288-290) | descriptive | **Confirmed** by grep (§3). |
| "No trained models and no model files in the library" (:291-294) | descriptive | **True for crate `syrup`; false for the workspace's `syrup-runtime`**, whose default feature bundles the YuNet ONNX model (README's own table at :88 lists "YuNet faces"). |
| Performance tables "Before/Now" (:421-458), and ms PR #17's "24x faster per frame" | descriptive (benchmarks) | **Documented only.** CI does not run `cargo bench` (`.github/workflows/ci.yml` runs clippy/test/examples). Not re-run (time). |
| "Every number in the overlay is computed by the library on that frame" (demo.gif caption, :8-10) | descriptive | Not verified. |
| "Cargo git dependency, pinned to a tag" (:500) | descriptive | **False**: ms pins `rev = "54dad1e…"` (ms `Cargo.toml:23`); syrup has **no tags** (`gh api repos/boggioMichael/syrup/tags` → empty). |
| MapleSyrup's "per-frame path runs on Syrup alone, with no vision code of its own and no OCR or model on it" (:509-512) | descriptive | **Overstated**: ms@03303a9 still carries 8,899 lines in `src/vision/` + `src/sight/` (13 of those files `use syrup`); the learned sight's numbers reader is ms code (`src/sight/numbers.rs`). Whether every per-frame call bottoms out in syrup: not traced. |
| "a low-confidence answer is reported as missing with its reason" (:115-117) | descriptive + unit | Consistent with `Detection{value: None, failure_reason}`; ms tests at `observation.rs:209`. Not independently re-tested. |

## 3. Product boundaries

Method: `grep -rnIE` over every tracked `.rs .toml .py .js .html .ps1 .cmd .bat .iss .nsi` file of ms@03303a9
(`src/`, `build.rs`, `package/`, `installer/`, `tools/`) and of syrup@54dad1e (`src/`, `crates/`, `python/`,
`example/`) for: `SetWindowsHookEx GetAsyncKeyState GetKeyState RegisterHotKey RegisterRawInputDevices
GetRawInputData SendInput keybd_event mouse_event ReadProcessMemory WriteProcessMemory OpenProcess
VirtualAllocEx CreateRemoteThread SetWinEventHook PostMessage SendMessage WH_KEYBOARD WH_MOUSE` plus the input
crates `enigo rdev inputbot device_query` and (syrup) `XTest xdotool CGEventPost uinput`. **Zero hits in both
repositories.** This is a static check of the audited commits only, not of binaries or of later commits.

| Boundary | Verdict | Evidence |
|---|---|---|
| No reading of the game's memory | **Confirmed** | 0 hits for `ReadProcessMemory`/`OpenProcess`/`VirtualAllocEx`. The only calls that touch the game's window: `src/platform/overlay.rs:85-115` (ms@03303a9) — `FindWindowW` by exact title, `IsIconic`, `GetClientRect`, `ClientToScreen`, `GetWindowThreadProcessId` (pid only), `GetForegroundWindow`. Grep for MapleStory install paths / `.wz` / registry: nothing but a test string (`src/metrics.rs:1701`). |
| No code injection or hooks into the game | **Confirmed** | 0 hits for hooks/remote threads. `LibraryLoader` is used only for `GetModuleHandleW(null)` of MapleSyrup's own module (`overlay.rs:67`). Syrup's `libloading` loads syrup's own compiled intents (`src/intent/native.rs:55`, `crates/syrup-runtime/src/loader.rs:27`) and `libpipewire` (`src/capture/pipewire.rs:86`) into its own process. |
| No anti-cheat bypass | **Confirmed (no such code)** | 0 hits for `anti.?cheat|hackshield|xigncode|nprotect|gameguard` in code. Capture is the OS's own (Windows.Graphics.Capture / GDI features in syrup `Cargo.toml`); whether Nexon tolerates screen capture is a policy question code cannot answer. |
| No automated play | **Confirmed** | 0 hits for `SendInput keybd_event mouse_event PostMessage SendMessage` or any input crate, in either repo. Syrup README:288-290 states "No input synthesis, no window manipulation, no process inspection" — consistent with the grep. |
| No typing commands for the user | **Confirmed** | Same grep. Voice commands ("start recording", "change yourself") act on MapleSyrup itself (`Job::Command`, `Job::Rewrite` in `src/ai/mod.rs:292-295`). |
| No global keylogger | **Confirmed** | 0 hits for `GetAsyncKeyState GetKeyState RegisterHotKey RegisterRawInputDevices GetRawInputData WH_KEYBOARD WH_MOUSE`. |

What MapleSyrup **does** do to/around the game (not violations, but must be disclosed):
- **Lowers the game's volume** while it speaks: `src/platform/sound.rs:424-440` sets the game's Windows audio
  session volume by pid (`GetMasterVolume`/`SetMasterVolume`), pid from `src/bin/maplesyrup/main.rs:3984`.
- **Records all PC sound** (loopback) during a recording: `src/platform/loopback.rs:91` (`start`).
- Hides its own overlay from captures (`SetWindowDisplayAffinity` + `WDA_EXCLUDEFROMCAPTURE`, `overlay.rs:72`).

Code-execution surfaces (not game boundaries, but a security reviewer will ask):
- **Workshop** (`src/workshop.rs`): runs `claude -p <prompt> --permission-mode acceptEdits --allowedTools
  Read,Edit,MultiEdit,Write,Glob,Grep,LS,Bash(cargo *),Bash(rustfmt *)` or `codex exec --full-auto
  --skip-git-repo-check <prompt>` (`src/workshop.rs:645-660`), then `cargo build/test` (`:738`, `:748`). The
  prompt is the player's spoken request **plus the end of the session log** (`:633-635`). `Bash(cargo *)` runs
  build scripts, i.e. arbitrary code. Header promises "nothing is pushed … never on master" (`:12-18`); I did not
  verify a mechanical guard against push beyond the prompt text at `:628`.
- **syrup-runtime** generates Rust, compiles it with `rustc` and `dlopen`s it (README:24-31,
  `crates/syrup-runtime/src/compiler.rs`, `loader.rs:27`).

## 4. Data flows today (ms@03303a9)

There is **no HTTP library** in ms (`Cargo.toml`): every outbound request is a `curl` subprocess
(`src/ai/openai.rs:387`, `src/update.rs:984`, `src/app/recorder.rs:112`, `src/phone/tunnel.rs:72`) or the
phone's browser. Endpoints found in code: `https://api.openai.com/v1` (`src/ai/openai.rs`),
`https://api.openai.com/v1/realtime/calls` (`src/ai/live.rs`), `https://api.x.ai/v1` (`src/ai/mod.rs:164`,
model `grok-4.3`), `https://api.elevenlabs.io/v1` (`src/ai/eleven.rs`), `https://datta-syrup.ai/downloads/manifest.json`
(`src/update.rs:43`), ffmpeg zips from GitHub BtbN / gyan.dev (`src/app/recorder.rs`), cloudflared from GitHub
+ `*.trycloudflare.com` (`src/phone/tunnel.rs`).

### 4.1 What leaves the PC

| Flow | What is sent | When | Switch | Evidence |
|---|---|---|---|---|
| Conversation reply (OpenAI Responses, or xAI Grok when an xAI key exists) | the player's transcribed words; the conversation turns; a text "snapshot" of the game (level, bars, map, job…); **the whole game frame, 640×400 JPEG q60, "low" detail, with rulers**; the persona; **the notebook** (`about-me.txt` lines, `memory.json` facts/style/words, up to 12 `knowledge.json` lessons); session facts (deaths, level-ups, EXP rate, ETA) | every reply | an API key file; picture only when the game window is in front; Grok only with `xai-key.txt` and not `--no-grok` | `converse` (`src/ai/mod.rs:1515`) builds instructions = persona + `brain.learned()` (`:1539-1552`); `src/ai/mod.rs:181-188` (`Eyes::pictures`), `:1263-1275`, `:1336-1340`; `src/ai/brain.rs:604-655`, `:756-777`; `src/ai/memory.rs:622-640` (`Learning::prompt`); `main.rs:3985-3988` (`in_front`). Grok is the same `OpenAi` client pointed at `api.x.ai`, used as the `fast` replier (`main.rs:2636-2668`) — so **xAI gets the same instructions, notebook included** |
| Coach "looks" (autonomous) | frame + text, as above, **with the notebook** (`src/ai/mod.rs:1312-1319`: persona + `COACH_GUIDE` + `brain.learned()`) | every 45 s, backing off to 180 s (`LOOK_EVERY`, `LOOK_AT_MOST`) while playing | key; the frame buffer itself is filled only while the game is in front and cleared otherwise (`main.rs:644-653`); coach checks `in_view` (`main.rs:3017`) | `src/coach/mod.rs:361-362` |
| Speech-to-text outside a call | **the player's voice → the phone browser's speech service** (Web Speech API: Google's servers on Chrome/Android; Apple's on Safari, on-device or server by OS settings) | whenever the phone is listening | starting to listen on the phone | `src/phone/page.html:2046-2053` (`window.SpeechRecognition || window.webkitSpeechRecognition`) |
| Notebook update ("learner") | the notebook as JSON (facts, style, words, last_time), `about-me.txt`, kept lessons, **the latest conversation text** (each line ≤400 chars) | when ≥30 lines, or ≥6 after 4 min, or ≥2 after 8 min | key | `src/ai/memory.rs:496-530` (`look_back`), `:733-737` (`due`) |
| Teacher model (learned sight) | **full frame JPEG q88 "high" detail, HUD strips/crops as PNG "high"** | when the sight calibrates/teaches (trigger not traced) | key | `src/sight/teacher.rs:188-189`, `:211`, `:245`, `:270` |
| Web look-ups | the question | when the model calls the search tool | key | `Job::LookUp` (`src/ai/mod.rs:301`); utm_source=openai URLs in `src/ai/brain.rs` |
| Live call (OpenAI Realtime) | **the player's voice, streamed** from the phone's browser straight to OpenAI over WebRTC; the PC mints a short-lived key (`POST /api/live`) and serves `/api/eyes` (screen + readings, only while game in front: `main.rs:4114-4122`) and `/api/tool` | while a call is on | key + user starts a call | `src/ai/live.rs:1-26` |
| Voice (ElevenLabs) | the text of each line MapleSyrup says | every spoken line | ElevenLabs key | `src/ai/eleven.rs`; package/README.txt:48-56 |
| Workshop | the player's change request + **"the end of the session's log"** + version/commit → Anthropic or OpenAI via the local coding agent (`src/workshop.rs:619-640`); passes the player's `OPENAI_API_KEY` to the coder (`:663-665`) | on a "change yourself" request | workshop toggle, **off by default** (`main.rs:2436-2439`: `options.workshop.or(memory.workshop).unwrap_or(false)`) | `src/workshop.rs:619-665` |
| Updater | a plain `curl` GET (no identifiers, curl's default User-Agent) — reveals IP and that MapleSyrup runs | 45 s after start, then hourly | **on by default**: `update: std::env::var_os("MAPLESYRUP_NO_UPDATE").is_none()` (`main.rs:197`) and the phone setting `learning.memory().updates.unwrap_or(true)` (`main.rs:2427`); off with `--no-update` (`main.rs:253`), the env var, or Settings | `src/update.rs:43`, `:56-57`, `:983-1003` |
| ffmpeg download | IP | first recording | user asks to record | `src/app/recorder.rs:112-131` |
| Phone over the internet | **everything the phone and the PC exchange** — the player's recognised words, the replies' audio, the game screen for a call (`/api/eyes`), the phone microphone when recording, the link key `k` in the URL — through a Cloudflare quick tunnel (`https://<random>.trycloudflare.com`); Cloudflare terminates TLS, the local leg is plain HTTP (`tests/phone_link.rs::the_local_door_for_a_tunnel_serves_plain_http` :87); `cloudflared` is downloaded from GitHub if absent | only the "(phone over the internet)" launcher | opt-in launcher | `src/phone/tunnel.rs:1-6`, `:72-106`; package/README.txt:208-212 |
| Play stats export | **nothing** — no upload code; `TODO(upload)` | — | — | `src/metrics.rs:30-35` |

### 4.2 What stays local (and is tempting to upload later)

| File | Content | Sensitivity |
|---|---|---|
| `%APPDATA%\MapleSyrup\memory.json` | facts about the player, style, words | **personal profile — do not upload** |
| `%APPDATA%\MapleSyrup\knowledge.json` | lessons from corrections, looked-up facts | may hold personal facts; corrections are valuable episode data but need consent |
| `%APPDATA%\MapleSyrup\about-me.txt` | what the player wrote about themself (name, class, goals) | **personal — do not upload** |
| `MapleSyrup sessions\…\log.txt` | "what was said and heard" (package/README.txt:272) | **transcripts — highest-risk file** |
| `mic.wav` | the phone microphone, when started with `--record-mic` / recording launcher | **voice biometric** |
| `recording HH-MM-SS.mp4` | screen + PC sound + the phone mic | gameplay footage (Nexon rights) + voice + possibly other windows' sound |
| `markers.csv`, `mark-NNN.png` | marked moments, screenshots | gameplay footage; may show chat/names |
| `learned/` (sight) | HUD crops/templates the sight learned | gameplay footage fragments |
| `%APPDATA%\MapleSyrup\metrics\` | `sessions.jsonl`, `current-<id>.json`, `share.json`, `share-export.json` | pseudonymous stats; map names kept locally |
| `<settings>/workshop/` | per job: the prompt, coder output, build/test logs | contains session-log excerpts |
| `updates/log.txt` | updater actions | low |

### 4.3 Docs vs code

| Promise (package/README.txt, "Privacy", lines 246-266) | Code | Verdict |
|---|---|---|
| "pictures only while MapleStory is the window in front" | `main.rs:644-653` (frame buffer cleared when not in front), `brain.rs:756-777`, `main.rs:4114-4122` (live call) | **Holds** at the source (read, not run). Caveat: syrup's GPU path is a true window capture (`CreateForWindow`, syrup `src/capture/windows.rs:429`), but its CPU fallback is `PrintWindow`, "else a copy of the screen" (`windows.rs:3`) — on that last path anything drawn over the game (always-on-top overlays, notifications) is in the frame, which is why `main.rs:644-647` gates on "in front". And "MapleStory" is any window whose title contains the word (`src/capture.rs:57-67`), so a non-game window so titled (Discord, Notepad, Explorer are not excluded) would be captured and sent as if it were the game (code reading, not run). Teacher path not traced separately. |
| "To learn, every few minutes it sends the text of the conversation (not the pictures) … what it learns is kept on your PC only" | `memory.rs:496-530` sends the notebook + conversation; `brain.rs:643-655` + `memory.rs:622-640` put the notebook into **every** reply's instructions | **Misleading**: stored locally, but its contents go to OpenAI/xAI with every reply |
| "Without a key, nothing leaves your PC except the phone link on your own network" | updater contacts datta-syrup.ai hourly by default (`update.rs:43,56-57`) | **False as written** (the README's own "Updates" section, lines 222-230, says it checks) |
| "Recordings are made and kept on your PC only" | no upload code for recordings found | **Holds** (static) |
| "Nothing is sent anywhere yet" (stats) | `metrics.rs:30-35` TODO(upload), no endpoint | **Holds** |
| (not mentioned) workshop sends session-log excerpts to the coding agent's provider | `workshop.rs:633-635` | **Undisclosed in Privacy** |
| (not mentioned) "phone over the internet" relays mic audio via Cloudflare | `tunnel.rs` | **Undisclosed in Privacy** (described only as a connectivity fix) |
| (not mentioned) outside a live call the player's speech is recognised by the phone browser's speech service | `page.html:2053` | **Undisclosed**: with Chrome this is Google's server-side recognition — the player's voice leaves the phone to a third party even without any MapleSyrup key |
| "records a video of the whole screen with every sound" (package/README.txt:185-188, not in "Privacy") | `recorder.rs:363-377` (`ddagrab output_idx=0…draw_mouse=1` / `gdigrab desktop`), loopback of all PC sound, phone mic | **Holds, and is disclosed** — but it means a recording holds every window on the primary display and every PC sound, not only the game: the riskiest file to ever upload |
| `README.md:12`: "Download `MapleSyrup-Setup-<version>.exe` from Releases" | `gh api repos/boggioMichael/ms/releases` → **0 releases**, 0 tags | **False today** |
| `README.md:148-149`: 2,700 frames, mean 43.2 ms/frame (2026-09-30) | branch `demo/real-recording-output` 997ef8a, CI run 36643081386 (not opened) | **Documented, evidence branch exists**; not re-checked |

## 5. Play stats and sharing vs the new rules (ms@03303a9 — exists only on `claude/human`)

What is there (`src/metrics.rs`, `docs/data-and-metrics.md`): one `SessionStats` record per session, kept in
`%APPDATA%\MapleSyrup\metrics\sessions.jsonl` (last 365 sessions, `KEEP_RECORDS`, `metrics.rs:65`); a separate
"sharing" switch that, when on, writes `share.json` `{on, since: "YYYY-MM-DD", id: <uuid v4>}`
(`Consent`, `metrics.rs:495-507`; `set_sharing`, `:1064-1085`) and a local `share-export.json` built from an
allow-list (`EXPORT_FIELDS`, `:136`); **no upload** (`TODO(upload)`, `:30-35`). The phone card has an
"I'm 18 or older" checkbox and the PC refuses `POST /api/share {on:true}` without `adult:true`
(`src/phone/mod.rs:1079-1088`, test `:1544`, refusal asserted `:1572-1574`).

| New rule | Today | Verdict | What must change |
|---|---|---|---|
| **Purpose-based consent (not one boolean)** | One boolean `Consent.on` (`metrics.rs:497`) covering "share with MapleSyrup's partners, who may buy them" (`page.html:290`, `package/README.txt:264-266`). The local product stats have no consent at all (always on, "stay on the PC"). | **Fails** | Replace `on` with per-purpose grants (e.g. product analytics, studio insights, research panels, licensed episodes for training/eval, eval suites, commercial sale), each with its own text, each off by default; the export keyed by purpose. Decide whether local product stats need notice-only or consent. |
| **Commercial off by default** | Sharing is off until turned on (`Consent::default()`; test `sharing_is_off_until_turned_on_and_off_deletes_the_export_and_the_id` :1851). But the only sharing purpose *is* commercial ("may buy them"). | **Partially fits** | Keep off-by-default; split commercial sale from non-commercial purposes so turning on research does not turn on sale. |
| **Consent receipts (text version, time, source, scope)** | `share.json` holds `on`, a **date** (no time), and the install id. No text version/hash of what was shown, no language, no source (page/app version), no scope (fields/purposes). **Turning it off deletes `share.json`** (`turn_off`, "no file is off") — so **no record that consent was given or withdrawn survives**. The 18+ confirmation "is asked each time … and is not kept" (`docs/data-and-metrics.md:110`). | **Fails** | Append-only receipt log: receipt id, UTC timestamp, consent-text id + hash + language, UI source (page build, PC version/commit), purposes + field-set version (`EXPORT_FIELDS` version), age-assurance method + result, and a withdrawal receipt. Withdrawal deletes the data and the id but keeps the (pseudonymous) receipt. |
| **Adults only, proportionate age assurance** | Self-declared checkbox, enforced server-side (400 `adult_only`), not stored. MapleStory has young players (the doc says so, :187-190). | **Partially fits** (gate exists; assurance = self-declaration only; not recorded) | Record the assurance in the receipt; decide (with counsel) whether self-declaration is proportionate for *sale*; likely stronger assurance for commercial/episode purposes. |
| **Rights manifest per title** (MapleStory / MapleStory Worlds need title-specific review; Nexon restricts commercial use of gameplay footage) | Nothing. Records carry `hud` (`modern`/`classic`) and `job`/`maps` — no title, no client build, no region/world, no rights fields. Everything is implicitly "MapleStory". | **Missing** | A per-title rights manifest (title, publisher, allowed uses per purpose, footage yes/no, review status, reviewer, date) that the export checks before including any session; a title/build/world field on each record. Stats are not footage, but recordings, `mark-NNN.png`, `learned/` crops and the frames sent to providers are. |
| **No export without an approved basis** | No upload exists. The local export file is built whenever sharing is on. The only gate on a future upload is a comment ("once there is an endpoint, a privacy policy and terms", `:33-35`). | **Fits today by absence; no mechanism** | A code gate: no transmission unless an approved-basis record exists for (purpose, title, jurisdiction, field-set version); default deny; tested. |
| Data minimisation / no free text | Allow-lists `RECORD_FIELDS`/`EXPORT_FIELDS` with tests (`:1677`, `:1775`); class from a closed list (`:2319`); week not day; level bands; map **counts** only in the export; `09274f8` "no free text in the export". | **Fits** | Keep; version the field set and put the version in receipts. |
| Withdrawal and deletion | Off/"Delete it" deletes `share.json`, the export, the id, partials; verifies; 500 `not_deleted` if anything remains (test `:2075`). New id on re-enable. | **Fits locally** | Upstream deletion does not exist (no server) — needed before any upload (doc's own item 2, :173-175). |
| Pseudonymity / aggregation before sale | Export is per install id (doc: "pseudonymous, not anonymous", :153). k≥50 floor is **documented only** (:157-162); no aggregation code. | **Documented only** | Build and test the aggregation/suppression before any sale. |

Blunt summary: the stats code is careful about *what* it collects and about withdrawal, and it is honest that
nothing is sent. It does **not** implement the new consent model: one boolean, a date instead of a receipt, the
age confirmation thrown away, no purposes, no title rights. All of it is only on `claude/human` (PR #19, unmerged).

## 6. Gaps for the data program — what an Episode can be built from today

Episode = goal → observed state → help request → advice → action → outcome → correction.

| Episode part | Available today | Where | Gap |
|---|---|---|---|
| Goal | Free text only: `about-me.txt` ("what you're working towards", `package/README.txt:276-277`); `remember_fact` (`tools.rs:307`) | local file | **Missing as structured data**: no goal object, no goal start/end, no link to a session. |
| Observed state | Per frame `WorldState` → `Observation` (HUD: HP/MP/EXP/level, job, map name; other detectors run for the preview only) | `snapshot.rs:25`, `observation.rs:83` | **Not persisted per frame** in the companion; only text lines and `markers.csv`/`mark-NNN.png` on "mark". The `vision_debug` recorder (`MS_VISION_RECORD`, #10) records annotated runs for debugging, not sessions. Snapshot text sent with a reply is not logged as structured data (not traced). |
| Help request | The player's words | `log.txt` lines `HH:MM:SS  [heard] text` (`src/app/session.rs:76-79`; `kind_label`, `main.rs:1380-1388`) | Transcript only, from browser STT or Realtime; free text, local time to the second, no ids, the date only in the folder name. Highest privacy risk. |
| Advice | MapleSyrup's replies, warnings, coach lines | `log.txt` kinds `reply`, `warning`, `alert`, `info` (`main.rs:1380-1388`) plus `coach`, `live`, `lookup`, `turn`, `sight`, … | Not linked to the observation it was based on, nor to a request id; the model's input (frame + notebook) is not kept. |
| Action | **None observed — by design** (no input capture, §3) | — | Only *inferred* from state changes (e.g. `potions_answered`: a low-HP warning "answered" by a bar rising, `docs/data-and-metrics.md:31`). Action provenance is impossible without either input capture (excluded by the boundaries) or an explicit, consented player self-report. |
| Outcome | Deaths, level-ups, close calls, EXP rate, stalls | coach/companion counters (`src/coach/mod.rs:361-432`), metrics counts | Counts per session, not per advice; no outcome window tied to an advice id. |
| Correction | `knowledge.json` entries `{id, about, answer, from: Player, when: YYYY-MM-DD, uses}` | `src/ai/knowledge.rs:35-46` | Date only; not linked to the advice it corrects or the observation. |
| Game build / world detection | `hud`: `modern` vs `classic` (Classic World) from the HUD's look | `metrics.rs` `hud` field; `4288187` | **No client version/build, region, server or world-type detection**. The game is identified **by window title alone**: an exact "MapleStory", else the first title *containing* "maplestory" that is not on a short list of media-player/browser markers (`src/capture.rs:31-67`, `pick_game_window`). By that code (not run), a "MapleStory Worlds" window, a private-server client, or "maplestory notes.txt - Notepad" would each be taken as the game. Needed for the rights manifest; today the title of a session cannot be asserted. |
| Observation coverage | `game_minutes` (minutes the game was seen) vs `minutes` | `metrics.rs` record | No per-frame coverage/quality record, no "unseen because behind another window" accounting outside the prompt text (`brain.rs:772-777`). |
| Event sequencing | Local wall-clock seconds in `log.txt`; `markers.csv` | `session.rs:76-79` | **No monotonic clock, no event ids, no causal links, local time zone**. `examples/evening.rs` shows the event model exists in memory (`MM:SS [kind] text`), but nothing writes it as structured episodes. |

Net: today one could reconstruct, from local `log.txt` text, *request → advice* pairs with second-level timing,
and session-level *outcome* counts. Goals, actions, structured observed state, advice↔outcome links and
title/build provenance do not exist. Building Episodes needs a new, consented, structured event log
(schema-versioned, monotonic, with ids), not the existing files — and the existing files (`log.txt`, `mic.wav`,
recordings, `memory.json`) must **not** be backfilled into it without fresh, purpose-specific consent.

## 7. What I could not verify

1. **The ms suite at 03303a9** was not re-run by me (forbidden to build in `/home/claude/ms`; I did not build an
   archive either, for time). I cite the manager's 505/505 and CI runs 38027967314 / 38027963441 (green). Per-test
   pass/fail names exist only for d78108d (Windows CI log: 404 ok, 0 failed).
2. **Anything against the live game**: all perception evidence is fixtures/recorded frames; no Windows, no
   MapleStory, no Classic World client here.
3. **Live providers**: OpenAI/xAI/ElevenLabs behaviour is tested only against fake servers
   (`tests/openai_fake.rs`, `tests/eleven_fake.rs`); I did not check that the model names in code
   (`gpt-6-luna`, `gpt-6.1-sol`, `grok-4.3`, `eleven_v4`, …) exist or behave as assumed.
4. **The update channel**: `https://datta-syrup.ai/downloads/manifest.json` unreachable from this sandbox
   (proxy `CONNECT … 403`); ms has 0 GitHub releases/tags, so whether any signed release was ever served is unknown
   (likely none via the pipeline).
5. **syrup-runtime locally**: not built (vendored crates lack `rqrr`; lockfile `clap` mismatch). CI is the
   evidence. My root-crate run used a **re-resolved** lockfile, not the locked versions. Doc-tests not run.
   `wayland_capture`/`x11_capture` passed in 0.00 s (almost certainly self-skipped without a display).
6. **Benchmarks** (syrup README performance tables; "24x faster per frame") — not run.
7. **Workshop guarantees** ("nothing is pushed", "never on master", release files out of bounds): verified only
   as code intent and fake-coder tests (`tests/workshop_pipeline.rs`). For Claude Code the `--allowedTools` list
   admits no `git` in Bash (only `cargo *`, `rustfmt *`) — a real limit, though `cargo` build scripts can run
   anything; Codex runs `--full-auto` with no tool list in the command. No repository-level guard exists
   (`grep "remote|hooksPath|pre-push" src/workshop.rs` → 0 hits): "no push" rests on the prompt (`:628`).
8. **Teacher-model trigger** (when `sight::teacher` calls the model): not traced. `perceive()` receives `in_view`
   (`main.rs:663-669`) and the frame buffer is cleared when the game is not in front (`:648-653`), which suggests
   the same gating, but I did not follow the call to the model.
9. **Commits after 03303a9** on `claude/human` (two workers active) — not audited.
10. **Binaries** (`build-output/*/MapleSyrup.zip`, installer) — not inspected; all boundary checks are source-only
    at the named commits.
11. **Content of** `feature/ui`, `llm-knowledge-architecture`, the August `agents/*` one-commit branches, syrup's
    `example/` (TheLip server, iOS app, Chrome extension) and `python/` — only grepped for endpoints, not read.
12. **Legal questions** (Nexon ToS on screen capture and on commercial use of footage/derived data; Israeli
    Privacy Protection Law Amendment 13; GDPR; age assurance) — out of scope for code review; flagged, not answered.

Side effects of this audit (outside both repositories' working trees): the instructed `git fetch origin` moved
ms's remote-tracking ref `origin/master` 3748d58→6d36a0e (syrup's had nothing new); the extra branch fetches
wrote only objects and `FETCH_HEAD` in both `.git` directories; no local branch, HEAD, index or working tree was
touched (local ms `master` still 9b4fe23); files created under
`/home/claude/audit-src/` and `/home/claude/audit-target/`; syrup's native-intent tests created build folders
under `/root/.cache/syrup/intents/`. Nothing was committed, pushed, checked out or built in either repository.
