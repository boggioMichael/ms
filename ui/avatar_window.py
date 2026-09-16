"""Transparent animated dog overlay with desktop interaction controls."""

import math
import random
import subprocess
import sys
import time

from PySide6.QtCore import QPoint, QRect, Qt, QTimer, Signal
from PySide6.QtGui import QActionGroup, QColor, QPainter
from PySide6.QtWidgets import QMenu, QWidget

from avatar_assets import AnimationCatalog, AnimationSequence, FrameCache


CANVAS_WIDTH = 240
CANVAS_HEIGHT = 135
STROKE_COLOR = QColor('#57351F')
STROKE_WIDTH = 1
DRAG_DISTANCE = 6
PRESS_DURATION_MS = 100
PRESSED_SCALE = 0.98
PRESSED_OPACITY = 0.72
TRANSITION_DELAYS_MS = {'idle': 0, 'listening': 0, 'thinking': 0, 'speaking': 0}


def prefers_reduced_motion() -> bool:
    """Use system motion preference when Qt has no portable equivalent."""
    if sys.platform == 'darwin':
        try:
            result = subprocess.run(
                ['defaults', 'read', 'com.apple.universalaccess', 'reduceMotion'],
                capture_output=True, text=True, timeout=1, check=False,
            )
            if result.returncode == 0:
                return result.stdout.strip() in ('1', 'true', 'YES')
        except (OSError, subprocess.TimeoutExpired):
            pass
    elif sys.platform == 'win32':
        import ctypes
        enabled = ctypes.c_int()
        if ctypes.windll.user32.SystemParametersInfoW(0x1042, 0, ctypes.byref(enabled), 0):
            return not bool(enabled.value)
    return False


