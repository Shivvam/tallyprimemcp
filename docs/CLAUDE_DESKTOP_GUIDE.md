# Claude Desktop & TallyMCP: Complete Integration & Self-Diagnostic Guide

A comprehensive, production-grade guide to connecting **Claude Desktop** with **TallyPrime** using **TallyMCP**. This setup operates **100% locally** over standard I/O (`stdio`), keeping your financial books completely private with zero cloud relays, tunnels, or external exposure.

---

## Table of Contents

1. [Architecture & Why Local stdio Matters](#architecture--why-local-stdio-matters)
2. [Step-by-Step Setup](#step-by-step-setup)
   - [Step 1: Locate or Open Configuration](#step-1-locate-or-open-configuration)
   - [Step 2: Add TallyMCP to `claude_desktop_config.json`](#step-2-add-tallymcp-to-claude_desktop_configjson)
   - [Step 3: Fully Quit & Restart Claude Desktop](#step-3-fully-quit--restart-claude-desktop)
   - [Step 4: Verify Tools in Chat](#step-4-verify-tools-in-chat)
3. [TallyPrime Prerequisites](#tallyprime-prerequisites)
4. [5-Step Self-Diagnostic Checklist](#5-step-self-diagnostic-checklist)
5. [Claude Desktop Log Analysis & Common Errors](#claude-desktop-log-analysis--common-errors)
6. [Interactive Verification Recipes](#interactive-verification-recipes)
7. [FAQ](#faq)

---

## Architecture & Why Local stdio Matters

```
┌────────────────────────────────────────────────────────┐
│                   YOUR LOCAL COMPUTER                  │
│                                                        │
│  ┌──────────────────┐           ┌───────────────────┐  │
│  │  Claude Desktop  │──stdio───▶│     tally-mcp     │  │
│  │ (Anthropic App)  │◀──stdio───│ (Rust MCP Server) │  │
│  └──────────────────┘           └─────────┬─────────┘  │
│                                           │            │
│                                      XML-over-HTTP     │
│                                      (127.0.0.1:9000)  │
│                                           │            │
│                                           ▼            │
│                                 ┌───────────────────┐  │
│                                 │    TallyPrime     │  │
│                                 │ (Company Books)   │  │
│                                 └───────────────────┘  │
└────────────────────────────────────────────────────────┘
```

> [!NOTE]
> **Why not "Add custom connector"?**
> When you click *Settings ▸ Connectors ▸ Add custom connector*, Claude attempts to connect from Anthropic's **cloud servers**. Remote cloud servers cannot reach `127.0.0.1` on your machine.
> Connecting via **Local MCP stdio** allows Claude Desktop to spawn the `tally-mcp` binary as a local child process on your computer. All queries stay strictly on loopback.

---

## Step-by-Step Setup

### Step 1: Locate or Open Configuration

#### Method A: Directly inside Claude Desktop (Easiest)
1. Open **Claude Desktop**.
2. Press `Ctrl + ,` (Windows) or `Cmd + ,` (macOS), or go to **File ▸ Settings**.
3. Click the **Developer** tab in the left navigation sidebar.
4. Click **Edit Config**.
5. Claude Desktop will open `claude_desktop_config.json` in your default text editor (Notepad, VS Code, etc.).

#### Method B: Direct File Path
- **Windows**: Press `Win + R`, paste `%APPDATA%\Claude\claude_desktop_config.json` and press Enter.  
  *(Default full path: `C:\Users\<YourUsername>\AppData\Roaming\Claude\claude_desktop_config.json`)*
- **macOS**: `~/Library/Application Support/Claude/claude_desktop_config.json`
- **Linux**: `~/.config/Claude/claude_desktop_config.json`

---

### Step 2: Add TallyMCP to `claude_desktop_config.json`

> [!NOTE]
> **Server Identifier:** We recommend `"tallymcp.pro"` as the standard server identifier across all clients. The legacy alias `"tally"` is also supported.

#### If your configuration file is empty:
```json
{
  "mcpServers": {
    "tallymcp.pro": {
      "command": "tally-mcp",
      "args": ["serve"]
    }
  }
}
```

#### If your file already has other settings:
> [!IMPORTANT]
> **Do not overwrite existing keys** such as `preferences` or other MCP servers. Insert `"tallymcp.pro"` inside the `"mcpServers"` object, ensuring valid JSON formatting (no missing or trailing commas).

```json
{
  "preferences": {
    "coworkWebSearchEnabled": true
  },
  "coworkUserFilesPath": "C:\\Users\\User\\Claude",
  "mcpServers": {
    "tallymcp.pro": {
      "command": "tally-mcp",
      "args": ["serve"]
    }
  }
}
```

#### Using Absolute Path (Recommended on Windows if `tally-mcp` is not in PATH):
If Windows fails to find `tally-mcp`, specify the absolute path to your binary. Remember to escape every backslash with a double backslash (`\\`):

```json
{
  "mcpServers": {
    "tallymcp.pro": {
      "command": "C:\\Users\\<YourUsername>\\AppData\\Local\\Programs\\TallyMCP\\tally-mcp.exe",
      "args": ["serve"]
    }
  }
}
```

---

### Step 3: Fully Quit & Restart Claude Desktop

> [!WARNING]
> Claude Desktop **does not reload config on window close**. Closing the window with `X` only minimizes Claude to the Windows notification tray!

To properly restart:
1. Right-click the Claude icon in your Windows Taskbar Notification Tray (bottom right near clock) and choose **Quit Claude**.
2. *Alternative via Command Prompt*:
   ```cmd
   taskkill /IM Claude.exe /F
   ```
3. Relaunch Claude Desktop from the Start Menu.

---

### Step 4: Verify Tools in Chat

1. Start a new chat in Claude Desktop.
2. In the chat prompt input box, locate the **Hammer / Tools icon** (🔨) in the bottom-right corner.
3. Click the hammer to view active tools. You should see:
   - `tally_status`
   - `tally_list_companies`
   - `tally_list_ledgers`
   - `tally_day_book`
   - `tally_trial_balance`
   - `tally_draft_voucher`
   - ...and other Tally tools.

---

## TallyPrime Prerequisites

Before Claude can query your accounts, verify TallyPrime is configured to allow XML/HTTP communication:

1. **Launch TallyPrime**.
2. Press **`F1` (Help) ▸ Settings ▸ Connectivity**.
3. Under **Client/Server configuration**, set:
   - **TallyPrime acts as**: `Both` (or `Server`)
   - **Enable ODBC**: `Yes`
   - **Port**: `9000` *(matches TallyMCP default)*
4. If you made any changes, press `Enter` to save and **restart TallyPrime**.
5. **Open a Company**: Ensure at least one company is actively opened in TallyPrime Gateway (e.g. *ABC Corp*). If Gateway shows *"Select Company"*, MCP read functions will return empty lists.

---

## 5-Step Self-Diagnostic Checklist

If the hammer icon does not appear or tools return errors, follow this 5-step checklist in order:

```
[ Step 1: Check Claude Logs ] ➔ [ Step 2: Test CLI Doctor ] ➔ [ Step 3: Check Tally Port ] ➔ [ Step 4: Companion UI Test ] ➔ [ Step 5: Cold Restart ]
```

### 1. Check Claude Desktop MCP Log
- **Windows**: Open `%APPDATA%\Claude\logs\mcp.log` and `%APPDATA%\Claude\logs\mcp-server-tally.log`.
- **macOS**: Check `~/Library/Logs/Claude/mcp.log`.
- Look for error messages at the end of the file (see [Log Analysis](#claude-desktop-log-analysis--common-errors) below).

### 2. Run CLI Diagnostic
Open Command Prompt or Terminal and run:
```bash
tally-mcp doctor
```
Expected output:
```text
✓ Tally server reachable at 127.0.0.1:9000
✓ Active company detected: ABC Corp Ltd
✓ Journal database initialized
✓ Security: Loopback only
```

### 3. Verify Tally Port in Browser
Open your browser and navigate to:
```text
http://127.0.0.1:9000
```
- **Success**: You should see XML output (such as `<RESPONSE>...</RESPONSE>` or standard Tally HTTP response).
- **Failure** (*Connection Refused*): TallyPrime is closed, port is not 9000, or Connectivity is set to `Client` instead of `Both`/`Server`.

### 4. Test MCP Tools via Companion UI
1. Launch the Companion UI:
   ```bash
   tally-mcp ui
   ```
2. Navigate to the **🧪 Test MCP** tab.
3. Click **Run Pre-Push Test Suite** or test `tally_list_companies` with 1 click.
4. Verify response latency and check that your company name appears.

### 5. Validate JSON Syntax
Paste your `claude_desktop_config.json` into an online JSON validator (or run in PowerShell: `Get-Content $env:APPDATA\Claude\claude_desktop_config.json | ConvertFrom-Json`). Ensure no red syntax errors exist.

---

## Claude Desktop Log Analysis & Common Errors

| Log Error / Symptom | Root Cause | Solution |
|---|---|---|
| `spawn tally-mcp ENOENT` | Executable cannot be found in system `PATH`. | Replace `"command": "tally-mcp"` with the full absolute path in `claude_desktop_config.json` (e.g., `C:\\Users\\...\\tally-mcp.exe`). |
| `SyntaxError: Unexpected token ... in JSON at position ...` | Syntax error in `claude_desktop_config.json` (e.g. trailing comma or missing quotation mark). | Open config, remove trailing commas before closing braces `}` or `]`, and ensure all quotes match. |
| `Connection refused (os error 10061 / 111)` | Tally is not running or not listening on port 9000. | Open TallyPrime, check **F1 ▸ Settings ▸ Connectivity**, ensure Port is 9000 and Server is enabled. |
| Hammer icon (🔨) missing completely | Claude Desktop did not reload config or encountered a fatal JSON parse error at boot. | Fully terminate Claude with `taskkill /IM Claude.exe /F`, verify JSON syntax, and restart. |
| Tools run but return `"No active company found"` | Tally is open but no company was selected. | Select and load your company in Gateway of Tally, or ask Claude: *"Set active company to ABC Corp"*. |
| Claude says `"Tally request timed out"` | Tally is busy running a heavy report or keep-alive connection hung. | TallyMCP includes built-in streaming de-chunking; ensure you are on TallyMCP v1.0.0+ and Tally is responsive. |

---

## Interactive Verification Recipes

Once connected, verify in Claude Desktop by sending these test prompts:

### 1. Connection Health Check
> *"Check the connection status of TallyPrime and list all accessible companies."*

**Expected Tool Invocation**: Claude invokes `tally_status` followed by `tally_list_companies`.  
**Expected Output**: Displays connection latency, Tally version, and loaded company names.

### 2. General Ledger Query
> *"List the first 10 accounts from our Sundry Debtors ledger."*

**Expected Tool Invocation**: Claude invokes `tally_list_ledgers` with `group: "Sundry Debtors"`.  
**Expected Output**: Clean list of customer account names and opening/closing balances.

### 3. Financial Summary
> *"Summarize the current Trial Balance for the active company."*

**Expected Tool Invocation**: Claude invokes `tally_trial_balance`.  
**Expected Output**: Debit and Credit totals, balance sheet group summaries.

---

## FAQ

**Q: Does Claude Desktop transmit my Tally accounting data to third parties?**  
A: No. TallyMCP runs locally on your PC. Claude only sees the specific text results of tools it explicitly invokes to answer your prompt.

**Q: Can Claude alter or post vouchers into Tally without my permission?**  
A: No. TallyMCP adheres to strict human-in-the-loop safety. All voucher drafting operations generate a staged draft and return a local, token-gated approval link. No voucher can ever be written into Tally without your physical click on the approval button.

**Q: Do I need Node.js or npm installed?**  
A: No! TallyMCP is compiled directly into a single native Rust binary. It has zero dependencies on Node.js, Python, or external runtimes.
