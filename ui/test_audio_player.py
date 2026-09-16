"""Offline voice lifecycle tests with no model download or speaker playback."""

from array import array
from pathlib import Path
import sys
import time
import unittest
from unittest.mock import Mock, patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from PySide6.QtMultimedia import QMediaPlayer
from PySide6.QtTest import QTest
from PySide6.QtWidgets import QApplication

from ai.voice.local_tts import LocalTTS
from ai.voice.tts import AudioClip
from audio_player import VoiceTest, audio_envelope, wave_levels


def clip():
    return AudioClip(b'\x00\x00' * 480 + b'\x00\x40' * 480, 24000)


class FakeTTS:
    requests = []

    def synthesize(self, text):
        self.requests.append(text)
        time.sleep(0.1)
        return clip()


class VoiceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app = QApplication.instance() or QApplication([])

    def setUp(self):
        FakeTTS.requests = []
        self.voice = VoiceTest(provider_factory=FakeTTS)
        self.voice.player = Mock()
        self.voice.player.playbackState.return_value = QMediaPlayer.PlaybackState.PlayingState
        self.voice.player.position.return_value = 20
        self.frames = []
        self.voice.frame.connect(lambda *frame: self.frames.append(frame))
        self.device = patch('audio_player.QMediaDevices.defaultAudioOutput')
        self.device.start().return_value.isNull.return_value = False

    def tearDown(self):
        self.voice.shutdown()
        self.assertTrue(self.voice.worker.wait(3000))
        self.device.stop()
        self.app.processEvents()

    def wait_generated(self):
        for _ in range(100):
            if not self.voice.generating:
                return
            QTest.qWait(10)
        self.fail('Voice worker did not finish')

    def test_pcm_contract_and_timeline(self):
        envelope = audio_envelope(clip())
        self.assertEqual(envelope, [0.0, 1.0])
        self.assertEqual(wave_levels(envelope, 0), [0.0] * 35)
        self.assertEqual(wave_levels(envelope, 20)[-1], 1.0)
        with self.assertRaises(ValueError):
            AudioClip(b'\x00', 24000)
        with self.assertRaises(RuntimeError):
            LocalTTS(Path('/nonexistent-maplesyrup-test-model')).synthesize('hello')

    def test_speaking_motion_and_end(self):
        self.voice.start()
        self.assertEqual(self.frames[-1][0], 'Thinking')
        self.wait_generated()
        self.voice.player.play.assert_called_once()
        self.assertTrue(self.voice.buffer.isOpen())
        self.voice.reduced_motion = False
        self.voice.update_frame()
        self.assertEqual(self.frames[-1][0], 'Speaking')
        self.assertEqual(self.frames[-1][1][-1], 1.0)
        self.voice.reduced_motion = True
        self.voice.update_frame()
        self.assertEqual(self.frames[-1][1], [0.0] * 35)
        self.voice.on_media_status(QMediaPlayer.MediaStatus.EndOfMedia)
        self.assertFalse(self.voice.active)
        self.assertIsNone(self.voice.buffer)
        self.assertEqual(self.frames[-1][0], 'Ready')
        self.assertFalse(self.voice.timer.isActive())

    def test_playback_emits_smoothed_pcm_speech_level_and_resets_on_stop(self):
        levels = []
        self.voice.speech_level.connect(levels.append)
        self.voice.start()
        self.wait_generated()

        self.voice.update_frame()

        self.assertGreater(levels[-1], 0.0)
        self.assertLessEqual(levels[-1], 1.0)
        self.voice.stop()
        self.assertEqual(levels[-1], 0.0)

    def test_voice_mute_changes_output_volume_without_stopping_playback(self):
        self.voice.start()
        self.wait_generated()

        self.voice.set_muted(True)

        self.assertTrue(self.voice.muted)
        self.assertEqual(self.voice.output.volume(), 0.0)
        self.assertTrue(self.voice.active)

    def test_cancel_generation_does_not_play_late_result(self):
        self.voice.start()
        self.voice.stop()
        self.wait_generated()
        self.voice.player.play.assert_not_called()
        self.assertFalse(self.voice.active)
        self.voice.start()
        self.wait_generated()
        self.voice.player.play.assert_called_once()

    def test_missing_device_and_playback_error(self):
        with patch('audio_player.QMediaDevices.defaultAudioOutput') as device:
            device.return_value.isNull.return_value = True
            self.voice.start()
        self.assertEqual(self.frames[-1][0], 'Error')
        self.assertFalse(self.voice.generating)
        self.voice.start()
        self.wait_generated()
        self.voice.on_playback_error(None, '')
        self.assertEqual(self.frames[-1][0], 'Error')
        self.assertFalse(self.voice.active)
        self.assertIsNone(self.voice.buffer)

    def test_custom_reply_and_latest_pending_request(self):
        self.voice.start('First reply')
        QTest.qWait(20)
        self.voice.start('Superseded reply')
        self.voice.start('Latest reply')
        self.wait_generated()
        self.assertEqual(FakeTTS.requests, ['First reply', 'Latest reply'])
        self.voice.player.play.assert_called_once()
        self.voice.update_frame()
        self.assertEqual(self.frames[-1][2], 'Latest reply')

    def test_stop_discards_pending_reply(self):
        self.voice.start('First reply')
        QTest.qWait(20)
        self.voice.start('Canceled reply')
        self.voice.stop()
        self.wait_generated()
        self.assertEqual(FakeTTS.requests, ['First reply'])
        self.voice.player.play.assert_not_called()


if __name__ == '__main__':
    unittest.main()
