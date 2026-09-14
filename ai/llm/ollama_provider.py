"""Ollama adapter for generating text with a local model."""

import json
import shutil
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen


class OllamaProvider:
    """Use an installed Ollama model without an API key."""

    def __init__(
        self,
        model: str,
        base_url: str = "http://127.0.0.1:11434",
        timeout: float = 120.0,
    ) -> None:
        self.model = model
        self.base_url = base_url.rstrip("/")
        self.timeout = timeout

    @staticmethod
    def _request(
        base_url: str, path: str, payload: dict | None = None, timeout: float = 10.0
    ) -> dict:
        """Call the local API and report actionable connection errors."""
        request = Request(
            f"{base_url.rstrip('/')}{path}",
            data=json.dumps(payload).encode("utf-8") if payload is not None else None,
            headers={"Content-Type": "application/json"},
        )
        try:
            with urlopen(request, timeout=timeout) as response:
                result = json.load(response)
        except HTTPError as exc:
            raise RuntimeError(
                f"Ollama returned HTTP {exc.code}. Check the service and ensure "
                "the selected model is installed and supports text generation."
            ) from exc
        except URLError as exc:
            if isinstance(exc.reason, TimeoutError):
                raise RuntimeError(f"Ollama timed out after {timeout} seconds.") from exc
            if shutil.which("ollama") is None:
                raise RuntimeError(
                    f"Cannot reach Ollama at {base_url}, and the 'ollama' command "
                    "was not found. Ollama may not be installed or may not be on PATH. "
                    "Install it from https://ollama.com/download, or check the "
                    "installation and restart your terminal."
                ) from exc
            raise RuntimeError(
                f"Ollama is installed, but its server is unreachable at {base_url}. "
                "Start the Ollama app or run 'ollama serve' in another terminal."
            ) from exc
        except TimeoutError as exc:
            raise RuntimeError(f"Ollama timed out after {timeout} seconds.") from exc
        except (ValueError, UnicodeError) as exc:
            raise RuntimeError("Ollama returned invalid JSON.") from exc

        if not isinstance(result, dict):
            raise RuntimeError("Ollama returned an unexpected response format.")
        return result

    @classmethod
    def list_models(cls, base_url: str = "http://127.0.0.1:11434") -> list[str]:
        """Fetch model names from the running local Ollama installation."""
        result = cls._request(base_url, "/api/tags")
        models = result.get("models")
        if not isinstance(models, list) or any(
            not isinstance(model, dict)
            or not isinstance(model.get("name"), str)
            or not model["name"].strip()
            for model in models
        ):
            raise RuntimeError("Ollama returned an invalid model list.")
        return sorted({model["name"] for model in models})

    def generate(
        self, instructions: str, user_input: str,
        history: list[dict[str, str]] | None = None,
    ) -> str:
        """Send optional conversation history followed by the current input."""
        payload = {
            "model": self.model,
            "messages": [
                {"role": "system", "content": instructions},
                *(history or []),
                {"role": "user", "content": user_input},
            ],
            "stream": False,
        }
        result = self._request(
            self.base_url, "/api/chat", payload, timeout=self.timeout
        )
        message = result.get("message")
        text = message.get("content") if isinstance(message, dict) else None
        if not isinstance(text, str) or not text.strip():
            raise RuntimeError("Ollama returned no response text.")
        return text

    def close(self) -> None:
        """Each HTTP request already closes its own connection."""
