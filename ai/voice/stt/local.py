"""Local streaming English recognition with a Sherpa-ONNX Zipformer model."""

import os
from pathlib import Path

from .provider import RecognitionResult


MODEL_NAME = 'sherpa-onnx-streaming-zipformer-en-2023-06-26'
MODEL_FILES = (
    'encoder-epoch-99-avg-1-chunk-16-left-128.int8.onnx',
    'decoder-epoch-99-avg-1-chunk-16-left-128.onnx',
    'joiner-epoch-99-avg-1-chunk-16-left-128.int8.onnx',
    'tokens.txt',
)
SAMPLE_RATE = 16000


class LocalSTT:
    """Accept 16 kHz audio incrementally and return text at utterance endpoints."""

    def __init__(self, model_dir: Path | None = None, recognizer_factory=None):
        self.model_dir = model_dir or Path(__file__).resolve().parents[1] / '.models' / 'stt' / MODEL_NAME
        self.recognizer_factory = recognizer_factory
        self.recognizer = None
        self.stream = None

    def accept_audio(self, samples: list[float], sample_rate: int) -> RecognitionResult | None:
        """Decode a non-empty 16 kHz normalized sample frame."""
        if not samples:
            raise ValueError('Audio samples must not be empty.')
        if sample_rate != SAMPLE_RATE:
            raise RuntimeError('Local speech recognition requires 16 kHz audio.')
        recognizer, stream = self._stream()
        stream.accept_waveform(sample_rate, samples)
        while recognizer.is_ready(stream):
            recognizer.decode_stream(stream)
        if not recognizer.is_endpoint(stream):
            return None
        text = recognizer.get_result_all(stream).text.strip()
        recognizer.reset(stream)
        if not text:
            return None
        return RecognitionResult(text)

    def reset(self) -> None:
        """Discard partial speech while retaining the loaded model."""
        if self.recognizer is not None and self.stream is not None:
            self.recognizer.reset(self.stream)

    def _stream(self):
        if self.recognizer is None:
            self.recognizer = self._create_recognizer()
            self.stream = self.recognizer.create_stream()
        return self.recognizer, self.stream

    def _create_recognizer(self):
        if self.recognizer_factory is not None:
            return self.recognizer_factory(self.model_dir)
        missing = [name for name in MODEL_FILES if not (self.model_dir / name).is_file()]
        if missing:
            raise RuntimeError(
                'Local speech recognition model files are missing. See the local speech recognition setup in ui/README.md.'
            )
        try:
            import sherpa_onnx
        except ImportError as exc:
            raise RuntimeError('Speech recognition dependency is missing. Install ui/requirements.txt and restart.') from exc
        return sherpa_onnx.OnlineRecognizer.from_transducer(
            tokens=str(self.model_dir / MODEL_FILES[3]),
            encoder=str(self.model_dir / MODEL_FILES[0]),
            decoder=str(self.model_dir / MODEL_FILES[1]),
            joiner=str(self.model_dir / MODEL_FILES[2]),
            num_threads=min(4, os.cpu_count() or 2),
            decoding_method='greedy_search',
            enable_endpoint_detection=True,
            rule1_min_trailing_silence=2.4,
            rule2_min_trailing_silence=1.2,
            rule3_min_utterance_length=20.0,
            provider='cpu',
        )
