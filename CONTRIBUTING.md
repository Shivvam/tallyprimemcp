# Contributing to TallyMCP

Thank you for your interest in contributing! Whether you are a Rust systems programmer, an accountant, a Chartered Accountant (CA), or an AI workflow developer, your contributions are welcome.

## Code of Conduct

This project follows the [Code of Conduct](CODE_OF_CONDUCT.md). By participating, you agree to uphold it.

---

## Non-Code & Domain Contributions (No Rust Needed!)

You don't need to know Rust to make high-impact contributions to TallyMCP. We actively welcome:

### 1. TallyPrime Version Compatibility Testing
Help test TallyMCP against different Tally versions, network setups, and company configurations.
- **TallyPrime 2.x / 3.x / 4.x / 5.0** and **Tally.ERP 9** (best-effort)
- Single-user (Silver) vs. Multi-user (Gold) network setups
- Windows 10/11, Windows Server, Wine/Linux setups
- Open an issue or discussion with your findings and `tally-mcp doctor` output.

### 2. Accounting Edge Cases & Anonymized XML Fixtures
Accounting rules in India have complex edge cases. We need real-world scenarios to expand our offline test suite:
- GST rounding discrepancies (CGST/SGST 1-paisa split round-off)
- Reverse Charge Mechanism (RCM) voucher structures
- TDS / TCS voucher tags
- Multi-currency transactions
- Empty companies or companies with tens of thousands of ledger entries
- *Note:* Always sanitize and scrub real company names, PANs, and GSTINs before sharing sample XML.

### 3. Report Packs (`src/packs/`)
TallyMCP uses declarative XML query definitions for extracting reports from Tally. You can help design and verify new report extraction definitions (like Aged Payables, Cost Center summaries, or Bank Reconciliation) by contributing TDL/XML queries.

### 4. Documentation & Setup Guides
- Setup tutorials for new MCP clients (Claude Desktop, Cursor, Windsurf, ChatGPT, LibreChat)
- Guides for office LAN configuration and firewall setup
- Troubleshooting tips for common Tally connectivity errors

---

## Contributing Code & Technical PRs

### Pull Requests Ground Rules (Enforced by CI)

1. **Never write non-JSON-RPC output to `stdout` during `serve`.** All diagnostics go to `stderr` or the audit log.
2. **Build Tally XML with the typed `quick-xml` builder only.** Never use raw string concatenation — this prevents XML injection attacks.
3. **No MCP tool may commit a write to Tally without human approval.** Any tool that stages changes must route through the loopback approval token flow.
4. **All tests must pass offline.** CI enforces that the test suite passes with zero network access.

### Setting Up the Development Environment

**Prerequisites:**
- Rust 1.80+ (`rustup update stable`)
- Visual Studio 2022 Build Tools with C++ workload and Windows SDK (for Windows `.exe` and MSI builds)
- Linux or macOS can be used for developing core logic, MCP routing, and offline tests

**Build and Run Tests:**

```bash
git clone https://github.com/Shivvam/tallyprimemcp.git
cd tallyprimemcp/tallymcp
cargo build
cargo test
```

**Before Submitting a PR:**

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

### Commit Style

We follow the [Conventional Commits](https://www.conventionalcommits.org/) format:

```text
feat(validator): add strict financial year date check
fix(journal): handle atomic rename failure on Windows FAT32
docs(readme): add BYOK offline profile section
test(write_safety): add idempotency key collision test
```

---

## Contributor License Agreement

By submitting a pull request, you agree that your contribution is dual-licensed under both the [MIT License](LICENSE-MIT) and the [Apache License (Version 2.0)](LICENSE-APACHE), the same licenses as this project.

## Getting Help

- [GitHub Discussions](https://github.com/Shivvam/tallyprimemcp/discussions) for general questions, setup support, and feature ideas
- [GitHub Issues](https://github.com/Shivvam/tallyprimemcp/issues) for reproducible bug reports
- [Security Policy](SECURITY.md) for reporting potential security vulnerabilities privately
