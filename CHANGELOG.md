# Changelog

All notable changes to TallyMCP are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [1.0.0] — 2026-10-15

### Added

- **MCP server** (`tally-mcp serve`) over stdio (JSON-RPC 2.0, MCP spec 2024-11-05).
- **Read tools:** `tally_list_companies`, `tally_get_active_company`, `tally_set_active_company`, `tally_list_ledgers`, `tally_get_ledger_balance`, `tally_trial_balance`, `tally_day_book`, `tally_ledger_vouchers`, `tally_outstandings`, `tally_run_report`.
- **Draft tools (two-phase commit):** `tally_draft_voucher`, `tally_draft_ledger`, `tally_preview_draft`, `tally_discard_draft`.
- **Document extraction tool:** `tally_extract_document` — local JSON/PDF/image ingestion into `DocumentExtraction`; routes through BYOK AI provider when configured.
- **Write-ahead journal** with atomic `.tmp` → `.json` rename and startup crash reconciliation.
- **Loopback companion UI** (`tally-mcp ui`) on `127.0.0.1:9190` with one-time HMAC approval token and DNS-rebinding protection.
- **Circuit breaker** on the Tally XML gateway (configurable failure threshold and reset interval).
- **`tally-mcp doctor`** diagnostics: Tally reachability, circuit breaker state, loaded companies, journal reconciliation.
- **`tally-mcp setup`** interactive wizard: detects Tally port, prints client config snippet, supports `--host` / `--port` flags for LAN connections.
- **`tally-mcp cleanup`** data scrubber with `--dry-run`, `--backup`, and `--categories` flags.
- **Tool packs** — declarative JSON read-only report packs in `%APPDATA%\TallyMCP\tools\*.json`. Bundled: `outstanding-ageing`, `gst-summary`, `stock-thresholds`.
- **BYOK document extraction** via Anthropic, OpenAI, Ollama, or Gemini (OpenAI-compat).
- **Zero-balance validator** with `rust_decimal` precision, idempotency fingerprinting, blocked-ledger pattern enforcement.
- **Tamper-evident rotating audit log** in `%LOCALAPPDATA%\TallyMCP\audit\YYYY-MM.jsonl`.
- **UTF-16LE with BOM** XML encoding for full `₹` and multilingual character support in Tally.
- **Deterministic XML builder** using `quick-xml` escape — no string concatenation, no injection vector.
- **MSI installer** built with WiX Toolset v4, per-user install, no UAC elevation required.
- **19 offline tests** covering write safety, gateway parsing, lifecycle/cleanup, pack enforcement, and UI/BYOK flows.
- **Single binary** — 1.38 MB release binary, < 15 ms cold start, ~21 MB idle RAM.
- **Environment variable overrides:** `TALLY_HOST` and `TALLY_PORT` for LAN connectivity without editing config.
- **LAN support:** `tally-mcp serve --host <IP> --port <PORT>` connects to remote TallyPrime over local network.

### Security

- Host-header enforcement on the companion UI blocks DNS-rebinding attacks.
- CSRF protection on POST endpoints (Origin header allow-list).
- Egress allow-list: only `127.0.0.1`/`localhost`, configured Tally host, and AI provider endpoints (when BYOK is enabled).
- API keys stored in the OS credential store (Windows Credential Manager), never written to disk in plaintext.
- No telemetry: zero bytes sent to any external service by the MCP server itself.

---

[1.0.0]: https://github.com/Shivvam/tallyprimemcp/releases/tag/v1.0.0
