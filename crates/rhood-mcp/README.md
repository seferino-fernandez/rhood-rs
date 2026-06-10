# rhood-mcp

MCP server exposing Robinhood brokerage operations as tools for LLM clients.

Built on [rmcp](https://github.com/modelcontextprotocol/rust-sdk) and [rhood-core](../rhood-core/).

## Transports

- `stdio` for Claude Desktop, Claude Code, and other local MCP clients
- `http` (streamable HTTP with session management) for remote deployments

## Authentication Modes

| Mode    | Flag                | Description                                         |
| ------- | ------------------- | --------------------------------------------------- |
| `token` | `--auth-mode token` | Static bearer token (default)                       |
| `oauth` | `--auth-mode oauth` | OAuth 2.1 with PKCE and dynamic client registration |
| `none`  | `--auth-mode none`  | No auth (loopback-only, local development)          |

## Quick Start

### stdio (Claude Desktop / Claude Code)

```bash
rhood-mcp --transport stdio
```

### HTTP with OAuth

```bash
rhood-mcp --transport http --auth-mode oauth --port 8080
```

### HTTP with static token

```bash
rhood-mcp --transport http --auth-mode token --token my-secret
```

## Adding to Your Claude Setup

### Claude Code (CLI)

The `claude mcp add` command registers the server in your Claude Code configuration.

Local scope (default; private to you, current project only):

```bash
claude mcp add rhood --transport stdio -- rhood-mcp --transport stdio
```

User scope (available across all your projects):

```bash
claude mcp add rhood --transport stdio --scope user -- rhood-mcp --transport stdio
```

With Robinhood credentials:

```bash
claude mcp add rhood --transport stdio \
  --env RHOOD_USERNAME=user \
  --env RHOOD_PASSWORD=pass \
  -- rhood-mcp --transport stdio
```

HTTP transport (connect to a running server):

```bash
rhood-mcp --transport http --port 8080 &
claude mcp add rhood --transport http http://localhost:8080/mcp
```

Via JSON configuration:

```bash
claude mcp add-json rhood '{
  "type": "stdio",
  "command": "rhood-mcp",
  "args": ["--transport", "stdio"],
  "env": {
    "RHOOD_USERNAME": "user",
    "RHOOD_PASSWORD": "pass"
  }
}'
```

Verify it's configured:

```bash
claude mcp list
claude mcp get rhood
```

### Claude Desktop

Add to your `claude_desktop_config.json`:

- macOS: `~/Library/Application Support/Claude/claude_desktop_config.json`
- Windows: `%APPDATA%\Claude\claude_desktop_config.json`

```json
{
    "mcpServers": {
        "rhood": {
            "command": "rhood-mcp",
            "args": ["--transport", "stdio"],
            "env": {
                "RHOOD_USERNAME": "user",
                "RHOOD_PASSWORD": "pass"
            }
        }
    }
}
```

> If `rhood-mcp` is not on your `PATH`, use the full path to the binary (e.g., `"/usr/local/bin/rhood-mcp"` or `"C:\\path\\to\\rhood-mcp.exe"`). On Windows, wrap the command with `cmd /c`: `"command": "cmd", "args": ["/c", "rhood-mcp", "--transport", "stdio"]`.

### Shared project config (`.mcp.json`)

Create a `.mcp.json` file at your project root. This is checked into version control so all team members share the same MCP configuration:

```json
{
    "mcpServers": {
        "rhood": {
            "command": "rhood-mcp",
            "args": ["--transport", "stdio"],
            "env": {
                "RHOOD_USERNAME": "${RHOOD_USERNAME}",
                "RHOOD_PASSWORD": "${RHOOD_PASSWORD}"
            }
        }
    }
}
```

Environment variables use `${VAR}` expansion, so each team member sets their own credentials in their shell environment.

### Cursor / Windsurf / other MCP clients

Any client that supports the MCP stdio transport can connect using:

```bash
rhood-mcp --transport stdio
```

For HTTP-based clients, start the server and point your client at the endpoint (`http://localhost:8080/mcp`):

```bash
rhood-mcp --transport http --port 8080
```

## Tools

Tools are grouped by domain. The complete authoritative list is what `tools/list` returns at runtime — what follows is an overview of the surface.

- **Stocks** — quotes, history, fundamentals, plus stock-order history (open and full).
- **Options** — chain metadata, contract quotes (bid/ask, Greeks, volume), positions, and order history.
- **Futures** — contract lookup, real-time quotes, order history, and futures-account discovery.
- **Indices** — index quotes (SPX, NDX, VIX, RUT, XSP), index option chains, and contract search.
- **Account** — positions (current and including closed), portfolio summary, account profile and unified summary, account documents (statements, tax forms).
- **Income** — dividend history (and total), interest/sweep payments, and unified transfers (ACH, wire, debit card).
- **Research** — earnings, ratings, news, splits, and category tags.
- **Recurring investments** — list, create, update, cancel, and next-investment-date lookup.
- **Watchlists** — list, view items, add/remove symbols.
- **User** — authenticated user profile and day-trade / pattern-day-trader status.
- **Market** — exchange listings, trading hours (any date or today), and daily movers.
- **Orders** — staging (`place_stock_order`, `place_option_order`), execution (`confirm_order`), and cancellation (`cancel_order`, `cancel_option_order`).

For the live list, call MCP `tools/list` against the running server.

### Read-only vs. write

Order placement, confirmation, and cancellation are write operations. The server starts in read-only mode by default — write tools are hidden from `tools/list` and rejected on call. Pass `--read-write` or set `read_only = false` in config to enable them.

Order placement uses two-step confirmation: `place_*` stages the order and returns a `pending_order_id`; `confirm_order` then submits it.

## CLI Reference

```shell
rhood-mcp [OPTIONS]

Options:
  --transport <stdio|http>     Transport protocol [default: stdio]
  --host <HOST>                Bind address [default: 127.0.0.1]
  --port <PORT>                Bind port [default: 8080]
  --auth-mode <token|oauth|none>  Authentication mode [default: token]
  --token <TOKEN>              Static bearer token for auth_mode=token
  --base-url <URL>             External base URL for OAuth metadata
  --oauth-pin <PIN>            PIN for the OAuth consent screen
  --read-write                 Enable order placement and cancellation
  -V, --version                Print version
  -h, --help                   Print help
```

## Configuration

All settings can be specified via config file, environment variables, or CLI flags. Precedence: CLI > env > config file.

Config file location: `~/.config/rhood/config.toml`

See [config.toml.example](../../config.toml.example) for all available fields.

### Key environment variables

| Variable                            | Description                          |
| ----------------------------------- | ------------------------------------ |
| `RHOOD_MCP_HOST`                    | Bind address                         |
| `RHOOD_MCP_PORT`                    | Bind port                            |
| `RHOOD_MCP_TOKEN`                   | Static bearer token                  |
| `RHOOD_MCP_AUTH_MODE`               | Authentication mode                  |
| `RHOOD_MCP_BASE_URL`                | External base URL for OAuth          |
| `RHOOD_MCP_OAUTH_PIN`               | OAuth consent screen PIN             |
| `RHOOD_MCP_OAUTH_TOKEN_EXPIRY_SECS` | OAuth token lifetime (default: 3600) |
| `RHOOD_READ_ONLY`                   | Disable write operations             |

## Read-Only Mode

The server starts in read-only mode by default. Write tools (`place_*_order`, `confirm_order`, `cancel_*_order`) are hidden from tool listings and blocked from execution.

Pass `--read-write` or set `read_only = false` in config to enable order operations.

## OAuth Flow

When running with `--auth-mode oauth`:

1. Client discovers metadata via `/.well-known/oauth-authorization-server`
2. Client registers dynamically via `POST /oauth/register`
3. User is redirected to consent page at `/oauth/authorize`
4. After approval, client exchanges authorization code for tokens at `/oauth/token`
5. Tokens are sent as `Bearer` in the `Authorization` header to `/mcp`

All authorization codes use PKCE (S256). Tokens, codes, and CSRF nonces are swept periodically by a background task.
