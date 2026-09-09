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
