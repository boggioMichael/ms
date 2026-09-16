"""Background Agent requests and a small text-only integration test dialog."""

import queue
from pathlib import Path

from PySide6.QtCore import QThread, Signal
from PySide6.QtGui import QTextCursor
from PySide6.QtWidgets import (
    QCheckBox, QComboBox, QDialog, QHBoxLayout, QLabel, QLineEdit, QPlainTextEdit,
    QPushButton, QVBoxLayout,
)


class AgentWorker(QThread):
    """Own the provider and Agent on one background thread, serializing requests."""

    succeeded = Signal(int, str, object)
    failed = Signal(int, str, str)
    delta = Signal(int, str)

    def __init__(self, parent=None, provider_factory=None):
        super().__init__(parent)
        self.commands = queue.Queue()
        self.provider_factory = provider_factory
        self.provider = None
        self.agent = None
        self.selection = None
        self.stopping = False
        self.version = 0

    def submit(self, operation, *args):
        if not self.stopping:
            self.version += 1
            self.commands.put((self.version, operation, args))
            return self.version

    def stop(self):
        if self.stopping:
            return
        self.stopping = True
        self.commands.put(None)

    def provider_type(self, name):
        if self.provider_factory is not None:
            return self.provider_factory(name)
        from dotenv import load_dotenv
        load_dotenv(Path(__file__).resolve().parents[1] / 'ai' / '.env')
        if name == 'Ollama':
            from ai.llm.ollama_provider import OllamaProvider
            return OllamaProvider
        if name == 'OpenAI':
            import os
            if not os.getenv('OPENAI_API_KEY', '').strip():
                raise RuntimeError('Set OPENAI_API_KEY in ai/.env before using OpenAI.')
            from ai.llm.openai_provider import OpenAIProvider
            return OpenAIProvider
        raise ValueError('Unknown provider')

    def run(self):
        try:
            while True:
                command = self.commands.get()
                if command is None or self.stopping:
                    break
                request_id, operation, args = command
                if request_id != self.version:
                    continue
                cancelled = lambda: self.stopping or request_id != self.version
                try:
                    if operation == 'models':
                        result = self.provider_type(args[0]).list_models()
                    elif operation == 'respond':
                        name, model, message = args
                        if self.selection != (name, model):
                            if self.provider is not None:
                                self.provider.close()
                            self.provider = None
                            self.agent = None
                            self.selection = None
                            provider_type = self.provider_type(name)
                            from ai.agent import Agent
                            self.provider = provider_type(model=model)
                            self.agent = Agent(self.provider)
                            self.selection = (name, model)
                        parts = []
                        for text in self.agent.respond_stream(message, cancelled=cancelled):
                            if cancelled():
                                break
                            parts.append(text)
                            self.delta.emit(request_id, text)
                        result = ''.join(parts)
                    else:
                        raise ValueError('Unknown operation')
                    if not cancelled():
                        self.succeeded.emit(request_id, operation, result)
                except RuntimeError as exc:
                    if not cancelled():
                        self.failed.emit(request_id, operation, str(exc))
                except ImportError:
                    if not cancelled():
                        self.failed.emit(request_id, operation, 'Missing dependency. Install ui/requirements.txt and restart.')
                except Exception:
                    if not cancelled():
                        self.failed.emit(request_id, operation, 'Request failed unexpectedly. Check the provider and retry.')
        finally:
            if self.provider is not None:
                self.provider.close()


