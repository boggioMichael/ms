"""Speech-to-text providers independent of microphone and desktop UI code."""

from .provider import RecognitionResult, STTProvider
from .local import LocalSTT

__all__ = ('LocalSTT', 'RecognitionResult', 'STTProvider')
