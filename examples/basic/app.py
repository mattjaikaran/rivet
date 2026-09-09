"""
Basic Rivet Application Example.
This is the smallest possible app to test the transpiler.
"""
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