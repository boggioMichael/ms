"""Local Kokoro synthesis, loaded lazily and kept in memory for repeated use."""

from array import array
import math
import os
from pathlib import Path
import sys

from .tts import AudioClip


class LocalTTS:
    def __init__(self, model_dir: Path | None = None):
        self.model_dir = model_dir or Path(__file__).resolve().parent / '.models' / 'kokoro-multi-lang-v1_0'
        self.engine = None

    def synthesize(self, text: str) -> AudioClip:
        if not text.strip():
            raise ValueError('Speech text must not be empty.')
        if self.engine is None:
            root = self.model_dir
            for name in ('model.onnx', 'voices.bin', 'tokens.txt', 'lexicon-us-en.txt', 'espeak-ng-data'):
                if not (root / name).exists():
                    raise RuntimeError('Kokoro model files are missing. See the local voice setup in ui/README.md.')
            import sherpa_onnx
            config = sherpa_onnx.OfflineTtsConfig(model=sherpa_onnx.OfflineTtsModelConfig(
                kokoro=sherpa_onnx.OfflineTtsKokoroModelConfig(
                    model=str(root / 'model.onnx'), voices=str(root / 'voices.bin'),
                    tokens=str(root / 'tokens.txt'), lexicon=str(root / 'lexicon-us-en.txt'),
                    data_dir=str(root / 'espeak-ng-data'), lang='en-us'),
                num_threads=min(4, os.cpu_count() or 2), provider='cpu', debug=False))
            if not config.validate():
                raise RuntimeError('The local TTS configuration is invalid.')
            self.engine = sherpa_onnx.OfflineTts(config)
        audio = self.engine.generate(text, sid=3, speed=1.0)
        samples = audio.samples
        if not samples or not all(math.isfinite(value) for value in samples):
            raise RuntimeError('The TTS model returned invalid audio.')
        pcm = array('h', (round(max(-1.0, min(1.0, value)) * 32767) for value in samples))
        if sys.byteorder != 'little':
            pcm.byteswap()
        return AudioClip(pcm.tobytes(), audio.sample_rate)
