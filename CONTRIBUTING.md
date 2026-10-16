# Contributing to TallyMCP

Thank you for taking the time to contribute! All contributions are welcome: bug reports, documentation improvements, tests, and code.

## Code of Conduct

This project follows the [Code of Conduct](CODE_OF_CONDUCT.md). By participating, you agree to uphold it.

## How to Contribute

### Reporting Bugs

1. Search [existing issues](https://github.com/Shivvam/tallyprimemcp/issues) first.
2. Open a new issue using the **Bug report** template.
3. Include: TallyMCP version (`tally-mcp version`), TallyPrime version, OS, and the output of `tally-mcp doctor` (redact company names).

### Suggesting Features

Open a [feature request](https://github.com/Shivvam/tallyprimemcp/issues/new?template=feature_request.md). Check the [Roadmap](README.md#roadmap) first to avoid duplicates.

### Pull Requests

**Ground rules (enforced by CI):**

1. **Never write non-JSON-RPC output to `stdout` during `serve`.** All diagnostics go to `stderr` or the audit log.
2. **Build Tally XML with the typed `quick-xml` builder only.** Never raw string concatenation — this is how we prevent XML injection.
3. **No MCP tool may commit a write to Tally without human approval.** Any tool that calls `ImportData` must route through the approval token flow.
4. **Add tests for new behaviour.** The test suite must stay at 100% pass rate.

**Before opening a PR:**

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

### Setting Up the Development Environment

**Prerequisites:**
- Rust 1.80+ (`rustup update stable`)
- Visual Studio 2022 Build Tools with the C++ workload and Windows 10/11 SDK (for Windows `.exe` / MSI builds)
- Linux or macOS can be used for core logic, tests, and cross-compilation

**Build:**

```bash
git clone https://github.com/Shivvam/tallyprimemcp.git
cd tallymcp
cargo build
cargo test
```

**Running tests without TallyPrime:**

All 19 tests are fully offline. No TallyPrime installation is required:

```bash
cargo test
```

### Project Structure

See [Project Layout](README.md#project-layout) in the README.

### Commit Style

Use the [Conventional Commits](https://www.conventionalcommits.org/) format:

```
feat(validator): add strict financial year date check
fix(journal): handle atomic rename failure on Windows FAT32
docs(readme): add BYOK offline profile section
test(write_safety): add idempotency key collision test
```

## Contributor License Agreement

By submitting a pull request you agree that your contribution is licensed under the MIT License, the same license as this project.

## Getting Help

- [GitHub Discussions](https://github.com/Shivvam/tallyprimemcp/discussions) for questions and ideas
- [Issues](https://github.com/Shivvam/tallyprimemcp/issues) for bugs and feature requests
