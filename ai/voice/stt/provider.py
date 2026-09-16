"""Shared contract for local and future speech-to-text providers."""

from dataclasses import dataclass
from typing import Protocol


@dataclass(frozen=True)
class RecognitionResult:
    """A final, non-empty player utterance recognized from microphone audio."""

    text: str

    def __post_init__(self):
        text = self.text.strip()
        if not text:
            raise ValueError('Recognition text must not be empty.')
        object.__setattr__(self, 'text', text)


class STTProvider(Protocol):
    """Accept audio incrementally and emit text only at an utterance endpoint."""

    def accept_audio(self, samples: list[float], sample_rate: int) -> RecognitionResult | None:
        """Return a final result when an utterance completes, otherwise None."""
        ...

    def reset(self) -> None:
        """Discard partial speech and prepare to recognize a new utterance."""
        ...
