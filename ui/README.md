# MapleSyrup desktop UI

A standalone PySide6 avatar overlay. It displays a small animated dog on a
transparent background, requests always-on-top placement, and starts near the
upper-right desktop corner. The avatar has a fixed 240 x 135 canvas, can be
dragged, and uses the same Qt Widgets implementation on macOS and Windows.

The avatar starts in Idle. A short left click enables or disables local
continuous listening; dragging never changes microphone state. A right click
opens Settings, Captions, Mute voice, and Exit. The Agent text dialog and Test
voice action remain under Settings. Agent replies are still streamed and spoken
sentence by sentence with local Kokoro.

## Animation assets

Put frame sequences under `ui/animations/`:

```text
ui/animations/idle/idle_01/frame_0001.png
ui/animations/idle/idle_02/frame_0001.png
ui/animations/listening/frame_0001.png
ui/animations/thinking/thinking_01/frame_0001.png
ui/animations/speaking/speaking_01/frame_0001.png
```

A state may hold `frame_*.png` files directly or several subdirectories, each
with its own sequence. Files are sorted by frame number. Each sequence defaults
to 15 FPS; add `animation.json` beside its frames to override it:

```json
{"fps": 12}
```

Missing Listening, Thinking, or Speaking assets fall back to Idle. When several
Idle sequences exist, the next loop selects a different sequence when possible.
If no valid animation files exist, the app shows a temporary drawn marker and
still starts.

## Install and run from the repository root

Use a standard CPython installation (Python 3.14.3 was tested). The pinned
PySide6 6.11.2 macOS wheel requires macOS 13 or newer. A separate environment
keeps UI packages independent of `ai/.venv`. UI requirements include the existing
AI requirements so that text mode can use both providers.

macOS:

```sh
python3 -m venv ui/.venv
ui/.venv/bin/python -m pip install -r ui/requirements.txt
ui/.venv/bin/python -B ui/main.py
```

Windows PowerShell (not yet tested on Windows):

```powershell
py -m venv ui/.venv
.\ui\.venv\Scripts\python.exe -m pip install -r ui/requirements.txt
.\ui\.venv\Scripts\python.exe -B ui/main.py
```

Create the environment on each machine; do not copy a Mac environment to Windows.
No Qt tools, fonts, or image assets need to be installed separately.

## Controls

- Left click the dog to enable or disable microphone listening. The local STT
  model currently recognizes English only.
- Drag the dog to move the overlay.
- Right click for Settings, **Captions**, **Mute voice**, and Exit. Captions show
  only the sentence currently playing through TTS.
- Settings retains **Agent text test...**, **Test voice**, **Stop speech**,
  **Reduce motion**, and manual Demo state selection.
- Speaking begins only while Qt reports actual audio playback. Thinking covers
  Agent generation and sentence synthesis; after playback, the avatar returns
  to Listening when the microphone is enabled, otherwise Idle.

## Agent text testing

1. Open the gear menu and choose **Agent text test...**. Choose a provider and
   model before enabling microphone listening, because the first STT version uses
   the same selected Agent session as typed messages.
2. Choose Ollama or OpenAI, then click **Load models**. This explicitly contacts
   that provider; merely opening the dialog sends no request.
3. Choose a model, enter a message, and click **Send to Agent** (or press Enter).
4. Leave **Read replies aloud** checked to hear speech segments as they become available.
   Text accumulates live in the scrollable dialog while Kokoro prepares segments.
   Thinking covers generation; Speaking and the VOICE badge indicate playback.
   Ready marks the end. The compact window follows the currently playing segment.
   A faded previous segment appears above it when the current one fits on one line.
   Otherwise both lines prioritize current speech. Long sentences have a two-line
   preview and full sentence tooltip; the entire answer stays in the text dialog.
5. Uncheck **Read replies aloud** for text-only responses. Unchecking also stops
   current reply audio and discards pending speech. Checking it again applies to
   future replies; it does not replay old answers. **Stop speech** in the gear
   menu stops the current clip without changing the checkbox.

Submitting another message stops previous speech. If synthesis cannot be stopped
immediately, its obsolete result is discarded; only the latest pending reply can
be played. Switching to Demo or the fixed voice test prevents late Agent replies
from interrupting that mode. Text remains available if speech fails, with the
speech error shown in the test dialog.

