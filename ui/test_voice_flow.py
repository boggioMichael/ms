"""Application-level streaming/cancellation checks without network or speakers."""

import os
from pathlib import Path
import subprocess
import sys
import unittest


class VoiceFlowTests(unittest.TestCase):
    def test_streaming_speech_and_cancellation_modes(self):
        script = r'''
import sys, threading, time
from unittest.mock import Mock, patch
from PySide6.QtCore import QTimer
from PySide6.QtMultimedia import QMediaPlayer
from PySide6.QtWidgets import QApplication
import main
from agent_chat import AgentWorker, TextChatDialog
from audio_player import VoiceTest, TEST_TEXT
from ai.voice.tts import AudioClip
from avatar_window import AvatarWindow
scenario = sys.argv[1]
release = threading.Event()
class Provider:
    def __init__(self, model): pass
    def close(self): pass
    def stream(self, user_input, **kwargs):
        yield user_input + ' first. '
        release.wait(3)
        yield user_input + ' second.'
class TTS:
    def synthesize(self, text):
        time.sleep(.025)
        return AudioClip(b'\0\x20'.decode('unicode_escape').encode('latin1') * 2400, 24000)
app = QApplication([])
main.QApplication = lambda args: app
main.AgentWorker = lambda parent: AgentWorker(parent, provider_factory=lambda name: Provider)
spoken = []
voice_ref = []
def make_voice(parent):
    voice = VoiceTest(parent, provider_factory=TTS)
    voice.player = Mock()
    voice.player.playbackState.return_value = QMediaPlayer.PlaybackState.PlayingState
    voice.player.position.return_value = 20
    def play():
        token, buffer = voice.token, voice.buffer
        def finish():
            if token == voice.token and buffer is voice.buffer:
                voice.update_frame()
                voice.on_media_status(QMediaPlayer.MediaStatus.EndOfMedia)
        QTimer.singleShot(90, finish)
    voice.player.play.side_effect = play
    voice.captions.connect(lambda previous,current: spoken.append(current) if current else None)
    voice_ref.append(voice)
    return voice
main.VoiceTest = make_voice
stage = 0
passed = []
errors = []
def tick():
    global stage
    try:
        window = next(w for w in app.topLevelWidgets() if isinstance(w, AvatarWindow))
        dialog = window.findChild(TextChatDialog)
        voice = voice_ref[0]
        if stage == 0:
            window.text_chat_action.trigger()
            dialog.model.addItem('test'); dialog.model.setCurrentIndex(0)
            dialog.message.setText('Old'); dialog.send_message()
            stage = 1
        elif stage == 1 and 'Old first.' in spoken:
            # Real incremental provider output reaches TTS before the stream ends.
            assert dialog.busy and dialog.full_response == 'Old first. '
            assert not dialog.worker.agent.memory.turns
            if scenario == 'stop': window.stop_voice_action.trigger()
            elif scenario == 'off': dialog.read_aloud.setChecked(False)
            elif scenario == 'demo': window.state_actions['listening'].trigger()
            elif scenario == 'test': window.voice_test_action.trigger()
            elif scenario == 'new':
                dialog.message.setText('New'); dialog.send_message()
            elif scenario == 'close':
                window.close()
                assert not voice.active
                passed.append(True)
                timer.stop()
            release.set()
            stage = 2
        elif stage == 2 and not dialog.busy and not voice.active and not voice.generating:
            assert 'Old second.' not in spoken
            if scenario == 'new':
                assert spoken[-2:] == ['New first.', 'New second.'], spoken
                assert dialog.full_response == 'New first. New second.'
                assert len(dialog.worker.agent.memory.turns) == 1
                assert dialog.worker.agent.memory.turns[0]['user'] == 'New'
            else:
                assert dialog.full_response == 'Old first. Old second.'
                assert dialog.full_response in dialog.output.toPlainText()
                assert len(dialog.worker.agent.memory.turns) == 1
            if scenario == 'demo': assert window.state == 'idle'
            if scenario == 'test': assert TEST_TEXT in spoken
            passed.append(True); timer.stop(); window.close()
    except Exception as exc:
        errors.append(repr(exc)); timer.stop(); release.set(); app.exit(2)
timer = QTimer(); timer.timeout.connect(tick); timer.start(10)
QTimer.singleShot(6000, lambda: (release.set(), app.exit(3)))
with patch('audio_player.QMediaDevices.defaultAudioOutput') as device:
    device.return_value.isNull.return_value = False
    result = main.main()
assert result == 0 and passed and not errors, (result, errors, spoken)
'''
        for scenario in ('stop', 'off', 'demo', 'test', 'new', 'close'):
            with self.subTest(scenario=scenario):
                result = subprocess.run(
                    [sys.executable, '-B', '-', scenario], input=script, text=True,
                    cwd=Path(__file__).resolve().parent,
                    env={**os.environ, 'QT_QPA_PLATFORM': 'offscreen'},
                    capture_output=True, timeout=10,
                )
                self.assertEqual(result.returncode, 0, result.stderr)