class AvatarWindow(QWidget):
    """Render a time-based avatar sequence without coupling it to application state."""

    microphone_toggled = Signal()
    settings_requested = Signal()
    test_voice_requested = Signal()
    stop_voice_requested = Signal()
    motion_requested = Signal(bool)
    captions_toggled = Signal(bool)
    voice_muted = Signal(bool)
    exit_requested = Signal()
    demo_state_requested = Signal(str)
    moved = Signal(QRect)
    closed = Signal()

    def __init__(self, catalog: AnimationCatalog, reduced_motion: bool,
                 clock=time.monotonic, random_choice=random.choice, parent=None):
        super().__init__(parent)
        self.catalog = catalog
        self.clock = clock
        self.random_choice = random_choice
        self.cache = FrameCache(self.size_hint())
        self.reduced_motion = reduced_motion
        self.state = 'idle'
        self.active_sequence = None
        self.sequence_started = self.clock()
        self._last_frame_index = 0
        self._image = None
        self._outline_image = None
        self._pending_state = None
        self._pending_at = None
        self._speech_level = 0.0
        self._press_global = None
        self._window_origin = None
        self._dragged = False
        self.pressed = False
        self.can_close = None
        self.setWindowTitle('MapleSyrup')
        self.setWindowFlags(Qt.WindowType.Tool | Qt.WindowType.FramelessWindowHint
                            | Qt.WindowType.WindowStaysOnTopHint)
        self.setAttribute(Qt.WidgetAttribute.WA_TranslucentBackground)
        self.setFixedSize(self.size_hint())
        self.setMouseTracking(True)
        self._create_menu()
        self.timer = QTimer(self)
        self.timer.setInterval(8)
        self.timer.timeout.connect(self.tick)
        self._press_feedback_timer = QTimer(self)
        self._press_feedback_timer.setSingleShot(True)
        self._press_feedback_timer.timeout.connect(self._clear_pressed)
        self._activate('idle')
        self.timer.start()

    @staticmethod
    def size_hint():
        from PySide6.QtCore import QSize
        return QSize(CANVAS_WIDTH, CANVAS_HEIGHT)

    @property
    def frame_index(self) -> int:
        if self.active_sequence is None or not self.active_sequence.frames:
            return 0
        elapsed = max(0.0, self.clock() - self.sequence_started)
        return int(elapsed * self.active_sequence.fps) % len(self.active_sequence.frames)

    def set_state(self, state: str) -> None:
        state = state.lower()
        if state not in TRANSITION_DELAYS_MS:
            state = 'idle'
        if state == self.state and self._pending_state is None:
            return
        delay = TRANSITION_DELAYS_MS[state] / 1000.0
        if delay:
            self._pending_state = state
            self._pending_at = self.clock() + delay
            return
        self._activate(state)

    def set_speech_level(self, level: float) -> None:
        self._speech_level = max(0.0, min(1.0, level))

    def set_captions_visible(self, visible: bool) -> None:
        self.captions_action.blockSignals(True)
        self.captions_action.setChecked(visible)
        self.captions_action.blockSignals(False)

    def set_voice_muted(self, muted: bool) -> None:
        self.mute_voice_action.blockSignals(True)
        self.mute_voice_action.setChecked(muted)
        self.mute_voice_action.blockSignals(False)

    def set_reduced_motion(self, enabled: bool) -> None:
        self.reduced_motion = enabled
        self.motion_action.blockSignals(True)
        self.motion_action.setChecked(enabled)
        self.motion_action.blockSignals(False)

    def advance_after_wrap(self) -> None:
        """Select another Idle sequence at its loop boundary when possible."""
        if self.state != 'idle':
            return
        choices = list(self.catalog.sequences('idle'))
        if len(choices) > 1 and self.active_sequence is not None:
            choices = [choice for choice in choices if choice != self.active_sequence]
        if choices:
            self._activate_sequence(self.random_choice(choices))

    def tick(self) -> None:
        if self._pending_at is not None and self.clock() >= self._pending_at:
            state = self._pending_state
            self._pending_state = self._pending_at = None
            self._activate(state)
        index = self.frame_index
        if self.state == 'idle' and self.active_sequence is not None and index < self._last_frame_index:
            self.advance_after_wrap()
            index = self.frame_index
        self._last_frame_index = index
        self._set_image(self.cache.frame(self.active_sequence, index) if self.active_sequence else None)
        self.update()

    def _activate(self, state: str) -> None:
        self.state = state
        sequence = self.catalog.fallback(state)
        self._activate_sequence(sequence)

    def _activate_sequence(self, sequence: AnimationSequence | None) -> None:
        self.active_sequence = sequence
        self.sequence_started = self.clock()
        self._last_frame_index = 0
        if sequence is not None:
            for index in range(len(sequence.frames)):
                self.cache.frame(sequence, index)
            self._set_image(self.cache.frame(sequence, 0))
        else:
            self._set_image(None)
        self.update()

    def _set_image(self, image) -> None:
        if image is self._image:
            return
        self._image = image
        self._outline_image = self._tinted_outline(image) if image is not None else None

    def _create_menu(self) -> None:
        self.menu = QMenu(self)
        self.menu.setStyleSheet('''
            QMenu { background: #FFF4DD; color: #57351F; border: 1px solid #D99A43;
                    padding: 5px; font-size: 13px; }
            QMenu::item { padding: 6px 24px; }
            QMenu::item:selected { background: #FFE5B8; }
            QMenu::separator { height: 1px; background: #E7C99C; margin: 4px; }
        ''')
        settings = self.menu.addMenu('Settings')
        self.text_chat_action = settings.addAction('Agent text test...')
        self.text_chat_action.triggered.connect(self.settings_requested)
        self.voice_test_action = settings.addAction('Test voice')
        self.voice_test_action.triggered.connect(self.test_voice_requested)
        self.stop_voice_action = settings.addAction('Stop speech')
        self.stop_voice_action.triggered.connect(self.stop_voice_requested)
        self.motion_action = settings.addAction('Reduce motion')
        self.motion_action.setCheckable(True)
        self.motion_action.setChecked(self.reduced_motion)
        self.motion_action.toggled.connect(self.motion_requested)
        demo = settings.addMenu('Demo state')
        group = QActionGroup(self)
        self.state_actions = {}
        for state in ('idle', 'listening', 'thinking', 'speaking'):
            action = demo.addAction(state.title())
            action.setCheckable(True)
            action.setChecked(state == 'idle')
            group.addAction(action)
            action.triggered.connect(lambda checked=False, value=state: self.demo_state_requested.emit(value))
            self.state_actions[state] = action
        self.captions_action = self.menu.addAction('Captions')
        self.captions_action.setCheckable(True)
        self.captions_action.toggled.connect(self.captions_toggled)
        self.mute_voice_action = self.menu.addAction('Mute voice')
        self.mute_voice_action.setCheckable(True)
        self.mute_voice_action.toggled.connect(self.voice_muted)
        self.menu.addSeparator()
        self.menu.addAction('Exit').triggered.connect(self.exit_requested)

    def paintEvent(self, event) -> None:
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.SmoothPixmapTransform)
        offset = 0.0 if self.reduced_motion else math.sin(self.clock() * 2.2) * 3.0
        painter.translate(0, offset)
        if self.pressed:
            painter.translate(CANVAS_WIDTH / 2, CANVAS_HEIGHT / 2)
            painter.scale(PRESSED_SCALE, PRESSED_SCALE)
            painter.translate(-CANVAS_WIDTH / 2, -CANVAS_HEIGHT / 2)
            painter.setOpacity(PRESSED_OPACITY)
        if self._image is not None:
            for dx, dy in ((-STROKE_WIDTH, 0), (STROKE_WIDTH, 0), (0, -STROKE_WIDTH),
                           (0, STROKE_WIDTH), (-STROKE_WIDTH, -STROKE_WIDTH),
                           (-STROKE_WIDTH, STROKE_WIDTH), (STROKE_WIDTH, -STROKE_WIDTH),
                           (STROKE_WIDTH, STROKE_WIDTH)):
                painter.drawImage(dx, dy, self._outline_image)
            painter.drawImage(0, 0, self._image)
            return
        painter.setPen(Qt.PenStyle.NoPen)
        painter.setBrush(QColor('#FFE5B8'))
        painter.drawEllipse(85, 25, 70, 70)
        painter.drawEllipse(82, 22, 25, 28)
        painter.drawEllipse(133, 22, 25, 28)
        painter.setBrush(QColor('#57351F'))
        painter.drawEllipse(113, 60, 14, 10)

    @staticmethod
    def _tinted_outline(image):
        outline = image.copy()
        painter = QPainter(outline)
        painter.setCompositionMode(QPainter.CompositionMode.CompositionMode_SourceIn)
        painter.fillRect(outline.rect(), STROKE_COLOR)
        painter.end()
        return outline

    def mousePressEvent(self, event) -> None:
        if event.button() == Qt.MouseButton.RightButton:
            self.menu.popup(event.globalPosition().toPoint())
            event.accept()
            return
        if event.button() == Qt.MouseButton.LeftButton:
            self._press_global = event.globalPosition().toPoint()
            self._window_origin = self.pos()
            self._dragged = False
            self._press_feedback_timer.stop()
            self.pressed = True
            self.update()
            event.accept()
            return
        super().mousePressEvent(event)

    def mouseMoveEvent(self, event) -> None:
        if self._press_global is None:
            return super().mouseMoveEvent(event)
        delta = event.globalPosition().toPoint() - self._press_global
        if not self._dragged and delta.manhattanLength() >= DRAG_DISTANCE:
            self._dragged = True
            self._clear_pressed()
        if self._dragged:
            self.move(self._window_origin + delta)
        event.accept()

    def mouseReleaseEvent(self, event) -> None:
        if event.button() == Qt.MouseButton.LeftButton and self._press_global is not None:
            toggle = not self._dragged
            self._press_global = self._window_origin = None
            self._dragged = False
            if toggle:
                self.microphone_toggled.emit()
                self._press_feedback_timer.start(PRESS_DURATION_MS)
            else:
                self._clear_pressed()
            event.accept()
            return
        super().mouseReleaseEvent(event)

    def _clear_pressed(self) -> None:
        self._press_feedback_timer.stop()
        self.pressed = False
        self.update()

    def moveEvent(self, event) -> None:
        super().moveEvent(event)
        self.moved.emit(self.geometry())

    def closeEvent(self, event) -> None:
        if self.can_close is not None and not self.can_close():
            event.ignore()
            return
        self.timer.stop()
        self.closed.emit()
        event.accept()
