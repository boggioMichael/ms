"""Background speech generation and Qt playback with a PCM-derived waveform."""

from array import array
from collections import deque
import io
import math
import queue
import sys
import wave

from PySide6.QtCore import QBuffer, QIODevice, QObject, QThread, QTimer, QUrl, Signal
from PySide6.QtMultimedia import QAudioOutput, QMediaDevices, QMediaPlayer

from ai.voice.local_tts import LocalTTS


TEST_TEXT = "Hi! I'm MapleSyrup!"
DEFAULT_VOLUME = 0.65


def audio_envelope(clip):
    """Calculate RMS loudness for each 20 ms of actual PCM, outside the GUI thread."""
    samples = array('h')
    samples.frombytes(clip.pcm)
    if sys.byteorder != 'little':
        samples.byteswap()
    width = max(1, round(clip.sample_rate * 0.02))
    return [min(1.0, (math.sqrt(sum((s / 32768) ** 2 for s in chunk) / len(chunk)) * 4) ** 0.65)
            for start in range(0, len(samples), width)
            if (chunk := samples[start:start + width])]


def wave_levels(envelope, position_ms):
    """Show the current and preceding 680 ms, indexed by the player's timeline."""
    end = max(0, int(position_ms // 20))
    return [envelope[i] if 0 <= i < len(envelope) else 0.0
            for i in range(end - 34, end + 1)]


class VoiceWorker(QThread):
    ready = Signal(int, object, list)
    failed = Signal(int, str)

    def __init__(self, parent=None, provider_factory=LocalTTS):
        super().__init__(parent)
        self.provider_factory = provider_factory
        self.commands = queue.Queue()
        self.stopping = False

    def stop(self):
        if not self.stopping:
            self.stopping = True
            self.commands.put(None)

    def run(self):
        provider = None
        while not self.stopping:
            command = self.commands.get()
            if command is None or self.stopping:
                return
            token, text = command
            try:
                if provider is None:
                    provider = self.provider_factory()
                clip = provider.synthesize(text)
                envelope = audio_envelope(clip)
                if not self.stopping:
                    self.ready.emit(token, clip, envelope)
            except Exception as exc:
                if not self.stopping:
                    message = (str(exc) if isinstance(exc, RuntimeError) else
                               'Voice generation failed. Check the TTS installation and model files.')
                    self.failed.emit(token, message)


class VoiceTest(QObject):
    """One speech session, one synthesis worker, one sequential audio player."""

    frame = Signal(str, list, str, float)
    captions = Signal(str, str)
    speech_level = Signal(float)
    availability_changed = Signal()

    def __init__(self, parent=None, provider_factory=LocalTTS):
        super().__init__(parent)
        self.worker = VoiceWorker(self, provider_factory)
        self.worker.ready.connect(self.on_audio)
        self.worker.failed.connect(self.on_generation_error)
        self.player = QMediaPlayer(self)
        self.output = QAudioOutput(self)
        self.muted = False
        self.output.setVolume(DEFAULT_VOLUME)
        self.player.setAudioOutput(self.output)
        self.player.mediaStatusChanged.connect(self.on_media_status)
        self.player.errorOccurred.connect(self.on_playback_error)
        self.timer = QTimer(self)
        self.timer.setInterval(33)
        self.timer.timeout.connect(self.update_frame)
        self.buffer = None
        self.envelope = []
        self.token = 0
        self.generating = False
        self.active = False
        self.closing = False
        self.reduced_motion = False
        self.text = ''
        self.current_sentence = ''
        self.previous_sentence = ''
        self.pending_sentences = deque()
        self.ready_audio = deque()
        self.inflight = None
        self.input_finished = False
        self.caption_started = False
        self.smoothed_speech_level = 0.0
        self.worker.start()

    def begin(self):
        self.stop()
        if self.closing:
            return
        self.current_sentence = self.previous_sentence = ''
        self.captions.emit('', '')
        self.input_finished = False
        self.active = True
        if QMediaDevices.defaultAudioOutput().isNull():
            self.fail('No audio output device is available. Connect speakers or headphones and retry.')
            return
        self.frame.emit('Thinking', [], 'Waiting for the first sentence...', 0.5)
        self.availability_changed.emit()

    def set_muted(self, muted: bool) -> None:
        """Mute audio output without disrupting synthesis or playback state."""
        self.muted = bool(muted)
        self.output.setVolume(0.0 if self.muted else DEFAULT_VOLUME)

    def start(self, text=TEST_TEXT):
        self.begin()
        self.enqueue(text)
        self.finish_input()

    def enqueue(self, sentence):
        if self.active and not self.input_finished and sentence.strip():
            self.pending_sentences.append(sentence.strip())
            self.pump()

    def finish_input(self):
        self.input_finished = True
        self.pump()

    def pump(self):
        if not self.active or self.closing:
            return
        # Keep up to two prepared clips ahead; never interrupt current playback.
        if not self.generating and self.pending_sentences and len(self.ready_audio) < 2:
            sentence = self.pending_sentences.popleft()
            self.generating = True
            self.inflight = (self.token, sentence)
            self.worker.commands.put(self.inflight)
        if self.buffer is None and self.ready_audio:
            sentence, clip, envelope = self.ready_audio.popleft()
            self.play_clip(sentence, clip, envelope)
        if (self.input_finished and not self.generating and not self.pending_sentences
                and not self.ready_audio and self.buffer is None):
            self.active = False
            self.frame.emit('Ready', [0.0] * 35, self.current_sentence, 0.5)
        self.availability_changed.emit()

    def on_audio(self, token, clip, envelope):
        sentence = self.inflight[1] if self.inflight else ''
        self.generating = False
        self.inflight = None
        if token == self.token and self.active and not self.closing:
            self.ready_audio.append((sentence, clip, envelope))
        self.pump()
        self.availability_changed.emit()

    def play_clip(self, sentence, clip, envelope):
        data = io.BytesIO()
        with wave.open(data, 'wb') as wav:
            wav.setnchannels(1)
            wav.setsampwidth(2)
            wav.setframerate(clip.sample_rate)
            wav.writeframes(clip.pcm)
        self.text = sentence
        self.caption_started = False
        self.envelope = envelope
        self.buffer = QBuffer(self)
        self.buffer.setData(data.getvalue())
        self.buffer.open(QIODevice.OpenModeFlag.ReadOnly)
        self.player.setSourceDevice(self.buffer, QUrl('speech.wav'))
        self.player.play()
        self.timer.start()

    def update_frame(self):
        if not self.active or self.closing or self.buffer is None:
            return
        if self.player.playbackState() == QMediaPlayer.PlaybackState.PlayingState:
            if not self.caption_started:
                self.previous_sentence = self.current_sentence
                self.current_sentence = self.text
                self.caption_started = True
                self.captions.emit(self.previous_sentence, self.current_sentence)
            actual_levels = wave_levels(self.envelope, self.player.position())
            target = max(actual_levels, default=0.0)
            self.smoothed_speech_level += (target - self.smoothed_speech_level) * 0.35
            self.speech_level.emit(self.smoothed_speech_level)
            levels = [0.0] * 35 if self.reduced_motion else actual_levels
            self.frame.emit('Speaking', levels, self.current_sentence, 0.5)

    def release_audio(self):
        self.timer.stop()
        self.player.stop()
        self.player.setSource(QUrl())
        if self.buffer is not None:
            self.buffer.close()
            self.buffer.deleteLater()
            self.buffer = None
        self.envelope = []
        self.smoothed_speech_level = 0.0
        self.speech_level.emit(0.0)

    def on_media_status(self, status):
        if status == QMediaPlayer.MediaStatus.EndOfMedia and self.active:
            self.release_audio()
            if not self.ready_audio:
                self.frame.emit('Thinking', [0.0] * 35, self.current_sentence, 0.5)
            self.pump()

    def on_generation_error(self, token, message):
        self.generating = False
        self.inflight = None
        if token == self.token and self.active and not self.closing:
            self.fail(message)
        else:
            self.pump()
        self.availability_changed.emit()

    def on_playback_error(self, error, message):
        if self.active and not self.closing:
            self.fail('Audio playback failed. Check your output device and try again.')

    def fail(self, message):
        self.stop()
        self.frame.emit('Error', [], message, 0.5)

    def stop(self):
        self.token += 1
        self.active = False
        self.pending_sentences.clear()
        self.ready_audio.clear()
        self.input_finished = True
        self.release_audio()
        self.availability_changed.emit()

    def shutdown(self):
        self.closing = True
        self.stop()
        self.worker.stop()
