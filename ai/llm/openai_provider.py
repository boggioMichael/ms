"""OpenAI adapter for generating text responses."""

from openai import (
    APIConnectionError,
    APIStatusError,
    APITimeoutError,
    OpenAI,
    OpenAIError,
)


class OpenAIProvider:
    """Use a selected model with credentials from OPENAI_API_KEY."""

    def __init__(self, model: str) -> None:
        self.model = model
        try:
            self.client = OpenAI(timeout=120.0, max_retries=0)
        except OpenAIError as exc:
            raise RuntimeError(self._error_message(exc)) from exc

    @staticmethod
    def _error_message(error: OpenAIError) -> str:
        """Describe failures without printing credentials or response bodies."""
        if isinstance(error, APITimeoutError):
            return "OpenAI timed out. Try again or choose another model."
        if isinstance(error, APIConnectionError):
            return "Cannot reach OpenAI. Check your internet connection."
        if isinstance(error, APIStatusError):
            messages = {
                400: "OpenAI rejected the request. Choose a model that supports text in the Responses API.",
                401: "OpenAI rejected the API key. Check OPENAI_API_KEY in ai/.env.",
                403: "OpenAI denied access. Check the key's project and model permissions.",
                404: "The selected OpenAI model or endpoint is unavailable. Choose another model.",
                429: "OpenAI's rate or quota limit was reached. Check usage and billing, or retry later.",
            }
            return messages.get(
                error.status_code,
                f"OpenAI returned HTTP {error.status_code}. Try again later.",
            )
        return "OpenAI could not complete the request. Check your API configuration."

    @classmethod
    def list_models(cls) -> list[str]:
        """Fetch the models listed for the configured API credentials."""
        try:
            with OpenAI(timeout=10.0, max_retries=0) as client:
                return sorted({model.id for model in client.models.list()})
        except OpenAIError as exc:
            raise RuntimeError(cls._error_message(exc)) from exc

    def generate(
        self, instructions: str, user_input: str,
        history: list[dict[str, str]] | None = None,
    ) -> str:
        """Send optional conversation history followed by the current input."""
        messages = [*(history or []), {"role": "user", "content": user_input}]
        try:
            response = self.client.responses.create(
                model=self.model,
                instructions=instructions,
                input=messages,
            )
        except OpenAIError as exc:
            raise RuntimeError(self._error_message(exc)) from exc
        text = response.output_text
        if not text.strip():
            raise RuntimeError("OpenAI returned no response text. Try another model.")
        return text

    def close(self) -> None:
        """Release the SDK's HTTP connections."""
        self.client.close()

    def stream(self, instructions: str, user_input: str, history=None):
        """Yield only answer text deltas and require successful stream completion."""
        completed = False
        try:
            with self.client.responses.create(
                model=self.model, instructions=instructions,
                input=[*(history or []), {'role': 'user', 'content': user_input}],
                stream=True,
            ) as events:
                for event in events:
                    if event.type == 'response.output_text.delta':
                        yield event.delta
                    elif event.type == 'response.completed':
                        completed = True
                    elif event.type in ('response.failed', 'response.incomplete', 'error'):
                        raise RuntimeError('OpenAI could not finish the response. Please retry.')
            if not completed:
                raise RuntimeError('OpenAI stream ended before the response was complete.')
        except OpenAIError as exc:
            raise RuntimeError(self._error_message(exc)) from exc