class TextChatDialog(QDialog):
    """Explicit model discovery and chat; opening this dialog sends no requests."""

    presentation = Signal(str, str)
    response_started = Signal(int)
    response_delta = Signal(int, str)
    response_finished = Signal(int)
    response_failed = Signal(int)

    def __init__(self, worker, parent=None):
        super().__init__(parent)
        self.worker = worker
        self.busy = False
        self.operation = None
        self.request_id = 0
        self.full_response = ''
        self.active_selection = None
        self.setWindowTitle('MapleSyrup - Agent text test')
        self.resize(440, 420)
        self.setMinimumSize(370, 320)
        self.setStyleSheet('''
            QDialog { background: #FFF4DD; color: #57351F; }
            QLabel { color: #57351F; }
            QLineEdit, QPlainTextEdit, QComboBox {
                background: #FFFAEF; color: #57351F; border: 1px solid #D99A43;
                border-radius: 3px; padding: 6px;
            }
            QPushButton { background: #FFE5B8; color: #57351F;
                border: 1px solid #D99A43; border-radius: 3px; padding: 7px; }
            QPushButton:hover { background: #F9D69B; }
            QPushButton:focus { border: 2px solid #57351F; }
            QPushButton:disabled { color: #9B8975; background: #EEE2CD; }
        ''')
        layout = QVBoxLayout(self)
        note = QLabel('Type to the Agent. Replies can be read aloud; microphone input is off.')
        note.setWordWrap(True)
        layout.addWidget(note)
        row = QHBoxLayout()
        self.provider = QComboBox()
        self.provider.addItems(['Ollama', 'OpenAI'])
        self.provider.setAccessibleName('LLM provider')
        row.addWidget(self.provider)
        self.load = QPushButton('Load models')
        row.addWidget(self.load)
        layout.addLayout(row)
        self.model = QComboBox()
        self.model.setAccessibleName('LLM model')
        self.model.setPlaceholderText('Load models, then choose one')
        layout.addWidget(self.model)
        self.status = QLabel('Ollama uses your local server. Click Load models to connect.')
        self.status.setWordWrap(True)
        layout.addWidget(self.status)
        self.output = QPlainTextEdit()
        self.output.setReadOnly(True)
        self.output.setAccessibleName('Full Agent conversation')
        layout.addWidget(self.output)
        self.message = QLineEdit()
        self.message.setPlaceholderText('Type a message to the Agent...')
        self.message.setAccessibleName('Player message')
        layout.addWidget(self.message)
        self.send = QPushButton('Send to Agent')
        self.send.setEnabled(False)
        layout.addWidget(self.send)
        self.read_aloud = QCheckBox('Read replies aloud')
        self.read_aloud.setChecked(True)
        layout.addWidget(self.read_aloud)
        history_note = QLabel('The Agent receives the last 5 completed turns. Nothing is saved to disk.')
        history_note.setWordWrap(True)
        layout.addWidget(history_note)
        self.provider.currentTextChanged.connect(self.provider_changed)
        self.model.currentTextChanged.connect(self.update_send)
        self.message.textChanged.connect(self.update_send)
        self.load.clicked.connect(self.load_models)
        self.send.clicked.connect(self.send_message)
        self.message.returnPressed.connect(self.send_message)
        self.worker.succeeded.connect(self.on_success)
        self.worker.failed.connect(self.on_failure)
        self.worker.delta.connect(self.on_delta)

    def provider_changed(self, name):
        self.model.clear()
        self.status.setText(
            'OpenAI sends text to the selected cloud model and may incur API charges. '
            'Model lists may include models without text support.' if name == 'OpenAI'
            else 'Ollama uses your local server. Click Load models to connect.'
        )
        self.update_send()

    def update_send(self, *args):
        self.send.setEnabled((not self.busy or self.operation == 'respond') and bool(self.model.currentText())
                             and bool(self.message.text().strip()))

    def set_busy(self, busy):
        self.busy = busy
        for widget in (self.provider, self.model, self.load):
            widget.setEnabled(not busy)
        self.update_send()

    def load_models(self):
        if self.busy:
            return
        self.model.clear()
        self.set_busy(True)
        self.status.setText('Fetching models...')
        self.operation = 'models'
        self.request_id = self.worker.submit('models', self.provider.currentText())

    def send_message(self):
        if self.submit_player_message(self.message.text()) is not None:
            self.message.clear()

    def submit_player_message(self, text):
        """Submit typed or recognized player text through the shared Agent path."""
        text = text.strip()
        if (self.busy and self.operation != 'respond') or not text or not self.model.currentText():
            return None
        if self.busy:
            self.output.appendPlainText('\n[Response interrupted; not saved to Agent memory.]\n')
        selection = (self.provider.currentText(), self.model.currentText())
        if self.active_selection != selection:
            self.output.clear()
            self.active_selection = selection
            self.output.appendPlainText(f'New session: {selection[0]} / {selection[1]}\n')
        self.output.appendPlainText(f'You: {text}\n\nAgent: ')
        self.set_busy(True)
        self.status.setText(f'Waiting for {selection[0]} / {selection[1]}...')
        self.operation = 'respond'
        self.full_response = ''
        self.request_id = self.worker.submit('respond', *selection, text)
        if self.request_id is None:
            self.set_busy(False)
            return None
        self.response_started.emit(self.request_id)
        self.presentation.emit('Thinking', 'Waiting for the Agent...')
        return self.request_id

    def on_delta(self, request_id, text):
        if request_id != self.request_id:
            return
        self.full_response += text
        cursor = self.output.textCursor()
        cursor.movePosition(QTextCursor.MoveOperation.End)
        cursor.insertText(text)
        self.output.setTextCursor(cursor)
        self.response_delta.emit(request_id, text)

    def on_success(self, request_id, operation, result):
        if request_id != self.request_id:
            return
        self.set_busy(False)
        if operation == 'models':
            self.model.addItems(result)
            if result:
                self.model.setCurrentIndex(0)
            self.status.setText('Choose a model and send a message.' if result else
                                'No models found. Install an Ollama model or check provider access.')
        else:
            self.output.appendPlainText('')
            self.full_response = result
            self.status.setText('Ready. Full response is shown below.')
            self.presentation.emit('Ready', result)
            self.response_finished.emit(request_id)
            self.message.setFocus()

    def on_failure(self, request_id, operation, message):
        if request_id != self.request_id:
            return
        self.set_busy(False)
        self.status.setText(message)
        if operation == 'respond':
            self.output.appendPlainText(f'\nRequest failed: {message}\n')
            self.response_failed.emit(request_id)
            self.presentation.emit('Error', 'Request failed. See the text test window for details.')
