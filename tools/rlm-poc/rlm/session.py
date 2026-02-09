"""Session loader — reads hstry exports (markdown or JSON) into structured data."""

import json
import re
from dataclasses import dataclass, field
from pathlib import Path
from typing import Optional


@dataclass
class Message:
    role: str  # user, assistant, tool, system
    content: str
    timestamp: Optional[str] = None


@dataclass
class Session:
    id: str
    title: str
    workspace: Optional[str] = None
    created: Optional[str] = None
    updated: Optional[str] = None
    messages: list[Message] = field(default_factory=list)
    raw_text: str = ""

    @property
    def user_messages(self) -> list[Message]:
        return [m for m in self.messages if m.role == "user"]

    @property
    def assistant_messages(self) -> list[Message]:
        return [m for m in self.messages if m.role == "assistant"]

    @property
    def tool_messages(self) -> list[Message]:
        return [m for m in self.messages if m.role == "tool"]

    @property
    def turn_count(self) -> int:
        return len(self.user_messages)

    @property
    def total_chars(self) -> int:
        return sum(len(m.content) for m in self.messages)

    def summary_stats(self) -> dict:
        return {
            "id": self.id,
            "title": self.title,
            "turns": self.turn_count,
            "messages": len(self.messages),
            "user_msgs": len(self.user_messages),
            "assistant_msgs": len(self.assistant_messages),
            "tool_msgs": len(self.tool_messages),
            "total_chars": self.total_chars,
        }


def load_markdown(text: str) -> Session:
    """Parse hstry markdown export into a Session."""
    lines = text.split("\n")

    # Title from first H1
    title = "Unknown Session"
    for line in lines:
        if line.startswith("# "):
            title = line[2:].strip()
            break

    # Extract metadata from the header block
    created = updated = workspace = None
    for line in lines[:10]:
        if line.startswith("- Created:"):
            created = line.split(":", 1)[1].strip()
        elif line.startswith("- Updated:"):
            updated = line.split(":", 1)[1].strip()
        elif line.startswith("- Workspace:"):
            workspace = line.split(":", 1)[1].strip()

    # Parse messages: each starts with ## role
    messages: list[Message] = []
    current_role = None
    current_timestamp = None
    current_lines: list[str] = []

    role_pattern = re.compile(r"^## (user|assistant|tool|system)\s*$")
    timestamp_pattern = re.compile(r"^_at (.+)_$")

    for line in lines:
        role_match = role_pattern.match(line)
        if role_match:
            # Flush previous message
            if current_role is not None:
                content = "\n".join(current_lines).strip()
                if content:
                    messages.append(Message(
                        role=current_role,
                        content=content,
                        timestamp=current_timestamp,
                    ))
            current_role = role_match.group(1)
            current_timestamp = None
            current_lines = []
            continue

        ts_match = timestamp_pattern.match(line)
        if ts_match and current_role is not None:
            current_timestamp = ts_match.group(1)
            continue

        if current_role is not None:
            current_lines.append(line)

    # Flush last message
    if current_role is not None:
        content = "\n".join(current_lines).strip()
        if content:
            messages.append(Message(
                role=current_role,
                content=content,
                timestamp=current_timestamp,
            ))

    # Generate a stable ID from the title
    session_id = title.lower().replace(" ", "-")[:40]

    return Session(
        id=session_id,
        title=title,
        workspace=workspace,
        created=created,
        updated=updated,
        messages=messages,
        raw_text=text,
    )


def load_json(text: str) -> Session:
    """Parse hstry JSON export into a Session."""
    data = json.loads(text)

    # Handle both direct conversation objects and wrapped {result: ...}
    conv = data.get("result", data) if isinstance(data, dict) else data

    if isinstance(conv, list):
        # List of messages directly
        messages = [
            Message(
                role=m.get("role", "unknown"),
                content=m.get("content", ""),
                timestamp=m.get("timestamp"),
            )
            for m in conv
        ]
        return Session(
            id="json-import",
            title="Imported Session",
            messages=messages,
            raw_text=text,
        )

    messages = [
        Message(
            role=m.get("role", "unknown"),
            content=m.get("content", ""),
            timestamp=m.get("timestamp"),
        )
        for m in conv.get("messages", [])
    ]

    return Session(
        id=conv.get("id", "json-import"),
        title=conv.get("title", "Imported Session"),
        workspace=conv.get("workspace"),
        created=conv.get("created"),
        updated=conv.get("updated"),
        messages=messages,
        raw_text=text,
    )


def load_session(source: str | Path) -> Session:
    """Load a session from a file path, stdin marker '-', or raw text.

    Auto-detects markdown vs JSON format.
    """
    if source == "-":
        import sys
        text = sys.stdin.read()
    elif isinstance(source, Path) or (isinstance(source, str) and (
        source.endswith(".md") or source.endswith(".json") or
        source.startswith("/") or source.startswith(".")
    )):
        text = Path(source).read_text()
    else:
        text = source

    text = text.strip()
    if text.startswith("{") or text.startswith("["):
        return load_json(text)
    return load_markdown(text)
