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
