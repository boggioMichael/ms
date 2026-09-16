"""Offline tests of real Agent orchestration with a simulated slow provider."""

import sys
import os
import subprocess
import threading
import time
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from PySide6.QtCore import QTimer
from PySide6.QtTest import QTest
from PySide6.QtWidgets import QApplication

from agent_chat import AgentWorker, TextChatDialog


class FakeProvider:
    instances = []

    def __init__(self, model):
        self.model = model
        self.calls = []
        self.closed = False
        self.threads = []
        self.instances.append(self)

    @classmethod
    def list_models(cls):
        time.sleep(0.08)
        return ['test-model', 'second-model']

    def generate(self, instructions, user_input, history=None):
        self.threads.append(threading.get_ident())
        self.calls.append((user_input, history))
        time.sleep(0.08)
        if user_input == 'fail':
            raise RuntimeError('Service unavailable; try again.')
        return '<plain text> ' + user_input

    def close(self):
        self.closed = True


class AgentChatTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app = QApplication.instance() or QApplication([])

    def setUp(self):
        FakeProvider.instances = []
        self.worker = AgentWorker(provider_factory=lambda name: FakeProvider)
        self.dialog = TextChatDialog(self.worker)
        self.worker.start()
        self.dialog.show()
        self.frames = []
        self.dialog.presentation.connect(lambda state, text: self.frames.append((state,text)))

    def tearDown(self):
        self.dialog.close()
        self.worker.stop()
        self.assertTrue(self.worker.wait(3000))
        self.app.processEvents()

    def wait_until(self, predicate):
        for _ in range(200):
            if predicate():
                return
            QTest.qWait(10)
        self.fail('Timed out waiting for the worker')

    def discover(self):
        self.dialog.load_models()
        self.wait_until(lambda: not self.dialog.busy)
        self.assertEqual(self.dialog.model.count(), 2)

    def send(self, text):
        self.dialog.message.setText(text)
        self.dialog.send_message()
        self.wait_until(lambda: not self.dialog.busy)

    def test_background_requests_memory_and_literal_text(self):
        self.assertEqual(FakeProvider.instances, [])
        beats = []
        heartbeat = QTimer()
        heartbeat.setInterval(5)
        heartbeat.timeout.connect(lambda: beats.append(1))
        heartbeat.start()
        self.discover()
        self.send('hello')
        self.send('follow up')
        heartbeat.stop()
        self.assertGreater(len(beats), 3)
        provider = FakeProvider.instances[0]
        self.assertTrue(all(t != threading.get_ident() for t in provider.threads))
        self.assertEqual(provider.calls[1][1], [
            {'role':'user','content':'hello'},
            {'role':'assistant','content':'<plain text> hello'},
        ])
        self.assertIn('<plain text> hello', self.dialog.output.toPlainText())
        self.assertEqual([f[0] for f in self.frames], ['Thinking','Ready','Thinking','Ready'])

    def test_failure_recovery_and_model_change(self):
        self.discover()
        self.send('hello')
        self.send('fail')
        self.assertEqual(self.frames[-1][0], 'Error')
        self.send('retry')
        self.assertEqual(len(FakeProvider.instances[0].calls[-1][1]), 2)
        self.dialog.model.setCurrentIndex(1)
        self.send('new session')
        self.assertTrue(FakeProvider.instances[0].closed)
        self.assertEqual(FakeProvider.instances[1].calls[0][1], [])

    def test_replacement_requests_and_shutdown(self):
        self.discover()
        self.dialog.message.setText('one')
        self.dialog.send_message()
        self.dialog.message.setText('two')
        self.dialog.send_message()
        self.assertFalse(self.dialog.provider.isEnabled())
        self.wait_until(lambda: not self.dialog.busy)
        self.assertEqual(FakeProvider.instances[0].calls[-1][0], 'two')
        self.dialog.send_message()
        QTest.qWait(20)
        count = len(self.frames)
        self.worker.stop()
        self.wait_until(lambda: not self.worker.isRunning())
        self.app.processEvents()
        self.assertEqual(len(self.frames), count)
        self.assertTrue(FakeProvider.instances[0].closed)

    def test_recognized_message_uses_same_agent_request_path_as_typed_text(self):
        self.discover()
        request_id = self.dialog.submit_player_message('  where should I train?  ')
        self.assertEqual(request_id, self.dialog.request_id)
        self.wait_until(lambda: not self.dialog.busy)
        self.assertEqual(FakeProvider.instances[0].calls[-1][0], 'where should I train?')
        self.assertIn('You: where should I train?', self.dialog.output.toPlainText())


class ApplicationShutdownTests(unittest.TestCase):
    def test_pending_request_exits_application(self):
        script = '''
from PySide6.QtCore import QTimer
from PySide6.QtWidgets import QApplication
import main
from agent_chat import AgentWorker, TextChatDialog
from avatar_window import AvatarWindow
from test_agent_chat import FakeProvider
app = QApplication([])
main.QApplication = lambda args: app
main.AgentWorker = lambda parent: AgentWorker(parent, provider_factory=lambda name: FakeProvider)
closed = []
def start_request_and_close():
    window = next(w for w in app.topLevelWidgets() if isinstance(w, AvatarWindow))
    dialog = window.findChild(TextChatDialog)
    window.text_chat_action.trigger()
    dialog.model.addItem('test-model')
    dialog.model.setCurrentIndex(0)
    dialog.message.setText('hello')
    dialog.send_message()
    window.close()
    assert not window.isVisible()
    closed.append(True)
QTimer.singleShot(50, start_request_and_close)
QTimer.singleShot(4000, lambda: app.exit(2))
assert main.main() == 0 and closed
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
