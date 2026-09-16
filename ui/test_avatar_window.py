"""Offline animation timing and interaction checks for the avatar overlay."""

from pathlib import Path
import sys
import tempfile
import unittest

from PySide6.QtCore import QPoint, Qt
from PySide6.QtGui import QColor, QImage
from PySide6.QtTest import QTest
from PySide6.QtWidgets import QApplication

sys.path.insert(0, str(Path(__file__).resolve().parent))

from avatar_assets import AnimationCatalog
from avatar_window import AvatarWindow, PRESS_DURATION_MS


def write_frame(path: Path, color: QColor = QColor('#D99A43')) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    image = QImage(40, 20, QImage.Format.Format_ARGB32_Premultiplied)
    image.fill(color)
    assert image.save(str(path))


class AvatarWindowTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app = QApplication.instance() or QApplication([])

    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        root = Path(self.directory.name)
        for name in ('idle_01', 'idle_02'):
            for index in range(4):
                write_frame(root / 'idle' / name / f'frame_{index + 1:04}.png')
        self.clock = [10.0]
        self.window = AvatarWindow(AnimationCatalog(root), reduced_motion=False,
                                   clock=lambda: self.clock[0], random_choice=lambda items: items[-1])
        self.window.show()
        self.app.processEvents()

    def tearDown(self):
        self.window.close()
        self.directory.cleanup()

    def test_elapsed_time_selects_frames_without_timer_drift(self):
        self.window.set_state('idle')
        self.clock[0] = 10.21

        self.assertEqual(self.window.frame_index, 3)

    def test_repeated_state_signal_does_not_restart_active_animation(self):
        self.clock[0] = 10.21
        self.window.set_state('idle')

        self.assertEqual(self.window.frame_index, 3)

    def test_missing_state_uses_idle_sequence(self):
        self.window.set_state('speaking')

        self.assertEqual(self.window.state, 'speaking')
        self.assertEqual(self.window.active_sequence.state, 'idle')

    def test_idle_wrap_uses_an_alternative_sequence_when_available(self):
        self.window.set_state('idle')
        first = self.window.active_sequence.name

        self.window.advance_after_wrap()

        self.assertNotEqual(self.window.active_sequence.name, first)

    def test_short_left_click_emits_microphone_toggle(self):
        toggles = []
        self.window.microphone_toggled.connect(lambda: toggles.append(True))

        QTest.mouseClick(self.window, Qt.MouseButton.LeftButton, pos=QPoint(20, 20))

        self.assertEqual(toggles, [True])

    def test_short_click_has_temporary_pressed_feedback(self):
        QTest.mousePress(self.window, Qt.MouseButton.LeftButton, pos=QPoint(20, 20))
        self.assertTrue(self.window.pressed)

        QTest.mouseRelease(self.window, Qt.MouseButton.LeftButton, pos=QPoint(20, 20))
        self.assertTrue(self.window.pressed)

        QTest.qWait(PRESS_DURATION_MS + 40)
        self.assertFalse(self.window.pressed)

    def test_display_timer_refreshes_every_eight_milliseconds(self):
        self.assertEqual(self.window.timer.interval(), 8)

    def test_drag_does_not_emit_microphone_toggle(self):
        toggles = []
        self.window.microphone_toggled.connect(lambda: toggles.append(True))

        QTest.mousePress(self.window, Qt.MouseButton.LeftButton, pos=QPoint(20, 20))
        QTest.mouseMove(self.window, QPoint(45, 20))
        QTest.mouseRelease(self.window, Qt.MouseButton.LeftButton, pos=QPoint(45, 20))

        self.assertEqual(toggles, [])

    def test_context_menu_exposes_required_actions(self):
        labels = [action.text() for action in self.window.menu.actions()]

        self.assertIn('Settings', labels)
        self.assertIn('Captions', labels)
        self.assertIn('Mute voice', labels)
        self.assertIn('Exit', labels)


if __name__ == '__main__':
    unittest.main()
