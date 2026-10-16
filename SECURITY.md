# Security Policy

## Supported Versions

| Version | Supported |
|---|---|
| 1.x (latest) | ✅ |
| < 1.0 | ❌ |

## Reporting a Vulnerability

Please **do not** open a public GitHub issue for security vulnerabilities.

To report a vulnerability privately:

1. Go to the [Security Advisories](https://github.com/Shivvam/tallyprimemcp/security/advisories/new) page on GitHub.
2. Click **Report a vulnerability**.
3. Fill in the details: affected component, reproduction steps, potential impact, and any suggested fix.

You will receive an acknowledgement within **72 hours**. We aim to release a patch within **14 days** of a confirmed critical vulnerability.

## Scope

In scope:
- Authentication bypass or token prediction in the loopback approval UI
- Vulnerabilities allowing an MCP tool call to commit a write to TallyPrime without human approval
- Information disclosure of API keys or accounting data outside the configured egress allow-list
- Crash or data-corruption bugs in the write-ahead journal or atomic rename path
- DNS rebinding or CSRF bypasses in the companion UI server

Out of scope:
- Issues that require physical access to the machine running TallyMCP
- Vulnerabilities in TallyPrime itself
- Issues in third-party LLM provider APIs (Anthropic, OpenAI, Google)

## Security Design Summary

TallyMCP enforces safety outside the AI model:

- **No LLM commit path:** No MCP tool can post to Tally without a separate human-initiated HTTP request to the loopback approval server.
- **One-time HMAC token:** Each draft approval URL carries a random token invalidated after first use.
- **Host header enforcement:** The companion UI rejects requests whose `Host` header is not `127.0.0.1` or `localhost`.
- **Egress allow-list:** Outbound connections are limited to `127.0.0.1`, configured Tally host, and AI provider endpoints (only when BYOK document extraction is enabled).
- **No telemetry:** Zero external connections are made by the core MCP server.

## Disclosure Credit

Responsible disclosures will be credited in the release notes unless the reporter prefers anonymity.
