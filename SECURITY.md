# Security Policy

## Supported Versions

| Version | Supported |
|---|---|
| 1.x (latest) | ✅ |
| < 1.0 | ❌ |

## Reporting a Vulnerability

Please **do not** open a public GitHub issue or public discussion for potential security vulnerabilities.

To report a vulnerability privately, use one of the following channels:

### Option 1: GitHub Private Vulnerability Reporting (Preferred)
1. Navigate to the [Security Advisories](https://github.com/Shivvam/tallyprimemcp/security/advisories/new) tab on GitHub.
2. Click **Report a vulnerability**.
3. Provide details: affected component, reproduction steps, potential impact, and suggested mitigation.

### Option 2: Direct Email
If you do not have a GitHub account or prefer email:
- **Email:** `security@tallymcp.pro` (with subject line: `[SECURITY] TallyMCP Vulnerability Report`)

### Response Timelines & SLA
- **Initial acknowledgement:** Within **48 to 72 hours**.
- **Triage & severity assessment:** Within **5 business days**.
- **Patch release:** Within **14 business days** for confirmed critical or high severity vulnerabilities.

---

## Scope

### In Scope
- Authentication bypass or HMAC token prediction in the loopback approval UI (`src/ui/`)
- Any path allowing an MCP tool call to commit or write to TallyPrime without explicit human approval
- Leaks of sensitive keys or accounting data outside configured egress boundaries
- Write-ahead journal corruption or atomic rename failures risking data loss
- DNS rebinding, CSRF, or Host header validation bypasses in the local companion UI

### Out of Scope
- Physical access attacks on the host machine running TallyMCP
- Unpatched vulnerabilities in TallyPrime itself or the Windows operating system
- Social engineering attacks
- Third-party LLM provider API failures (Anthropic, OpenAI, Google)

---

## Security Design Principles

TallyMCP is designed from the ground up to prevent AI agents from altering financial records autonomously:

- **No LLM commit path:** No MCP tool has the ability to post data to Tally directly. Every write requires a human click on a local, token-gated approval UI.
- **One-time HMAC token:** Each draft approval URL is bound to a single-use random HMAC token.
- **Strict Host header enforcement:** The loopback approval server rejects all requests whose `Host` header is not `127.0.0.1` or `localhost`.
- **Egress isolation:** Outbound network connections are strictly restricted to `127.0.0.1` and configured Tally LAN IP. Core server contains zero telemetry.

## Responsible Disclosure & Attribution

Security researchers who responsibly disclose vulnerabilities will be credited in release notes and the project advisory hall of fame (unless anonymity is requested).