Speech starts after the first speech segment has been synthesized, without
waiting for the full LLM response. The first segment uses a natural clause break
after at least five words, or a word boundary after ten complete words if no
earlier break is available. Later segments use the usual sentence boundaries
and clause breaks after at least 12 words. Kokoro still synthesizes each segment
in full; there is no audio-chunk streaming or word-level transcript alignment.
Initial model loading and long passages without suitable punctuation can still
add latency. The first segment's word-boundary fallback can split a phrase and
affect intonation; this limit applies only once per response.

Streaming overlaps text generation and speech synthesis; it does not guarantee
that playback starts before the last text arrives. A fast LLM can finish a short
answer while Kokoro is still preparing the first segment. Kokoro uses up to four
CPU threads (limited by the reported CPU count) to reduce local synthesis time.
On the development Mac, a warmed 18-word sample took about 4.05 seconds with two
threads and 2.41 seconds with four, across two runs each. Results depend on the
machine and concurrent workload; this is not a latency guarantee.

Ollama requires a running local service and an installed text model. OpenAI uses
`OPENAI_API_KEY` from the existing `ai/.env` (or environment) and sends text to the
cloud; API charges may apply. Do not put keys in UI source files. Its model list
may contain non-text models; select one supporting the Responses API.

The same Agent retains five completed turns. Sending with a different provider
or model starts a new Agent session and clears the displayed conversation.
Switching between Demo and text mode keeps the current text session. A new
message supersedes an in-flight request. Providers are still accessed serially;
the old stream is closed when its next chunk arrives. Partial/canceled/failed
turns are not added to Agent memory; one completed response is one stored turn.
TTS and STT are local. There is no MCP integration. No conversation or microphone
recording is written to disk.

## Local voice setup and testing

The voice library is installed by `ui/requirements.txt`. Model files are a
separate one-time download (about 350 MB). Run these commands from the repository
root on macOS, only if `ai/voice/.models/kokoro-multi-lang-v1_0` is missing:

```sh
mkdir -p ai/voice/.models
curl --fail -L https://github.com/k2-fsa/sherpa-onnx/releases/download/tts-models/kokoro-multi-lang-v1_0.tar.bz2 -o ai/voice/.models/kokoro.tar.bz2
tar -xjf ai/voice/.models/kokoro.tar.bz2 -C ai/voice/.models
```

