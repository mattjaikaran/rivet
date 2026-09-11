"""
Basic Rivet Application Example.
This is the smallest possible app to test the transpiler.
"""
from typing import List

from rivet import api

@api.get("/ping", stories=["US-001"])
def ping() -> dict:
    """
    A simple health check endpoint.
    Returns: {"status": "pong"}
    """
    return {"status": "pong"}

@api.post("/echo", stories=["US-002"])
def echo(request: dict) -> dict:
    """
    Echoes back the request body.
    Demonstrates request/response DTO handling.
    """
    return {"echo": request}

class Embedding:
    """A fixed-size vector, rendered as a Rust array under const generics."""

    values: List[float, 768]


@api.post("/embed", stories=["US-003"])
def embed(request: Embedding) -> Embedding:
    """Echo an embedding back, with no copy of the vector."""
    return request