"""Offline compact-caption checks for the avatar speech bubble."""

from pathlib import Path
import sys
import unittest

from PySide6.QtCore import QRect
from PySide6.QtTest import QTest
from PySide6.QtWidgets import QApplication

sys.path.insert(0, str(Path(__file__).resolve().parent))

from speech_bubble import SpeechBubble


class SpeechBubbleTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app = QApplication.instance() or QApplication([])

    def setUp(self):
        self.bubble = SpeechBubble()

    def tearDown(self):
        self.bubble.close()

    def test_bubble_shows_only_for_enabled_active_speech(self):
        self.bubble.set_caption('Current sentence.', speaking=True, enabled=True)

        self.assertTrue(self.bubble.isVisible())
        self.assertEqual(self.bubble.label.text(), 'Current sentence.')

        self.bubble.set_caption('Current sentence.', speaking=False, enabled=True)
        QTest.qWait(180)

        self.assertFalse(self.bubble.isVisible())

    def test_bubble_clamps_to_available_screen_edges(self):
        self.bubble.set_caption('Current sentence.', speaking=True, enabled=True)
        available = QRect(0, 0, 1000, 700)

        self.bubble.follow_avatar(QRect(900, 10, 240, 135), available)

        self.assertGreaterEqual(self.bubble.geometry().left(), 0)
        self.assertLessEqual(self.bubble.geometry().right(), available.right())
        self.assertGreaterEqual(self.bubble.geometry().top(), available.top())
        self.assertLessEqual(self.bubble.geometry().bottom(), available.bottom())

    def test_bubble_sits_above_the_avatar_and_points_to_its_head(self):
        available = QRect(0, 0, 1000, 700)
        avatar = QRect(400, 300, 240, 135)

        self.bubble.follow_avatar(avatar, available)

        self.assertLess(self.bubble.geometry().top(), avatar.top())
        self.assertGreaterEqual(self.bubble.geometry().bottom(), avatar.top())
        tail_global_x = self.bubble.geometry().left() + self.bubble.tail_center_x
        self.assertLessEqual(abs(tail_global_x - avatar.center().x()), 2)

    def test_tail_shifts_toward_avatar_when_bubble_is_near_an_edge(self):
        available = QRect(0, 0, 1000, 700)
        self.bubble.follow_avatar(QRect(0, 300, 240, 135), available)

        self.assertGreaterEqual(self.bubble.geometry().left(), available.left())
        self.assertLess(self.bubble.tail_center_x, self.bubble.width() / 2)

    def test_bubble_offsets_toward_screen_center_from_each_side(self):
        available = QRect(0, 0, 1000, 700)
        left_avatar = QRect(100, 300, 240, 135)
        right_avatar = QRect(660, 300, 240, 135)

        self.bubble.follow_avatar(left_avatar, available)
        left_bubble_center = self.bubble.geometry().center().x()
        self.bubble.follow_avatar(right_avatar, available)
        right_bubble_center = self.bubble.geometry().center().x()

        self.assertGreater(left_bubble_center, left_avatar.center().x())
        self.assertLess(right_bubble_center, right_avatar.center().x())


if __name__ == '__main__':
    unittest.main()
