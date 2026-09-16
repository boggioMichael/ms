"""Shared audio contract for local and future cloud TTS providers."""

from dataclasses import dataclass
from typing import Protocol


@dataclass(frozen=True)
class AudioClip:
    """Mono, signed 16-bit little-endian PCM audio."""

    pcm: bytes
    sample_rate: int

    def __post_init__(self):
        if not self.pcm or len(self.pcm) % 2 or self.sample_rate <= 0:
            raise ValueError('Audio must contain complete mono PCM16 samples at a positive rate.')


class TTSProvider(Protocol):
    def synthesize(self, text: str) -> AudioClip:
        """Generate audio; callers run this potentially slow operation off the UI thread."""
        ...
