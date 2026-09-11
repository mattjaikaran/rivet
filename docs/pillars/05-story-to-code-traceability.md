# 5. Story-to-Code Traceability

Every route is tagged with a story ID:

```python
@api.post("/orders", stories=["US-123", "EPIC-456"])
def create_order(request: OrderCreate) -> OrderResponse:
    ...
```

The Gauntlet enforces that every public endpoint has at least one story link
(`E2045`). `rivet sync` closes the loop outward: it reconciles those story
IDs with the issues in Jira or Linear.

## What the Gauntlet checks

A route whose decorator carries no `stories=[...]` fails the build. The
project opts out with `stories_required = false` in `[gauntlet]`.

The check is local: it proves the code names a story. It cannot prove the
story exists in the tracker, or that the tracker still agrees with the code.
That is what `rivet sync` adds.

## What `rivet sync` checks

The command reads the stories the blueprint declares and the issues the
tracker holds, then reports four disagreements:

| Difference | Meaning |
| :--- | :--- |
| **missing issue** | The blueprint declares a story the tracker has no issue for. |
| **orphan issue** | The tracker holds an issue for a story the blueprint no longer declares. |
| **title drift** | The issue title no longer matches the routes the story covers. |
| **state drift** | The tracker closed the issue, but the blueprint still serves the story. |

An issue binds to a story through its title: the title starts with the story
ID, then a colon, then the routes the story covers —
`US-002: POST /echo; POST /orders`. `--apply` writes that title, so the
issue it creates participates in the next run. A tracker full of ordinary
issues contributes no orphans: only an ID-shaped prefix before the colon
counts.

## Running it

```bash
rivet sync app.py --dry-run     # report the diff, write nothing
rivet sync app.py --apply       # create the missing issues
rivet sync app.py --from captured.json --dry-run
```

`--dry-run` is the default behavior: the command writes nothing unless
`--apply` is passed, and `--apply` only creates the missing issues. It never
edits, renames, or closes an issue a human owns.

`--from FILE` reads a captured tracker payload instead of calling the
tracker. With no credentials configured, the payload's own shape chooses the
parser (a top-level `issues` array is Jira, a top-level `data.issues.nodes`
array is Linear), so an offline run needs no secrets.

The command exits 0 when the blueprint and the tracker agree, and 1 with
`E3022` when a difference survives the run, so a pipeline can gate on it.

## Configuration

Credentials come from the environment only, never from the repository.
Configure exactly one provider:

| Provider | Variables |
| :--- | :--- |
| Jira | `RIVET_JIRA_BASE_URL`, `RIVET_JIRA_TOKEN`, `RIVET_JIRA_PROJECT` |
| Linear | `RIVET_LINEAR_TOKEN`, `RIVET_LINEAR_TEAM` |

Neither, or both, is `E3019`. A failed tracker call is `E3020`, and a failed
creation is `E3021`.

## The wire format

Each provider is a small module with pure request builders and pure
parsers:

- **Jira** — `GET /rest/api/3/search` over the configured project, asking
  for `summary`, `status`, and `labels`; `POST /rest/api/3/issue` to create.
  An issue is closed when its status category is `done`.
- **Linear** — one GraphQL document: `issues(filter: {team: {key: {eq:
  $team}}})` to read, `issueCreate` to create. An issue is closed when its
  state type is `completed` or `canceled`.

The network call is not exercised by the gate; the request assembly, the
response parsing, and the diff computation are. A captured tracker payload in
a test drives the same diff a live run computes.

## Scope

The command reconciles story IDs, titles, and open-or-closed state. It does
not read or write issue descriptions, assignees, sprints, or comments, and it
does not close an issue when its story disappears from the code: an orphan is
reported for a human to resolve.
