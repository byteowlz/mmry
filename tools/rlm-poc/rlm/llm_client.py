"""OpenAI-compatible LLM client for local or cloud models."""

import os
from typing import Optional

import httpx
from openai import OpenAI


class LLMClient:
    """Thin wrapper around OpenAI-compatible chat completion API.

    Works with llama.cpp server, vLLM, Ollama (with /v1 compat), or OpenAI itself.
    """

    def __init__(
        self,
        base_url: Optional[str] = None,
        api_key: Optional[str] = None,
        model: Optional[str] = None,
        timeout: float = 300.0,
    ):
        self.base_url = base_url or os.getenv("RLM_API_URL", "https://api.openai.com/v1")
        self.api_key = api_key or os.getenv("OPENAI_API_KEY", "no-key-needed")
        self.model = model or os.getenv("RLM_MODEL", "gpt-4o-mini")
        self.timeout = timeout

        # Track token usage
        self.total_prompt_tokens = 0
        self.total_completion_tokens = 0
        self.total_calls = 0

        self.client = OpenAI(
            base_url=self.base_url,
            api_key=self.api_key,
            timeout=httpx.Timeout(self.timeout),
        )

    def completion(
        self,
        messages: list[dict[str, str]] | str,
        max_tokens: Optional[int] = None,
        temperature: float = 0.7,
    ) -> str:
        """Send a chat completion request and return the text content."""
        if isinstance(messages, str):
            messages = [{"role": "user", "content": messages}]

        kwargs: dict = {
            "model": self.model,
            "messages": messages,
            "temperature": temperature,
        }
        if max_tokens is not None:
            kwargs["max_tokens"] = max_tokens

        response = self.client.chat.completions.create(**kwargs)
        self.total_calls += 1

        usage = response.usage
        if usage:
            self.total_prompt_tokens += usage.prompt_tokens or 0
            self.total_completion_tokens += usage.completion_tokens or 0

        return response.choices[0].message.content or ""

    def usage_summary(self) -> dict:
        return {
            "total_calls": self.total_calls,
            "prompt_tokens": self.total_prompt_tokens,
            "completion_tokens": self.total_completion_tokens,
            "total_tokens": self.total_prompt_tokens + self.total_completion_tokens,
        }
