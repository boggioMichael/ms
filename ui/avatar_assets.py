"""Animation discovery and bounded scaled-frame caching for the avatar overlay."""

from collections import OrderedDict
from dataclasses import dataclass
import json
from pathlib import Path
import re

from PySide6.QtCore import QSize, Qt
from PySide6.QtGui import QImage


DEFAULT_FPS = 15.0
FRAME_NAME = re.compile(r'^frame_(\d+)\.png$', re.IGNORECASE)


@dataclass(frozen=True)
class AnimationSequence:
    """One ordered frame sequence for one avatar state."""

    name: str
    state: str
    directory: Path
    frames: tuple[Path, ...]
    fps: float


class AnimationCatalog:
    """Discover direct and nested state sequences without hard-coded assets."""

    def __init__(self, root: Path):
        self.root = Path(root)
        self._by_state = {}

    def sequences(self, state: str) -> tuple[AnimationSequence, ...]:
        state = state.lower()
        if state not in self._by_state:
            self._by_state[state] = self._discover(state)
        return self._by_state[state]

    def fallback(self, state: str) -> AnimationSequence | None:
        choices = self.sequences(state)
        if choices:
            return choices[0]
        idle = self.sequences('idle')
        return idle[0] if idle else None

    def _discover(self, state: str) -> tuple[AnimationSequence, ...]:
        state_directory = self.root / state
        if not state_directory.is_dir():
            return ()
        found = []
        direct = self._sequence('default', state, state_directory)
        if direct is not None:
            found.append(direct)
        for directory in sorted((path for path in state_directory.iterdir() if path.is_dir()),
                                key=lambda path: path.name.lower()):
            sequence = self._sequence(directory.name, state, directory)
            if sequence is not None:
                found.append(sequence)
        return tuple(found)

    def _sequence(self, name: str, state: str, directory: Path) -> AnimationSequence | None:
        numbered = []
        for path in directory.iterdir():
            match = FRAME_NAME.match(path.name)
            if path.is_file() and match:
                numbered.append((int(match.group(1)), path))
        if not numbered:
            return None
        frames = tuple(path for _, path in sorted(numbered, key=lambda item: item[0]))
        return AnimationSequence(name, state, directory, frames, self._fps(directory))

    @staticmethod
    def _fps(directory: Path) -> float:
        try:
            value = json.loads((directory / 'animation.json').read_text()).get('fps')
            value = float(value)
            if value > 0:
                return value
        except (OSError, ValueError, TypeError, json.JSONDecodeError):
            pass
        return DEFAULT_FPS


class FrameCache:
    """Keep scaled QImages in a byte-bounded least-recently-used cache."""

    def __init__(self, canvas_size: QSize, max_bytes: int = 64 * 1024 * 1024):
        self.canvas_size = QSize(canvas_size)
        self.max_bytes = max(0, max_bytes)
        self._entries = OrderedDict()
        self.bytes_used = 0

    @property
    def entry_count(self) -> int:
        return len(self._entries)

    def frame(self, sequence: AnimationSequence, index: int) -> QImage | None:
        if not sequence.frames:
            return None
        index %= len(sequence.frames)
        key = (sequence.directory.resolve(), index)
        if key in self._entries:
            image, size = self._entries.pop(key)
            self._entries[key] = (image, size)
            return image
        image = QImage(str(sequence.frames[index]))
        if image.isNull():
            return None
        image = image.scaled(
            self.canvas_size,
            Qt.AspectRatioMode.KeepAspectRatio,
            Qt.TransformationMode.SmoothTransformation,
        ).convertToFormat(QImage.Format.Format_ARGB32_Premultiplied)
        size = image.sizeInBytes()
        self._entries[key] = (image, size)
        self.bytes_used += size
        self._evict()
        return image

    def _evict(self) -> None:
        while self._entries and self.bytes_used > self.max_bytes:
            _, (_, size) = self._entries.popitem(last=False)
            self.bytes_used -= size
