# 5. Story-to-Code Traceability
Every route is tagged with a story ID:

```python
@api.post("/orders", stories=["US-123", "EPIC-456"])
def create_order(request: OrderCreate) -> OrderResponse:
    ...
```

The Gauntlet enforces that every public endpoint has at least one story link. The audit checks that the implementation satisfies all linked stories.