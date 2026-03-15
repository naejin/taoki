"""Sample Python module for benchmarking Taoki's index_source."""

from __future__ import annotations

import os
import sys
import json
import logging
from typing import Any, Dict, List, Optional, Protocol, TypeVar
from dataclasses import dataclass, field
from pathlib import Path
from flask import Flask, Blueprint, request, jsonify

logger = logging.getLogger(__name__)

T = TypeVar("T")

MAX_ITEMS = 1000
DEFAULT_PAGE_SIZE = 20

app = Flask(__name__)
api_bp = Blueprint("api", __name__, url_prefix="/api/v1")


class ServiceError(Exception):
    """Base exception for service errors."""

    def __init__(self, message: str, code: int = 500) -> None:
        super().__init__(message)
        self.code = code


class NotFoundError(ServiceError):
    """Raised when a resource is not found."""

    def __init__(self, resource: str) -> None:
        super().__init__(f"{resource} not found", code=404)


class Indexable(Protocol):
    """Protocol for indexable items."""

    @property
    def id(self) -> str: ...

    @property
    def kind(self) -> str: ...

    def score(self) -> float: ...


@dataclass
class ClientConfig:
    """Configuration for the HTTP client."""

    base_url: str
    timeout_ms: int = 5000
    max_retries: int = 3
    headers: Dict[str, str] = field(default_factory=dict)
    user_agent: str = "taoki/0.1"
    follow_redirects: bool = True
    verify_ssl: bool = True
    proxy: Optional[str] = None


@dataclass
class User:
    id: str
    name: str
    email: str
    role: str = "viewer"

    def to_dict(self) -> Dict[str, Any]:
        return {"id": self.id, "name": self.name, "email": self.email, "role": self.role}

    @classmethod
    def from_dict(cls, data: Dict[str, Any]) -> "User":
        return cls(
            id=data["id"],
            name=data["name"],
            email=data["email"],
            role=data.get("role", "viewer"),
        )


def fetch_user(user_id: str) -> User:
    """Fetch a user by ID."""
    if not user_id:
        raise ServiceError("user_id cannot be empty", code=400)
    return User(id=user_id, name="Alice", email="alice@example.com")


def paginate(items: List[T], page: int = 1, per_page: int = DEFAULT_PAGE_SIZE) -> Dict[str, Any]:
    """Return a paginated slice of items."""
    total = len(items)
    start = (page - 1) * per_page
    end = min(start + per_page, total)
    return {
        "items": items[start:end],
        "total": total,
        "page": page,
        "per_page": per_page,
    }


def parse_header(raw: str) -> Optional[tuple[str, str]]:
    if ":" not in raw:
        return None
    key, _, value = raw.partition(":")
    return key.strip(), value.strip()


def _internal_hash(data: bytes) -> int:
    h = 0xCBF29CE484222325
    for b in data:
        h ^= b
        h = (h * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return h


@api_bp.route("/users/<user_id>", methods=["GET"])
def get_user(user_id: str):
    try:
        user = fetch_user(user_id)
        return jsonify(user.to_dict())
    except NotFoundError as e:
        return jsonify({"error": str(e)}), 404


@api_bp.route("/users", methods=["POST"])
def create_user():
    data = request.get_json()
    user = User.from_dict(data)
    return jsonify(user.to_dict()), 201


app.register_blueprint(api_bp)


def test_fetch_user():
    user = fetch_user("u1")
    assert user.id == "u1"


def test_paginate():
    items = list(range(10))
    result = paginate(items, page=2, per_page=3)
    assert result["items"] == [3, 4, 5]


if __name__ == "__main__":
    app.run(host="0.0.0.0", port=int(os.environ.get("PORT", 8080)))
