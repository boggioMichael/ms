"""Offline discovery and cache checks for avatar animation assets."""

import json
from pathlib import Path
import sys
import tempfile
import unittest

from PySide6.QtCore import QSize
from PySide6.QtGui import QColor, QImage

sys.path.insert(0, str(Path(__file__).resolve().parent))

from avatar_assets import AnimationCatalog, FrameCache


def write_frame(path: Path, color: QColor = QColor('#D99A43')) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    image = QImage(40, 20, QImage.Format.Format_ARGB32_Premultiplied)
    image.fill(color)
    assert image.save(str(path))


class AnimationCatalogTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.root = Path(self.directory.name)

    def tearDown(self):
        self.directory.cleanup()

    def test_catalog_sorts_numeric_frames_and_uses_default_fps(self):
        write_frame(self.root / 'idle' / 'idle_01' / 'frame_0010.png')
        write_frame(self.root / 'idle' / 'idle_01' / 'frame_0002.png')
        write_frame(self.root / 'idle' / 'idle_01' / 'thumbnail.png')

        sequence, = AnimationCatalog(self.root).sequences('idle')

        self.assertEqual(sequence.name, 'idle_01')
        self.assertEqual([path.name for path in sequence.frames], ['frame_0002.png', 'frame_0010.png'])
        self.assertEqual(sequence.fps, 15.0)

    def test_catalog_supports_direct_frames_and_sequence_fps(self):
        write_frame(self.root / 'listening' / 'frame_0001.png')
        (self.root / 'listening' / 'animation.json').write_text(json.dumps({'fps': 12}))

        sequence, = AnimationCatalog(self.root).sequences('listening')

        self.assertEqual(sequence.name, 'default')
        self.assertEqual(sequence.fps, 12.0)

    def test_invalid_fps_uses_default_and_missing_state_falls_back_to_idle(self):
        write_frame(self.root / 'idle' / 'idle_01' / 'frame_0001.png')
        (self.root / 'idle' / 'idle_01' / 'animation.json').write_text('{"fps": 0}')
        catalog = AnimationCatalog(self.root)

        sequence, = catalog.sequences('idle')

        self.assertEqual(sequence.fps, 15.0)
        self.assertEqual(catalog.fallback('speaking'), sequence)


class FrameCacheTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.root = Path(self.directory.name)
        for index in range(3):
            write_frame(self.root / 'idle' / 'idle_01' / f'frame_{index + 1:04}.png')
        self.sequence, = AnimationCatalog(self.root).sequences('idle')

    def tearDown(self):
        self.directory.cleanup()

    def test_cache_scales_frames_and_evicts_old_entries_to_stay_within_budget(self):
        cache = FrameCache(QSize(20, 10), max_bytes=800)

        for index in range(3):
            self.assertEqual(cache.frame(self.sequence, index).size(), QSize(20, 10))

        self.assertLessEqual(cache.bytes_used, 800)
        self.assertLessEqual(cache.entry_count, 1)


if __name__ == '__main__':
    unittest.main()
