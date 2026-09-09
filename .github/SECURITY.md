# Security policy

## Supported versions

We support the latest stable release of Rivet. We backport security fixes on a
case-by-case basis for critical vulnerabilities.

| Version | Supported |
| :--- | :--- |
| latest | yes |
| older versions | no |

## Reporting a vulnerability

Report vulnerabilities privately:

1. Open a [private security advisory](https://github.com/mattjaikaran/rivet/security/advisories/new).
2. Do not open a public issue.
3. Include a description of the vulnerability, steps to reproduce it, and its
   potential impact.

We acknowledge receipt within 48 hours, provide a fix as soon as it is ready,
and disclose the issue after the fix is released.

## Security properties under design

- Compile-time RBAC: role checks enforced when the app compiles, with no
  runtime lookup overhead.
- Type safety: the DSL parser rejects untyped handlers and dynamic types.
- Dependency scanning via `cargo-deny` in CI.
- SQL injection prevention via `sqlx` compile-time checked queries (arrives
  with the data layer).

## Disclosure

We follow a 90-day disclosure window. After a fix is available, we publish a
security advisory on GitHub.
