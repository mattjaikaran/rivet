# Security Policy

## Supported Versions

We only support the **latest stable release** of Rivet. Security fixes are backported on a case-by-case basis for critical CVEs.

| Version | Supported |
| :--- | :--- |
| latest (v0.x) | ✅ |
| older versions | ❌ |

---

## Reporting a Vulnerability

If you discover a security vulnerability, please **report it privately**:

1. Email us at **rivet-security@example.com**.
2. Do **not** open a public issue.
3. Include:
   - A detailed description of the vulnerability.
   - Steps to reproduce it.
   - Potential impact (e.g., RCE, SQL injection, auth bypass).
   - Any suggested fixes (optional).

We will:
- Acknowledge receipt within **48 hours**.
- Provide a fix within **90 days**.
- Disclose the issue publicly after the fix is released.

---

## Security Best Practices (Built into Rivet)

- **Compile-Time RBAC**: Role-based access control is enforced at compile time. If a route accesses a service it shouldn't, the Rust compiler fails.
- **Type Safety**: Zero `any`/`unknown` types allowed in the DSL.
- **Dependency Scanning**: We use `cargo-deny` to scan for CVEs in all dependencies.
- **SQL Injection Prevention**: `sqlx` provides compile-time query checking.
- **Secure Defaults**: HTTPS is enabled by default in `rivet dev --https`.

---

## Disclosure Policy

We follow a **90-day disclosure window**. After a fix is available, we will publish a security advisory on GitHub.

---

## Contact

For non-security issues, please use GitHub Issues.