class MicrophoneConversationTests(unittest.TestCase):
    def test_recognition_pauses_for_agent_speech_then_returns_to_listening(self):
        script = r'''
import sys, time
from unittest.mock import Mock, patch
from PySide6.QtCore import QObject, QTimer, Signal
from PySide6.QtMultimedia import QMediaPlayer
from PySide6.QtWidgets import QApplication
import main
from agent_chat import AgentWorker, TextChatDialog
from audio_player import VoiceTest
from ai.voice.tts import AudioClip
from avatar_window import AvatarWindow

class Provider:
    def __init__(self, model): pass
    def close(self): pass
    def stream(self, user_input, **kwargs):
        yield 'I heard ' + user_input + '.'

class TTS:
    def synthesize(self, text):
        return AudioClip(b'\0\x20'.decode('unicode_escape').encode('latin1') * 1200, 24000)

class FakeWorker(QObject):
    finished = Signal()
    def wait(self): return True

class FakeMicrophone(QObject):
    state_changed = Signal(str, str)
    levels_changed = Signal(list)
    recognized = Signal(int, str)
    availability_changed = Signal()
    def __init__(self, parent):
        super().__init__(parent)
        self.worker = FakeWorker(self)
        self.token = 0
        self.enabled = False
        self.capturing = False
        self.calls = []
    def start(self):
        self.token += 1; self.enabled = self.capturing = True; self.calls.append('start')
        self.state_changed.emit('Listening', 'Listening for your voice...')
    def pause(self):
        self.token += 1; self.capturing = False; self.calls.append('pause')
    def stop(self):
        self.token += 1; self.enabled = self.capturing = False; self.calls.append('stop')
        self.state_changed.emit('Muted', 'Microphone is muted.')
    def shutdown(self): self.stop(); self.worker.finished.emit()
    def emit_recognized(self, text): self.recognized.emit(self.token, text)

app = QApplication([])
main.QApplication = lambda args: app
main.AgentWorker = lambda parent: AgentWorker(parent, provider_factory=lambda name: Provider)
microphones = []
def make_microphone(parent):
    microphone = FakeMicrophone(parent); microphones.append(microphone); return microphone
main.MicrophoneController = make_microphone
def make_voice(parent):
    voice = VoiceTest(parent, provider_factory=TTS)
    voice.player = Mock()
    voice.player.playbackState.return_value = QMediaPlayer.PlaybackState.PlayingState
    voice.player.position.return_value = 20
    def play():
        token, buffer = voice.token, voice.buffer
        QTimer.singleShot(30, lambda: token == voice.token and buffer is voice.buffer and (
            voice.update_frame(), voice.on_media_status(QMediaPlayer.MediaStatus.EndOfMedia)))
    voice.player.play.side_effect = play
    return voice
main.VoiceTest = make_voice
stage = 0
errors = []
def tick():
    global stage
    try:
        window = next(w for w in app.topLevelWidgets() if isinstance(w, AvatarWindow))
        dialog = window.findChild(TextChatDialog)
        assert microphones
        microphone = microphones[0]
        if stage == 0:
            dialog.model.addItem('test'); dialog.model.setCurrentIndex(0)
            window.microphone_toggled.emit()
            assert microphone.enabled and microphone.capturing and window.state == 'listening'
            microphone.emit_recognized('where should I train')
            stage = 1
        elif stage == 1:
            assert microphone.calls[-1] == 'pause' and not microphone.capturing
            stage = 2
        elif stage == 2 and not dialog.busy and microphone.capturing:
            assert microphone.enabled and microphone.calls.count('start') == 2
            assert 'You: where should I train' in dialog.output.toPlainText()
            assert 'I heard where should I train.' in dialog.output.toPlainText()
            timer.stop(); window.close()
    except Exception as exc:
        errors.append(f'stage={stage}, state={window.state}, calls={microphone.calls}: {exc!r}'); timer.stop(); app.exit(2)
timer = QTimer(); timer.timeout.connect(tick); timer.start(10)
QTimer.singleShot(5000, lambda: app.exit(3))
with patch('audio_player.QMediaDevices.defaultAudioOutput') as device:
    device.return_value.isNull.return_value = False
    result = main.main()
assert result == 0 and not errors, (result, errors)
'''
        result = subprocess.run(
            [sys.executable, '-B', '-'], input=script, text=True,
            cwd=Path(__file__).resolve().parent,
            env={**os.environ, 'QT_QPA_PLATFORM': 'offscreen'},
            capture_output=True, timeout=8,
        )
        self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == '__main__':
    unittest.main()
