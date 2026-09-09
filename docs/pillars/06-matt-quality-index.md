# 6. The MQI (Matt Quality Index)
A multi-dimensional score calculated on every commit:

| Dimension | Metric | Weight |
| :--- | :--- | :--- |
Cyclomatic Complexity | < 8 | High |
Test Coverage | ≥ 80% | High |
Mutation Survival | 0% | Critical |
Dead Code | 0% | High |
Redundant Code | 0% | Medium |
Type Strictness | 0 any/unknown | High |
Documentation Coverage | 100% of public functions | Medium |

The overall grade (A+ to F) is reported in `rivet audit`.

## Phase-1 subset (`rivet audit`)

`rivet audit` runs the Gauntlet rules over one module and folds the
findings into a grade. Phase 1 scores the four dimensions that the rules
measure; the other three need the generated Rust crate and its tests, so
they stay Rust-side (see below).

| audit key | Dimension | Rule | Weight |
| :--- | :--- | :--- | :--- |
| `complexity` | Cyclomatic Complexity | `E2042` | High |
| `duplicate_code` | Redundant Code | `E2043` | Medium |
| `dead_code` | Dead Code | `E2044` | High |
| `type_strictness` | Type Strictness | `E2046` | High |

Weights map to numbers for the overall score: High = 3, Medium = 2,
Critical = 4 (reserved for mutation survival in a later phase).

### Grade scale and finding deductions

Every dimension starts at 100 points. A blocker finding deducts 20 points;
a warning deducts 10. A dimension cannot fall below 0. The overall score
is the weighted mean of the scored dimensions, rounded to one decimal
place. The score maps to a letter grade:

| score | grade |
| :--- | :--- |
| 97.0+ | A+ |
| 93.0-96.9 | A |
| 90.0-92.9 | A- |
| 87.0-89.9 | B+ |
| 83.0-86.9 | B |
| 80.0-82.9 | B- |
| 77.0-79.9 | C+ |
| 73.0-76.9 | C |
| 70.0-72.9 | C- |
| 60.0-69.9 | D |
| below 60.0 | F |

`rivet audit` reports the grade, per-dimension scores, and a machine
readable JSON breakdown. Story-gate findings (`E2045`) do not change the
grade: a storyless route already blocks `rivet build`. The breakdown
reports them separately under `story_gate`.

### Coverage and mutation survival (decision, 2026-09-09)

Test coverage, mutation survival, and documentation coverage measure the
generated Rust crate, not the DSL source. The generated-code test story
does not exist yet, so phase 1 cannot compute these metrics. Decision:
they stay Rust-side and are **not scored**; `rivet audit` lists them in
the `not_scored` array of the JSON breakdown with their reasons so the
index never reports a partial metric as complete. A dimension whose rule
the `[gauntlet]` config disables also appears in `not_scored`.

The mutation-tester roadmap bullet stays open until the generated-code
test story lands (see `docs/ROADMAP.md` phase 1).
