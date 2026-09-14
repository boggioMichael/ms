"""Select an LLM, check its connection, and chat through the Agent."""

import os
import random
from pathlib import Path
from time import perf_counter

from dotenv import load_dotenv

from agent import Agent
from llm.ollama_provider import OllamaProvider
from llm.openai_provider import OpenAIProvider


GREETINGS = (
    "Hi, I'm MapleSyrup! What would you like help with today?",
    "Hey there! I'm MapleSyrup. Tell me what's happening in your game.",
    "Hello, adventurer! I'm MapleSyrup. What are you working on today?",
    "Hi! MapleSyrup here. Tell me your goal, and let's think through your next step.",
)


def select_option(title: str, options: list[str]) -> str | None:
    """Read a numbered choice, with zero reserved for exiting."""
    print(f"\n{title}")
    for number, option in enumerate(options, start=1):
        print(f"{number}. {option}")
    print("0. Exit")

    while True:
        choice = input("Choice: ").strip()
        if choice == "0":
            return None
        try:
            index = int(choice) - 1
        except ValueError:
            index = -1
        if 0 <= index < len(options):
            return options[index]
        print(f"Enter a number from 0 to {len(options)}.")


def wants_to_chat() -> bool:
    while True:
        answer = input("\nWould you like to chat with the model? (y/n): ").strip().lower()
        if answer in ("y", "n"):
            return answer == "y"
        print("Enter y or n.")


def chat(agent: Agent) -> None:
    print("\nChatting through the Agent.")
    print("\nType /exit to finish. The last 5 completed turns are sent with each message.")
    print("Chat memory is kept in RAM only and is cleared when this session ends.")
    print(f"Agent: {random.choice(GREETINGS)}")
    while True:
        message = input("You: ").strip()
        if message.lower() == "/exit":
            return
        if not message:
            continue
        print("Waiting for the model...", flush=True)
        try:
            response = agent.respond(message)
        except RuntimeError as exc:
            print(f"LLM request failed: {exc}")
            continue
        print(f"Agent: {response}")


def run_menu() -> None:
    print("MapleSyrup LLM test console")
    while True:
        name = select_option("Select provider:", ["Ollama", "OpenAI"])
        if name is None:
            return
        provider_type = OllamaProvider if name == "Ollama" else OpenAIProvider

        if name == "OpenAI" and not os.getenv("OPENAI_API_KEY", "").strip():
            print("OpenAI API key check: FAIL")
            print("Set OPENAI_API_KEY in ai/.env, or choose Ollama for local testing.")
            continue

        print(f"Checking LLM server ({name}) and fetching models...", flush=True)
        try:
            models = provider_type.list_models()
        except RuntimeError as exc:
            print(f"LLM server check ({name}): FAIL")
            print(exc)
            continue
        print(f"LLM server check ({name}): PASS")

        if not models:
            print("Model availability check: FAIL - no models were listed.")
            if name == "Ollama":
                print("Download a model with 'ollama pull <model-name>', then try again.")
            else:
                print("Check model access with your OpenAI project administrator.")
            continue

        if name == "OpenAI":
            print("OpenAI may list non-text models. The response check will test text support.")
        model = select_option("Select model:", models)
        if model is None:
            return
        print(f"\nProvider: {name}\nModel: {model}")
        print("Model availability check: PASS (listed by provider)")

        try:
            provider = provider_type(model=model)
        except RuntimeError as exc:
            print(f"LLM client setup: FAIL - {exc}")
            continue

        try:
            print("Running model response check...", flush=True)
            started = perf_counter()
            try:
                response = provider.generate(
                    instructions="Reply in English with a short answer.",
                    user_input="Reply with exactly: Ready.",
                )
            except RuntimeError as exc:
                print(f"Model response check: FAIL - {exc}")
                continue
            elapsed = perf_counter() - started
            print("Model response check: PASS")
            print(f"Test response: {response}")
            print(f"Response time: {elapsed:.2f} seconds")
            if wants_to_chat():
                chat(Agent(provider))
            return
        finally:
            provider.close()


def main() -> None:
    env_path = Path(__file__).resolve().parent / ".env"
    load_dotenv(env_path)

    try:
        run_menu()
    except (EOFError, KeyboardInterrupt):
        print()
    print("Goodbye.")


if __name__ == "__main__":
    main()
