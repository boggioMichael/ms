"""Offline tests for local speech recognition contracts and microphone control."""

from pathlib import Path
import struct
import sys
from types import SimpleNamespace
import unittest

from PySide6.QtTest import QTest
from PySide6.QtMultimedia import QAudioFormat
from PySide6.QtWidgets import QApplication

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))


class RecognitionResultTests(unittest.TestCase):
    def test_recognition_result_rejects_blank_text(self):
        from ai.voice.stt.provider import RecognitionResult

        with self.assertRaises(ValueError):
            RecognitionResult('   ')

    def test_recognition_result_strips_final_text(self):
        from ai.voice.stt.provider import RecognitionResult

        self.assertEqual(RecognitionResult('  hello MapleSyrup  ').text, 'hello MapleSyrup')


class FakeStream:
    def __init__(self):
        self.frames = []

    def accept_waveform(self, sample_rate, samples):
        self.frames.append((sample_rate, list(samples)))


class FakeRecognizer:
    def __init__(self, endpoint=False, text=''):
        self.endpoint = endpoint
        self.text = text
        self.stream = FakeStream()
        self.decode_count = 0
        self.reset_count = 0

    def create_stream(self):
        return self.stream

    def is_ready(self, stream):
        return self.decode_count == 0

    def decode_stream(self, stream):
        self.decode_count += 1

    def is_endpoint(self, stream):
        return self.endpoint

    def get_result_all(self, stream):
        return SimpleNamespace(text=self.text)

    def reset(self, stream):
        self.reset_count += 1
        self.decode_count = 0


class LocalSTTTests(unittest.TestCase):
    def test_local_stt_emits_only_when_endpoint_is_detected(self):
        from ai.voice.stt.local import LocalSTT

        recognizer = FakeRecognizer(endpoint=False, text='partial words')
        stt = LocalSTT(recognizer_factory=lambda paths: recognizer)
        self.assertIsNone(stt.accept_audio([0.1] * 160, 16000))
        self.assertEqual(recognizer.stream.frames, [(16000, [0.1] * 160)])
        recognizer.endpoint = True
        recognizer.text = 'move to Henesys'
        self.assertEqual(stt.accept_audio([0.1] * 160, 16000).text, 'move to Henesys')
        self.assertEqual(recognizer.reset_count, 1)

    def test_local_stt_discards_blank_endpoint_and_resets(self):
        from ai.voice.stt.local import LocalSTT

        recognizer = FakeRecognizer(endpoint=True, text='  ')
        stt = LocalSTT(recognizer_factory=lambda paths: recognizer)
        self.assertIsNone(stt.accept_audio([0.1], 16000))
        self.assertEqual(recognizer.reset_count, 1)

    def test_local_stt_rejects_wrong_sample_rate_and_empty_audio(self):
        from ai.voice.stt.local import LocalSTT

        stt = LocalSTT(recognizer_factory=lambda paths: FakeRecognizer())
        with self.assertRaisesRegex(RuntimeError, '16 kHz'):
            stt.accept_audio([0.1], 48000)
        with self.assertRaisesRegex(ValueError, 'Audio samples'):
            stt.accept_audio([], 16000)

    def test_local_stt_reports_missing_model_files(self):
        from ai.voice.stt.local import LocalSTT

        with self.assertRaisesRegex(RuntimeError, 'missing'):
            LocalSTT(Path('/missing-maplesyrup-stt-model')).accept_audio([0.1], 16000)


class FakeSTT:
    result = 'hello MapleSyrup'

    def __init__(self):
        self.frames = []
        self.reset_count = 0

    def accept_audio(self, samples, sample_rate):
        from ai.voice.stt.provider import RecognitionResult

        self.frames.append((samples, sample_rate))
        return RecognitionResult(self.result) if self.result else None

    def reset(self):
        self.reset_count += 1


class FakeAudioSource:
    def __init__(self, device, audio_format, parent):
        self.started = False
        self.stopped = False

    def start(self):
        self.started = True

    def stop(self):
        self.stopped = True


class FakeInputDevice:
    def isNull(self):
        return False

    def isFormatSupported(self, audio_format):
        return True

    def preferredFormat(self):
        return None


class FloatInputDevice(FakeInputDevice):
    def isFormatSupported(self, audio_format):
        return False

    def preferredFormat(self):
        audio_format = QAudioFormat()
        audio_format.setSampleRate(48000)
        audio_format.setChannelCount(1)
        audio_format.setSampleFormat(QAudioFormat.SampleFormat.Float)
        return audio_format


class MicrophoneControllerTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app = QApplication.instance() or QApplication([])

    def setUp(self):
        self.recognized = []
        self.states = []
        self.mic = None

    def tearDown(self):
        if self.mic is not None:
            self.mic.shutdown()
            self.assertTrue(self.mic.worker.wait(3000))

    def create_microphone(self):
        from ui.microphone import MicrophoneController

        self.mic = MicrophoneController(
            provider_factory=FakeSTT,
            source_factory=FakeAudioSource,
            input_device_factory=FakeInputDevice,
        )
        self.mic.recognized.connect(lambda token, text: self.recognized.append((token, text)))
        self.mic.state_changed.connect(lambda state, text: self.states.append((state, text)))
        return self.mic

    def wait_for_recognition(self):
        for _ in range(100):
            if self.recognized:
                return
            QTest.qWait(10)
        self.fail('Recognition worker did not return a final result.')

    def test_final_recognition_uses_active_token_once(self):
        mic = self.create_microphone()
        mic.start()
        token = mic.token
        mic.feed_for_test([0.1] * 160, 16000)
        self.wait_for_recognition()
        self.assertEqual(self.recognized, [(token, 'hello MapleSyrup')])
        self.assertEqual(self.states[-1], ('Listening', 'Listening for your voice...'))

    def test_pause_and_stop_discard_late_recognition_and_partial_audio(self):
        mic = self.create_microphone()
        mic.start()
        old_token = mic.token
        mic.pause()
        mic._on_recognized(old_token, 'late pause text')
        self.assertEqual(self.recognized, [])
        self.assertTrue(mic.enabled)
        self.assertFalse(mic.capturing)
        mic.stop()
        mic._on_recognized(mic.token - 1, 'late stop text')
        self.assertEqual(self.recognized, [])
        self.assertFalse(mic.enabled)
        self.assertFalse(mic.capturing)

    def test_blank_recognition_and_missing_input_do_not_start_agent_turn(self):
        mic = self.create_microphone()
        FakeSTT.result = ''
        mic.start()
        mic.feed_for_test([0.1] * 160, 16000)
        QTest.qWait(50)
        self.assertEqual(self.recognized, [])
        mic.stop()
        FakeSTT.result = 'hello MapleSyrup'

        from ui.microphone import MicrophoneController
        no_input = MicrophoneController(input_device_factory=lambda: SimpleNamespace(isNull=lambda: True))
        errors = []
        no_input.state_changed.connect(lambda state, text: errors.append((state, text)))
        no_input.start()
        self.assertEqual(errors[-1][0], 'Error')
        no_input.shutdown()
        self.assertTrue(no_input.worker.wait(3000))

    def test_default_float_microphone_starts_and_converts_pcm_to_normalized_samples(self):
        from ui.microphone import MicrophoneController

        self.mic = MicrophoneController(
            provider_factory=FakeSTT,
            source_factory=FakeAudioSource,
            input_device_factory=FloatInputDevice,
        )
        self.mic.state_changed.connect(lambda state, text: self.states.append((state, text)))
        self.mic.start()

        self.assertTrue(self.mic.capturing)
        self.assertEqual(self.states[-1][0], 'Listening')
        self.assertEqual(
            self.mic._pcm_samples(struct.pack('<ffffff', 0.5, 0.0, 0.0, -0.5, 0.0, 0.0)),
            [0.5, -0.5],
        )

    def test_microphone_emits_nonzero_levels_for_non_silent_input(self):
        mic = self.create_microphone()
        levels = []
        mic.levels_changed.connect(levels.append)
        mic.start()
        mic.feed_for_test([0.5, -0.5] * 80, 16000)
        self.assertTrue(levels)
        self.assertGreater(max(levels[-1]), 0.0)


class ReadmeSetupTests(unittest.TestCase):
    def test_readme_documents_local_stt_model_and_platform_permissions(self):
        readme = (Path(__file__).resolve().parent / 'README.md').read_text()
        self.assertIn('Local speech recognition setup', readme)
        self.assertIn('sherpa-onnx-streaming-zipformer-en-2023-06-26', readme)
        self.assertIn('macOS', readme)
        self.assertIn('Windows', readme)
        self.assertIn('Microphone', readme)

    def test_voice_models_are_ignored(self):
        ignored = (Path(__file__).resolve().parents[1] / 'ai' / 'voice' / '.gitignore').read_text()
        self.assertIn('.models', ignored)


if __name__ == '__main__':
    unittest.main()
