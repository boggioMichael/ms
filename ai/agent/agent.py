"""Coordinate player requests through the selected LLM provider."""

from .memory import ConversationMemory
from .prompts import SYSTEM_PROMPT


class Agent:
    """Respond using the last five completed turns of this session."""

    def __init__(self, provider) -> None:
        """Accept a provider supporting instructions, user input, and history."""
        self.provider = provider
        self.memory = ConversationMemory()

    def respond(self, player_message: str) -> str:
        """Send recent history and save the turn only after a successful reply."""
        player_message = player_message.strip()
        if not player_message:
            raise ValueError("Player message must not be empty.")

        response = self.provider.generate(
            instructions=SYSTEM_PROMPT,
            user_input=player_message,
            history=self.memory.to_messages(),
        )
        self.memory.add_turn(player_message, response)
        return response

    def respond_stream(self, player_message: str, cancelled=lambda: False):
        """Yield answer deltas; only a fully consumed, successful turn enters memory."""
        player_message = player_message.strip()
        if not player_message:
            raise ValueError('Player message must not be empty.')
        if cancelled():
            return
        kwargs = dict(instructions=SYSTEM_PROMPT, user_input=player_message,
                      history=self.memory.to_messages())
        stream = getattr(self.provider, 'stream', None)
        chunks = iter(stream(**kwargs)) if stream else iter([self.provider.generate(**kwargs)])
        parts = []
        try:
            for text in chunks:
                if cancelled():
                    return
                if not isinstance(text, str):
                    raise RuntimeError('Provider returned invalid response text.')
                parts.append(text)
                if text:
                    yield text
            if cancelled():
                return
            response = ''.join(parts)
            if not response.strip():
                raise RuntimeError('Provider returned no response text.')
            self.memory.add_turn(player_message, response)
        finally:
            close = getattr(chunks, 'close', None)
            if close:
                close()
