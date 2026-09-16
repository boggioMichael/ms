"""Non-blocking microphone capture and local speech recognition coordination."""

from array import array
import math
import queue
import sys

from PySide6.QtCore import QObject, QThread, Signal
from PySide6.QtMultimedia import QAudioFormat, QAudioSource, QMediaDevices

from ai.voice.stt.local import SAMPLE_RATE, LocalSTT


class RecognitionWorker(QThread):
    """Run incremental local recognition away from the GUI event loop."""

    recognized = Signal(int, str)
    failed = Signal(int, str)

    def __init__(self, provider_factory=LocalSTT, parent=None):
        super().__init__(parent)
        self.provider_factory = provider_factory
        self.commands = queue.Queue()
        self.stopping = False
        self.active_token = 0

    def set_active_token(self, token):
        self.active_token = token

    def submit(self, token, samples, sample_rate):
        if not self.stopping:
            self.commands.put(('audio', token, samples, sample_rate))

    def reset(self):
        if not self.stopping:
            self.commands.put(('reset',))

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
            if command[0] == 'reset':
                if provider is not None:
                    provider.reset()
                continue
            _, token, samples, sample_rate = command
            if token != self.active_token:
                continue
            try:
                if provider is None:
                    provider = self.provider_factory()
                result = provider.accept_audio(samples, sample_rate)
                if result is not None and token == self.active_token and not self.stopping:
                    self.recognized.emit(token, result.text)
            except Exception as exc:
                if token == self.active_token and not self.stopping:
                    message = str(exc) if isinstance(exc, RuntimeError) else (
                        'Speech recognition failed. Check the microphone and local STT setup.'
                    )
                    self.failed.emit(token, message)


class MicrophoneController(QObject):
    """Capture default-device PCM and forward final local recognition results."""

    state_changed = Signal(str, str)
    levels_changed = Signal(list)
    recognized = Signal(int, str)
    availability_changed = Signal()

    def __init__(self, parent=None, provider_factory=LocalSTT, source_factory=QAudioSource,
                 input_device_factory=QMediaDevices.defaultAudioInput):
        super().__init__(parent)
        self.source_factory = source_factory
        self.input_device_factory = input_device_factory
        self.worker = RecognitionWorker(provider_factory, self)
        self.worker.recognized.connect(self._on_recognized)
        self.worker.failed.connect(self._on_error)
        self.source = None
        self.input_device = None
        self.audio_format = None
        self.read_device = None
        self.token = 0
        self.enabled = False
        self.capturing = False
        self.closing = False
        self.worker.start()

    @property
    def recognizing(self):
        return self.capturing

    def start(self):
        """Enable listening through the current default input device."""
        if self.closing or self.capturing:
            return
        self.enabled = True
        self.token += 1
        self.worker.set_active_token(self.token)
        self.input_device = self.input_device_factory()
        if self.input_device is None or self.input_device.isNull():
            self._fail('No microphone input device is available. Connect a microphone and retry.')
            return
        audio_format = QAudioFormat()
        audio_format.setSampleRate(SAMPLE_RATE)
        audio_format.setChannelCount(1)
        audio_format.setSampleFormat(QAudioFormat.SampleFormat.Int16)
        if not self.input_device.isFormatSupported(audio_format):
            audio_format = self.input_device.preferredFormat()
        if (audio_format is None
                or audio_format.sampleFormat() not in (
                    QAudioFormat.SampleFormat.Int16, QAudioFormat.SampleFormat.Float,
                )
                or audio_format.channelCount() < 1 or audio_format.sampleRate() <= 0):
            self._fail('The default microphone does not support a usable PCM audio format.')
            return
        try:
            self.audio_format = audio_format
            self.source = self.source_factory(self.input_device, audio_format, self)
            self.read_device = self.source.start()
            if self.read_device is not None:
                self.read_device.readyRead.connect(self._read_audio)
        except Exception:
            self._fail('Could not start the microphone. Check microphone permission and retry.')
            return
        self.capturing = True
        self.state_changed.emit('Listening', 'Listening for your voice...')
        self.availability_changed.emit()

    def pause(self):
        """Stop capture for an Agent turn while retaining the player's enabled choice."""
        self._stop_capture(invalidate=True)
        if self.enabled and not self.closing:
            self.worker.reset()

    def stop(self):
        """Disable listening and discard all partial microphone audio."""
        self.enabled = False
        self._stop_capture(invalidate=True)
        if not self.closing:
            self.state_changed.emit('Muted', 'Microphone is muted.')
        self.availability_changed.emit()

    def shutdown(self):
        self.closing = True
        self.stop()
        self.worker.stop()

    def feed_for_test(self, samples, sample_rate):
        """Forward normalized PCM to recognition and the real input waveform."""
        if self.capturing and samples:
            self.levels_changed.emit(self._waveform_levels(samples))
            self.worker.submit(self.token, list(samples), sample_rate)

    def _read_audio(self):
        if not self.capturing or self.read_device is None:
            return
        raw = bytes(self.read_device.readAll())
        if not raw:
            return
        samples = self._pcm_samples(raw)
        if samples:
            self.feed_for_test(samples, SAMPLE_RATE)

    def _pcm_samples(self, raw):
        sample_format = self.audio_format.sampleFormat()
        width = 2 if sample_format == QAudioFormat.SampleFormat.Int16 else 4
        channels = self.audio_format.channelCount()
        raw = raw[:len(raw) - len(raw) % (width * channels)]
        values = array('h' if sample_format == QAudioFormat.SampleFormat.Int16 else 'f')
        values.frombytes(raw)
        if sys.byteorder != 'little':
            values.byteswap()
        scale = 32768 if sample_format == QAudioFormat.SampleFormat.Int16 else 1
        mono = [max(-1.0, min(1.0, sum(values[index:index + channels]) / (scale * channels)))
                for index in range(0, len(values), channels)]
        source_rate = self.audio_format.sampleRate()
        if source_rate == SAMPLE_RATE:
            return mono
        output_count = round(len(mono) * SAMPLE_RATE / source_rate)
        return [mono[min(len(mono) - 1, int(index * source_rate / SAMPLE_RATE))]
                for index in range(output_count)] if mono else []

    @staticmethod
    def _waveform_levels(samples, count=35):
        """Map real normalized PCM energy into the compact bar range."""
        if not samples:
            return [0.0] * count
        levels = []
        for index in range(count):
            start = index * len(samples) // count
            end = max(start + 1, (index + 1) * len(samples) // count)
            segment = samples[start:end]
            rms = math.sqrt(sum(sample * sample for sample in segment) / len(segment))
            levels.append(min(1.0, rms * 12.0))
        return levels

    def _stop_capture(self, invalidate):
        if invalidate:
            self.token += 1
            self.worker.set_active_token(self.token)
        if self.source is not None:
            self.source.stop()
        self.source = None
        self.read_device = None
        self.capturing = False

    def _on_recognized(self, token, text):
        if self.capturing and token == self.token and not self.closing:
            self.recognized.emit(token, text)

    def _on_error(self, token, message):
        if token == self.token and not self.closing:
            self._fail(message)

    def _fail(self, message):
        self._stop_capture(invalidate=False)
        self.enabled = False
        self.state_changed.emit('Error', message)
        self.availability_changed.emit()
