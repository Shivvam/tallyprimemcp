<!--
  AI AGENT DISCOVERY & LLM CONTEXT METADATA
  Repository : Shivvam/tallyprimemcp
  Name       : TallyMCP
  Protocol   : Model Context Protocol (MCP) 2024-11-05
  Transport  : stdio (JSON-RPC 2.0)
  Language   : Rust (2021 edition, MSRV 1.80)
  Purpose    : Local Windows MCP server bridging TallyPrime to LLM clients
  Security   : No telemetry; Tally traffic over loopback only; human approval required for all writes
  Binary     : tally-mcp.exe
-->

<div align="center">

# TallyMCP

**A local, safety-first [Model Context Protocol](https://modelcontextprotocol.io) server for TallyPrime.**
Let AI assistants read your accounting data and *propose* vouchers, while a human always approves the write.

[![CI](https://github.com/Shivvam/tallyprimemcp/actions/workflows/release.yml/badge.svg)](https://github.com/Shivvam/tallyprimemcp/actions/workflows/release.yml)
[![Release](https://img.shields.io/github/v/release/Shivvam/tallyprimemcp?style=flat-square)](https://github.com/Shivvam/tallyprimemcp/releases)
[![Downloads](https://img.shields.io/github/downloads/Shivvam/tallyprimemcp/total?style=flat-square)](https://github.com/Shivvam/tallyprimemcp/releases)
[![License: MIT](https://img.shields.io/badge/license-MIT-green?style=flat-square)](LICENSE)
[![MCP](https://img.shields.io/badge/MCP-2024--11--05-blueviolet?style=flat-square)](https://modelcontextprotocol.io)
[![Platform](https://img.shields.io/badge/platform-Windows%2010%2F11%20x64-lightgrey?style=flat-square)](#requirements)
[![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen?style=flat-square)](CONTRIBUTING.md)

[Quick Start](#quick-start) · [Examples](#example-prompts) · [Tools](#mcp-tools-reference) · [Security](#security-model) · [Configuration](#configuration) · [Troubleshooting](#troubleshooting) · [Contributing](#contributing)

</div>

---

## Table of Contents

- [Overview](#overview)
- [Key Features](#key-features)
- [How It Works](#how-it-works)
- [Requirements](#requirements)
- [Quick Start](#quick-start)
- [Example Prompts](#example-prompts)
- [MCP Tools Reference](#mcp-tools-reference)
- [Security Model](#security-model)
- [Privacy and Data Flow](#privacy-and-data-flow)
- [CLI Reference](#cli-reference)
- [Configuration](#configuration)
- [File Locations](#file-locations)
- [Troubleshooting](#troubleshooting)
- [Known Limitations](#known-limitations)
- [Development](#development)
- [Project Layout](#project-layout)
- [Roadmap](#roadmap)
- [Contributing](#contributing)
- [Security Policy](#security-policy)
- [Support and Community](#support-and-community)
- [License and Trademarks](#license-and-trademarks)

---

## Overview

TallyMCP is a single native Windows executable, written in Rust, that connects **TallyPrime** to any MCP-compatible client such as Claude Desktop, Cursor, Cline, or Windsurf.

It talks to TallyPrime through its built-in XML-over-HTTP interface on the local machine (`127.0.0.1:9000` by default). No cloud relay, reverse-engineered API, or hosted service is involved.

**Design principle:** an AI model can *read* accounting data and *draft* changes, but it can never commit a change on its own. Every write needs an explicit human click in a local approval page.

### Why this exists

Accounting data is high-stakes. Letting an LLM emit raw XML straight into a ledger risks unbalanced entries, duplicate vouchers, and silent corruption. TallyMCP puts a deterministic, validated, auditable layer between the model and Tally.

## Key Features

| Area | What you get |
|---|---|
| **Read access** | Companies, ledgers, balances, trial balance, day book, ledger vouchers, outstandings |
| **Draft-only writes** | Vouchers and ledgers are staged as drafts. No MCP tool performs a commit |
| **Human approval** | One-time-token approval page served on loopback; commit happens only after a physical click |
| **Deterministic XML** | Typed XML generation with `quick-xml`; no string concatenation; UTF-16LE with BOM so `₹` and multilingual text survive |
| **Crash safety** | Write-ahead journal and startup reconciliation against Tally GUIDs |
| **Idempotency** | Fingerprinting prevents duplicate vouchers |
| **No telemetry** | No analytics, no phone-home |
| **Offline option** | Run fully air-gapped with a local LLM (Ollama / LM Studio) |
| **Small footprint** | Single binary, no runtime dependencies (see [Benchmarks](#benchmarks) for measured numbers) |

## How It Works

```
┌────────────────────────────────────────────────────────┐
│   MCP Client (Claude Desktop / Cursor / Cline / ...)   │
└──────────────────────────┬─────────────────────────────┘
                           │ stdio (JSON-RPC 2.0)
                           ▼
┌────────────────────────────────────────────────────────┐
│                  tally-mcp.exe (core)                  │
│                                                        │
│  ┌────────────────────┐      ┌──────────────────────┐  │
│  │   MCP Server API   │      │  Write-Ahead Journal │  │
│  │   (read + draft)   │      │  + Crash Recovery    │  │
│  └─────────┬──────────┘      └──────────┬───────────┘  │
│            │                            │              │
│  ┌─────────▼──────────┐      ┌──────────▼───────────┐  │
│  │ Typed XML Builder  │      │  Loopback Approval   │◄─┼── Human click
│  │ and Validator      │      │  UI (token-gated)    │  │
│  └─────────┬──────────┘      └──────────────────────┘  │
└────────────┼───────────────────────────────────────────┘
             │ XML over HTTP, loopback only
             ▼
┌────────────────────────────────────────────────────────┐
│              TallyPrime (same Windows PC)              │
└────────────────────────────────────────────────────────┘
```

**Write lifecycle (two-phase commit):**

1. The model calls `tally_draft_voucher`.
2. TallyMCP validates it (balanced entries, date within financial period, party ledger exists, blocked-ledger rules).
3. The draft is saved atomically to the journal and an approval URL is returned. **Nothing is sent to Tally yet.**
4. A human opens the URL, reviews the line items and the exact XML, and clicks **Approve**.
5. TallyMCP sends the XML to Tally, then reads back the voucher GUID to verify the write.

## Requirements

| Requirement | Details |
|---|---|
| OS | Windows 10 / 11 (x64) |
| TallyPrime | **TallyPrime Release 5.0+** (latest) and Release 4.x / 3.x recommended (Release 2.x / 1.x supported; Tally.ERP 9 best-effort). Fully compatible with Silver, Gold, and TallyPrime Edit Log editions |
| Tally HTTP/XML | Must be enabled (see [Quick Start](#1-enable-tallyprimes-xml-server)) |
| MCP client | Any client supporting MCP over stdio |
| To build from source | Rust 1.80+, Visual Studio 2022 Build Tools (C++ workload and Windows SDK) |

## Quick Start

### 1. Enable TallyPrime's XML server

TallyPrime must be configured to accept local XML requests:

- **TallyPrime Release 5.0+ (Latest), 4.x & 3.0+:**
  1. Open TallyPrime and open your company.
  2. Press **F1: Help → Settings → Connectivity → Client/Server configuration**.
  3. Set **TallyPrime acts as** to `Both` (or set **Enable ODBC** to `Yes` and **Enable HTTP Server** to `Yes`).
  4. Set **Port** to `9000`.
  5. Restart TallyPrime and keep the company open.
- **TallyPrime Release 1.x / 2.x:**
  1. Press **F1: Help → Settings → Advanced Configuration**.
  2. Set **Enable HTTP Server** to `Yes` (Port `9000`).
- **Tally.ERP 9 (Legacy):**
  1. Press **F12: Configure → Advanced Configuration**.
  2. Set **Tally Act as** to `Both` / `Server` (Port `9000`).

Verify it responds:

```powershell
curl http://127.0.0.1:9000
```

### 2. Install TallyMCP

**winget**
```powershell
winget install TallyMCP.TallyMCP
```

**Installer or standalone binary**

Download `tally-mcp-<version>-x64.msi` or `tally-mcp.exe` from [Releases](https://github.com/Shivvam/tallyprimemcp/releases).

**Verify your download** (recommended):
```powershell
Get-FileHash .\tally-mcp.exe -Algorithm SHA256
# Compare with SHA256SUMS.txt attached to the release
```

**Guided setup**
```powershell
tally-mcp.exe setup
tally-mcp.exe doctor    # confirms Tally is reachable
```

### 3. Connect your MCP client

<details>
<summary><b>Claude Desktop</b></summary>

Edit `%APPDATA%\Claude\claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "tally": {
      "command": "C:\\Users\\<YourUsername>\\AppData\\Local\\Programs\\TallyMCP\\tally-mcp.exe",
      "args": ["serve"]
    }
  }
}
```
Restart Claude Desktop fully after saving.
</details>

<details>
<summary><b>Cursor</b></summary>

`.cursor/mcp.json` (project) or global settings:

```json
{
  "mcpServers": {
    "tally": {
      "command": "tally-mcp",
      "args": ["serve"]
    }
  }
}
```
Use the full path to the `.exe` if `tally-mcp` is not on your `PATH`.
</details>

<details>
<summary><b>Cline (VS Code)</b></summary>

Cline Settings → MCP Servers → Add:

- **Name:** `tally`
- **Command:** `tally-mcp`
- **Arguments:** `["serve"]`
</details>

<details>
<summary><b>Windsurf and other clients</b></summary>

Any client that launches a stdio MCP server works. Use the same `command` / `args` pattern as above.
</details>

### 4. Try it

Ask your assistant: *"List my Tally companies and show the trial balance for FY 2025-26."*

## Example Prompts

**Read-only**
- "Which customers have outstanding receivables older than 60 days?"
- "Show the day book for 1 April 2026."
- "What is the closing balance of *HDFC Bank A/c*?"

**Drafting (always requires your approval)**
- "Draft a purchase voucher from *ABC Traders* for ₹45,000 dated 12 Apr 2026, narration *Office supplies*."
- "Create a ledger *Staff Welfare* under *Indirect Expenses*."

The assistant replies with an approval link. Open it, review, and click **Approve**.

## MCP Tools Reference

### Read tools (no side effects)

| Tool | Parameters | Description |
|---|---|---|
| `tally_list_companies` | none | Lists companies currently loaded in Tally |
| `tally_get_active_company` | none | Returns the session's active company |
| `tally_set_active_company` | `company` | Sets the active company for this session |
| `tally_list_ledgers` | `company?` | Lists ledgers |
| `tally_get_ledger_balance` | `ledger_name`, `company?` | Opening, closing, and current balance |
| `tally_trial_balance` | `company?`, `from_date`, `to_date` | Structured trial balance |
| `tally_day_book` | `company?`, `date` (`YYYYMMDD`) | All vouchers on a date |
| `tally_ledger_vouchers` | `ledger_name`, `from_date`, `to_date` | Vouchers for a ledger |
| `tally_outstandings` | `company?`, `report_type`, `as_on` | Receivable/payable ageing |
| `tally_run_report` | `tool_name`, `arguments` | Runs a read-only report from a loaded [tool pack](#tool-packs) |

### Draft tools (two-phase commit)

| Tool | Parameters | Description |
|---|---|---|
| `tally_draft_voucher` | `company`, `voucher_type`, `date`, `number`, `party_ledger`, `entries`, `narration` | Validates and stages a voucher; returns an approval URL. **Writes nothing to Tally** |
| `tally_draft_ledger` | `company`, `name`, `parent_group`, `opening_balance` | Stages a ledger creation |
| `tally_preview_draft` | `draft_id` | Returns the exact XML that would be posted |
| `tally_discard_draft` | `draft_id` | Discards an unapproved draft |

### Document extraction (optional, bring your own key)

| Tool | Parameters | Description |
|---|---|---|
| `tally_extract_document` | `file_path` | Extracts a local invoice/receipt (JSON, PDF, image) into a canonical `DocumentExtraction` for drafting |

Extracted data is only ever used to build a **draft**; it goes through the same validation and approval flow.

### Tool packs

Custom read-only reports can be added as declarative JSON packs in `%APPDATA%\TallyMCP\tools\*.json`. Unsigned user packs are disabled by default (`packs.enable_unsigned = false`). Bundled packs include `outstanding-ageing`, `gst-summary`, and `stock-thresholds`. All pack requests are strictly enforced as read-only Export envelopes—any attempt to invoke mutating `Import Data` actions is rejected at the gateway level.

## Security Model

TallyMCP assumes the AI model may be wrong, manipulated, or malicious, and enforces safety **outside** the model.

### Threat model (summary)

| Threat | Mitigation |
|---|---|
| Model emits an unbalanced or malformed voucher | Rule validator rejects it before staging |
| Model tries to commit directly | No commit tool exists in the MCP surface |
| Duplicate submission | `sha256(company \| type \| party \| number \| date \| total)` fingerprint |
| Crash mid-write | Write-ahead journal, GUID read-back, reconciliation on startup and `doctor` |
| Other local processes hitting the approval page | Random port, one-time HMAC token, loopback binding |
| Prompt injection via Tally data (e.g., a malicious narration) | Writes still require human review; review the approval page, not the chat |
| Accidental writes to sensitive ledgers | `blocked_ledger_patterns` |
| Unexpected network egress | Hard allow-list (see below) |
| Credential leakage | API keys stored in Windows Credential Manager, never plaintext on disk |

### Write pipeline

```
AI draft → Validate → Journal (atomic .tmp→.json) → Approval token
        → Human review + click → Commit (UTF-16LE XML) → Read-back verify
```

### Invariants

1. **Balanced entries:** `Σ Debits + Σ Credits == 0`.
2. **Idempotency:** duplicate fingerprints are rejected.
3. **No LLM commits:** approval is a human-only action.
4. **Crash reconciliation:** drafts stuck in `committing` are checked against Tally by GUID, then verified or reverted.
5. **Egress allow-list:** only `127.0.0.1` / `localhost`, plus the AI provider endpoints you configure (`api.anthropic.com`, `api.openai.com`, `generativelanguage.googleapis.com`) and **only when document extraction is enabled**.

> This project has not yet undergone an independent security audit. See [SECURITY.md](SECURITY.md) to report vulnerabilities.

## Privacy and Data Flow

| Component | Where data goes | Notes |
|---|---|---|
| TallyMCP core | Stays on the local machine | No telemetry or analytics |
| Tool results via a cloud MCP client | Sent to that client's provider as part of the conversation | Governed by *their* terms (Anthropic, OpenAI, Cursor, etc.). **Ledger names, balances, and vouchers you query become part of the prompt context** |
| BYOK document extraction | Sent directly from your PC to your chosen provider with your key | No intermediary |
| Fully offline profile | Local LLM client plus local Ollama/LM Studio for extraction | No data leaves the machine |

Choose your MCP client according to your organisation's data-handling policy. Use the offline profile for regulated or confidential books.

## CLI Reference

```
tally-mcp.exe <COMMAND>
```

| Command | Purpose |
|---|---|
| `serve` | Run the stdio MCP server (used by MCP clients) |
| `ui` | Launch the loopback companion UI for approvals and document drop |
| `setup` | Interactive setup (Tally detection, port, API keys) |
| `doctor` | Diagnostics: Tally reachability, circuit breaker, companies, journal reconciliation |
| `cleanup` | Uninstall and data scrub (`--dry-run` supported) |
| `version` | Print version and build commit |

> During `serve`, `stdout` is reserved for JSON-RPC. All logs go to `stderr` and the audit log.

## Configuration

File: `%APPDATA%\TallyMCP\config.toml`

```toml
[tally]
host = "127.0.0.1"
port = 9000
timeout_ms = 5000
allow_remote_tally = false      # keep false unless you fully understand the risk
breaker_failures = 3
breaker_reset_secs = 30
queue_depth = 64

[safety]
write_mode = "require_approval"  # "require_approval" | "read_only"
blocked_ledger_patterns = ["^Cash$", "^Bank.*Reserved"]
require_explicit_company = true
strict_financial_year = true

[ai]
profile = "cloud"                # "cloud" | "offline"
provider = "anthropic"           # "anthropic" | "openai" | "openai_compat"
offline_endpoint = "http://127.0.0.1:11434"
confidence_threshold = 0.90

[packs]
enable_unsigned = false
tools_dir = "tools"
```

**Tip:** set `write_mode = "read_only"` to disable drafting entirely.

## File Locations

| Item | Path |
|---|---|
| Executable | `%LOCALAPPDATA%\Programs\TallyMCP\` |
| Config | `%APPDATA%\TallyMCP\config.toml` |
| Tool packs | `%APPDATA%\TallyMCP\tools\*.json` |
| Drafts / journal | `%LOCALAPPDATA%\TallyMCP\drafts\`, `...\journal\` |
| Audit log | `%LOCALAPPDATA%\TallyMCP\audit\audit.log` |
| Secrets | Windows Credential Manager, `TallyMCP/<provider>` |

## Troubleshooting

| Symptom | Likely cause and fix |
|---|---|
| `doctor` says Tally unreachable | Tally isn't running, no company is open, or the port differs from `config.toml` |
| MCP client shows server "failed to start" | Wrong path in config (escape backslashes as `\\`); try the absolute path |
| Tools return empty results | No active company; call `tally_set_active_company` |
| `₹` or regional text appears garbled | Make sure you're on a current release; encoding is handled as UTF-16LE |
| Draft stuck in `committing` | Run `tally-mcp doctor` to reconcile |
| Approval link doesn't open | Run `tally-mcp ui` and check your firewall isn't blocking loopback |
| Port 9000 in use | Change the port in both Tally and `config.toml` |

Still stuck? Run `tally-mcp doctor` and attach its output (redact company names) to an [issue](https://github.com/Shivvam/tallyprimemcp/issues).

## Known Limitations

- **OS Platform:** Windows 10/11 x64 (matches native TallyPrime runtime requirements).
- **Supported Tally Versions:** Tested and optimized for **TallyPrime Release 5.0+** (including TallyPrime Edit Log), Release 4.x, and Release 3.x; backwards-compatible with Release 2.x/1.x and Tally.ERP 9 (best-effort).
- **Supported Voucher Types:** Financial vouchers (`Purchase`, `Sales`, `Payment`, `Receipt`, `Journal`) with full debit/credit balancing and multi-ledger entries. Tax ledgers (CGST, SGST, IGST) are supported via explicit ledger mapping. Inventory stock item mutations and stock journal transfers are planned for a future release (read-only inventory queries are supported via tool packs).
- **Draft Isolation:** Draft vouchers reside exclusively in the local write-ahead journal (`%LOCALAPPDATA%\TallyMCP\drafts\`) and are never visible inside Tally until explicitly approved and committed by a human.
- **Network Topologies:** Single-user (Silver) and multi-user (Gold) running on the same machine communicate over loopback `127.0.0.1:9000`. Remote Tally instances over LAN require setting `allow_remote_tally = true` in `config.toml` (note that Tally's XML/HTTP channel is unauthenticated plain HTTP).
- **Independent Project:** TallyMCP is not affiliated with Tally Solutions Pvt. Ltd.; behaviour is verified across official TallyPrime releases.

## Development

### Prerequisites
- Rust 1.80+ (`x86_64-pc-windows-msvc` for production builds)
- Visual Studio 2022 Build Tools (Windows)
- Linux/macOS can be used for core logic and tests

### Build and test

```bash
git clone https://github.com/Shivvam/tallyprimemcp.git
cd tallymcp

cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```

Output: `target/release/tally-mcp.exe` (Windows) or `target/release/tally-mcp` (Linux/macOS).

### Offline & Mock Testing

Contributors do not need a licensed TallyPrime installation to run the test suite. The project includes deterministic XML fixtures, UTF-16LE BOM decoders, crash-injection drills, and offline mock gateway tests:

```bash
# Run all 19 offline safety, validation, and gateway tests
cargo test
```

### Benchmarks (CI-Verified)

Metrics are automatically captured on every production release and published in `release/facts.json`:

| Metric | Target Gate | Measured Production Metric |
|---|---|---|
| **Full Binary Size** | ≤ 5.5 MB | **1.38 MB** (`1,386,928 bytes`) |
| **Core Engine Size** | ≤ 3.5 MB | **900 KB** (`921,600 bytes`) |
| **MSI Package Size** | ≤ 5.0 MB | **1.4 MB** (`1,478,000 bytes`) |
| **Cold Start (P95)** | < 150 ms | **< 15 ms** |
| **Idle Memory (RAM)**| < 30 MB | **~21 MB** |
| **CI Passing Tests** | 100% | **19 / 19 passed** |
| **Telemetry Calls** | 0 bytes | **0 (Zero external egress)** |

## Project Layout

```
tallymcp/
├── release/                        # Shared release contracts and build facts
│   ├── facts.json                  # Machine-generated release metrics and claims
│   ├── facts.schema.json           # Schema for facts verification
│   ├── release-manifest.json       # Signed release manifest for distribution
│   └── checksums.txt               # Verified SHA-256 asset checksums
├── tallymcp/                       # Native Rust Core & Companion UI
│   ├── Cargo.toml                  # Single source of truth for versioning (v1.0.0)
│   ├── installer/
│   │   └── Package.wxs             # WiX Toolset v4/v5 per-user MSI definition
│   ├── src/
│   │   ├── main.rs                 # CLI entry point (serve, ui, setup, doctor, cleanup)
│   │   ├── lib.rs                  # Crate root and module exports
│   │   ├── mcp/                    # Model Context Protocol server & JSON-RPC router
│   │   ├── tally/                  # XML client, builder, circuit breaker & envelope checks
│   │   ├── validator.rs            # Mathematical zero-balance and financial period checks
│   │   ├── journal.rs              # Write-ahead journal & atomic crash recovery
│   │   ├── audit.rs                # Tamper-evident rotating audit logger
│   │   ├── config.rs               # TOML config loader & Windows path resolver
│   │   ├── models.rs               # Canonical models (VoucherDraft, DocumentExtraction)
│   │   ├── doctor.rs               # Diagnostics & connectivity checker
│   │   ├── cleanup.rs              # Uninstaller & data scrubber
│   │   ├── manifest.rs             # Installation manifest tracker
│   │   ├── packs/                  # Read-only report pack loader & validator
│   │   ├── byok/                   # Vision/LLM invoice parser & provider connectors
│   │   └── ui/                     # Loopback companion approval server & HTML/JS assets
│   └── tests/                      # Gateway, write safety, lifecycle & pack tests
└── tallymcp-web/                   # Next.js 16 + Tailwind CSS Distribution Portal
    ├── src/app/                    # Landing page, docs, and uncached /api/download routes
    └── src/lib/release.ts          # Release manifest resolver & rate limiting
```

## Roadmap

- [ ] Signed releases and SBOM
- [ ] Mock Tally server for contributor testing
- [ ] Additional voucher types and inventory support
- [ ] Optional MCP resource/prompts support
- [ ] Localised approval UI

Have an idea? Open a [feature request](https://github.com/Shivvam/tallyprimemcp/issues/new?template=feature_request.md).

## Contributing

Contributions of all sizes are welcome: bug reports, docs, tests, and code.

**Ground rules**
1. Never write non-JSON-RPC output to `stdout` during `serve`.
2. Build Tally XML with the typed `quick-xml` builder only, never raw string concatenation.
3. Don't add any MCP tool that can commit a write without human approval.
4. Add tests for new behaviour. Before opening a PR run:
   ```bash
   cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
   ```

Please read [CONTRIBUTING.md](CONTRIBUTING.md) and the [Code of Conduct](CODE_OF_CONDUCT.md). Look for issues labelled `good first issue`.

## Security Policy

Please **do not** open public issues for vulnerabilities. Follow [SECURITY.md](SECURITY.md) for private disclosure.

## Support and Community

- **Bugs / features:** [GitHub Issues](https://github.com/Shivvam/tallyprimemcp/issues)
- **Questions / ideas:** [GitHub Discussions](https://github.com/Shivvam/tallyprimemcp/discussions)
- **Changelog:** [CHANGELOG.md](CHANGELOG.md)

## License and Trademarks

Released under the [MIT License](LICENSE).

**Tally** and **TallyPrime** are trademarks of Tally Solutions Pvt. Ltd. TallyMCP is an independent open-source project, not affiliated with, endorsed by, or sponsored by Tally Solutions Pvt. Ltd.

**Disclaimer:** TallyMCP is provided "as is". You remain responsible for reviewing every approved entry and maintaining backups of your Tally data. It is not a substitute for professional accounting judgement.

---

<div align="center">
  <sub>Built for accountants, engineers, and anyone who wants AI assistance without giving up control of the books.</sub>
</div>
