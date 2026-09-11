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


class Note:
    """A note whose text borrows from the request body."""

    text: borrowed[str]


@api.post("/notes", stories=["US-004"])
def create_note(request: Note) -> dict:
    """
    Reads a note's text straight out of the request body.
    The DTO holds a `&str`, so the generated crate copies no string.
    """
    return {"echo": request}


@api.get("/orders/{id}", stories=["US-005"])
def get_order(id: int) -> dict:
    """
    Fetches one order by its identifier.
    The `{id}` placeholder binds to the `id` parameter, which is an int.
    """
    return {"id": id, "status": "open"}


@api.get("/search", stories=["US-006"])
def search(page: int, size: int) -> dict:
    """
    Searches a page of results.
    Neither parameter is in the route path, and both are primitives, so both
    read from the query string: `/search?page=2&size=10`.
    """
    return {"page": page, "size": size}


@api.get("/orders/{id}/total", stories=["US-007"])
def order_total(id: int, quantity: int) -> dict:
    """
    Prices an order line.
    Binds a local, branches on a comparison, and returns from each branch.
    """
    total = id * quantity
    if total > 100:
        return {"id": id, "total": total, "tier": "bulk"}
    else:
        return {"id": id, "total": total, "tier": "single"}