On Windows, extract the same archive into `ai/voice/.models/`, preserving the
`kokoro-multi-lang-v1_0` directory. The archive and extracted model stay outside
Git. Model installation is not automatic when the UI starts. Download source:
[official sherpa-onnx Kokoro documentation](https://k2-fsa.github.io/sherpa/onnx/tts/pretrained_models/kokoro.html).

Open the gear menu and choose **Test voice**. The window says VOICE TEST. It first
shows Thinking while Kokoro generates audio in a background thread, then Speaking
while Qt plays the clip through the default system output. The voice is Kokoro
1.0 `af_heart` (speaker 3), speed 1.0. The fixed sentence is:

> Hi! I'm MapleSyrup!

The model stays loaded for repeated tests. Audio stays in RAM; no WAV file is
written. The waveform uses RMS amplitude from the generated PCM in 20 ms windows,
with a bounded perceptual gain curve to make quieter speech visible,
indexed by the media player's playback position, not an unrelated animation timer.
It follows the audio timeline; device buffering may add a small visual/audio offset.
The voice test displays its short transcript as a whole, without word alignment.

If the bars stay flat, check **Reduce motion**. This setting stops waveform motion
but leaves audio playback enabled. Playback uses 65% application volume; use the
system volume to adjust loudness. Volume and microphone selection controls are
still unavailable.

Use **Stop speech** to stop immediately. If synthesis is still running, its
result is discarded. A replacement test waits for that computation to finish,
then generates its own audio; obsolete audio is never played.
Closing the application stops audio immediately, then waits responsively for any
in-flight synthesis or Agent request to finish before exiting. No synthesis is
forcibly terminated. Missing model/output-device and playback errors appear in
the transcript (hover to read the complete message).

## Local speech recognition setup

`sherpa-onnx` is already installed through `ui/requirements.txt`; speech
recognition needs a separate local English model. The application never downloads
this model automatically. Download it once from the repository root, only if
`ai/voice/.models/stt/sherpa-onnx-streaming-zipformer-en-2023-06-26` is missing:

```sh
mkdir -p ai/voice/.models/stt
curl --fail -L https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-streaming-zipformer-en-2023-06-26.tar.bz2 -o ai/voice/.models/stt/stt-model.tar.bz2
tar -xjf ai/voice/.models/stt/stt-model.tar.bz2 -C ai/voice/.models/stt
```

On Windows PowerShell, download the same archive and extract it into
`ai/voice/.models/stt/`, preserving the final directory name:

```powershell
New-Item -ItemType Directory -Force ai/voice/.models/stt
Invoke-WebRequest https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-streaming-zipformer-en-2023-06-26.tar.bz2 -OutFile ai/voice/.models/stt/stt-model.tar.bz2
tar -xjf ai/voice/.models/stt/stt-model.tar.bz2 -C ai/voice/.models/stt
```

The final directory must be exactly:

```text
ai/voice/.models/stt/sherpa-onnx-streaming-zipformer-en-2023-06-26
```

On macOS, grant microphone access when the terminal or packaged MapleSyrup
application requests it. If permission was previously denied, enable it in
**System Settings → Privacy & Security → Microphone**. On Windows, enable
**Settings → Privacy & security → Microphone → Let desktop apps access your
microphone**.

Choose an Agent provider and model in **Agent text test...** before enabling the
microphone. Click the microphone button to start continuous listening. After the
recognizer detects a completed English utterance, it sends one message to the
Agent, pauses while MapleSyrup replies, and resumes when speech finishes. Click
the microphone again to mute immediately; partial audio is discarded. Audio
recordings are not written to disk.

Use headphones for the first version. It has no acoustic echo cancellation or
barge-in support, so speakers can cause MapleSyrup's own voice to be recognized
as player speech after listening resumes. Model source:
[official Sherpa-ONNX English streaming Zipformer documentation](https://k2-fsa.github.io/sherpa/onnx/pretrained_models/online-transducer/zipformer-transducer-models.html).

## Streaming and sentence pipeline

- Both existing providers expose `stream(instructions, user_input, history)`.
  OpenAI requests Responses API streaming and yields `response.output_text.delta`
  events, requiring `response.completed`. Ollama requests `stream: true` on
  `/api/chat` and parses newline-delimited JSON, requiring `done: true`.
  Reasoning/thinking fields are not spoken. Disconnects and incomplete responses
  are errors. The existing synchronous `generate` methods remain available.
- `Agent.respond_stream` forwards deltas and saves the complete turn only after
  successful stream completion. Non-streaming custom adapters can fall back to
  `generate`; the shipped OpenAI and Ollama adapters use real streaming.
- `SentenceBuffer` accumulates arbitrary chunks. It recognizes `.`, `!`, `?`,
  grouped punctuation, and closing quotes/brackets, and waits for boundary
  lookahead. Common English abbreviations, initials, decimals, and numbered list
  prefixes are kept together. Commas, semicolons, colons, and en/em dashes also
  release a segment once it contains at least 12 whitespace-separated words.
  The first segment of each response lowers this minimum to five and falls back
  to a word boundary at ten words. It waits for whitespace (or stream end) to
  confirm that the tenth word is complete, keeping partial streamed words and
  their punctuation together. The earliest eligible boundary wins even if a
  single provider chunk contains the entire answer. A short first sentence also
  counts as the first segment; subsequent segments have no ten-word limit.
  Short clauses stay together, and punctuation within numbers or times does not
  split speech. This is a minimum for clause breaks, not a maximum segment length;
  later text without natural breaks waits for sentence completion or stream end.
  Final text is flushed even without punctuation. The full response text and
  completed-turn memory are independent of these speech-only boundaries.
  These are conversational heuristics, not a general multilingual NLP parser.
- `VoiceTest` owns `pending_sentences`, up to two prepared `ready_audio` clips,
  and one sequential Qt player. A single background worker synthesizes the next
  sentence while the current clip plays. Captions advance on actual playback,
  not when synthesis finishes. Native clip transitions still incur small decoder
  overhead; a slow next synthesis can cause a gap.
- Request IDs prevent stale LLM signals from updating the current conversation.
  Speech session tokens invalidate pending/generated audio on Stop, a new request,
  mode changes, disabling read-aloud, or close. Stop does not halt text generation.
  Turning read-aloud back on applies to the next request. One in-flight native
  synthesis is allowed to finish and discarded after cancellation.
- Socket reads cannot be forcibly interrupted from the UI: cancellation/shutdown
  may wait for the next received chunk or the provider's 120-second read timeout.
  This timeout is not a total wall-clock deadline for a long response.

References: [OpenAI streaming](https://developers.openai.com/api/docs/guides/streaming-responses),
[Ollama chat](https://docs.ollama.com/api/chat).

## Files and future integration

- `avatar_assets.py`: discovers direct and nested animation sequences, reads
  optional per-sequence FPS configuration, and caches scaled frames.
- `avatar_window.py`: transparent dog overlay, elapsed-time frame selection,
  continuous floating, drag/click handling, outline drawing, and context menu.
- `speech_bubble.py`: compact current-sentence caption bubble that follows the
  avatar and clamps to screen edges.
- `agent_chat.py`: text test dialog and a background worker that owns the real
  Agent and provider. Discovery and generation do not block the GUI thread.
- `audio_player.py`: background voice generation, in-memory Qt playback,
  playback-position-based waveform updates, and a future mouth-animation level.
- `../ai/voice/sentences.py`: incremental sentence and long-clause segmentation.
- `../ai/voice/tts.py`: common TTS protocol and mono PCM16 audio contract.
- `../ai/voice/local_tts.py`: local Kokoro implementation; independent of Qt.
- `main.py`: connects the avatar to real microphone, Agent, TTS, captions, and
  desktop-window lifecycle events.
- `test_avatar_assets.py`, `test_avatar_window.py`, and `test_speech_bubble.py`:
  offline asset, interaction, timing, and caption tests.
- `test_agent_chat.py`: slow fake-provider checks of real Agent memory, responsive
  event processing, errors, model changes, and orderly worker shutdown.

- `test_audio_player.py`: synthetic PCM, cancellation, errors, reduced motion,
  and playback lifecycle tests without real speaker output.

A future cloud TTS provider can implement `TTSProvider.synthesize()` and return
an `AudioClip` in the same format. Within-segment audio streaming, speech
recognition, and microphone capture remain future work. A provider change may need configuration and credentials but should not
require changes to the window drawing code.

## Verification

Run the offline checks on macOS:

```sh
QT_QPA_PLATFORM=offscreen ui/.venv/bin/python -B -m unittest discover -s ui -p 'test_*.py'
```

The tests cover state selection, mouse/keyboard mute, settings availability,
transcript visibility, long and right-to-left text rendering, stable geometry,
reduced motion, and timer shutdown. RTL rendering is a smoke check, not a full
localization review.

Verified on macOS with Python 3.14.3 and PySide6 6.11.2:

- Existing UI, voice, and Agent tests plus streaming/queue tests pass offline.
  Protocol tests use mocked OpenAI SSE events and Ollama NDJSON; no OpenAI API
  requests were made. Full-application subprocess tests exercise Stop, read-aloud
  off, Demo, Test voice, replacement requests, and close while streaming.
- A native test used actual local Ollama `llama3.1:8b` and Kokoro. First speech
  began at about 4.24 seconds; the full LLM response finished at about 4.72 seconds
  in that run (not a benchmark). Sentences played in order, captions advanced,
  complete text matched one memory turn, and the GUI heartbeat continued.
- Actual PCM waveform levels varied in both Test voice and Agent speech. Enabling
  Reduce Motion froze them. The missing macOS preference previously selected a
  true fallback; it now leaves animation enabled. The native run started with
  Reduce Motion off. Capture/rendering checks do not measure physical speaker
  latency or replace human listening-quality review.

OpenAI streaming has offline protocol coverage only. Windows, native OS-assisted
dragging, and always-on-top behavior above games still require manual testing.

The window requests `WindowStaysOnTopHint`; the OS decides how it is applied.
This is not a guarantee above exclusive fullscreen games or other protected
windows. Windows placement, display scaling, drag behavior, accessibility
preferences, and behavior above MapleStory still require real Windows testing.

Design follows the supplied written specification. A separate reference image
was not available in the attachment received for this implementation.

The Agent-to-speech connection also has native Mac coverage with local Ollama:
an actual Agent response was synthesized and played, unchecked read-aloud kept
responses text-only, and disabling read-aloud during playback stopped the audio.
Offline tests additionally cover replacing and canceling pending speech requests.
