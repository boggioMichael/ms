"""Keep the last five completed conversation turns in RAM."""

from collections import deque


class ConversationMemory:
    """Store session history without writing it to disk."""

    def __init__(self) -> None:
        self.turns: deque[dict[str, str]] = deque(maxlen=5)

    def add_turn(self, user_message: str, agent_response: str) -> None:
        """Append a completed turn, automatically dropping the oldest if full."""
        self.turns.append({"user": user_message, "assistant": agent_response})

    def to_messages(self) -> list[dict[str, str]]:
        """Return a fresh chronological message list for the provider."""
        return [
            {"role": role, "content": turn[role]}
            for turn in self.turns
            for role in ("user", "assistant")
        ]
