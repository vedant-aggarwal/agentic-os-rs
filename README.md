# Agentic OS

A personal operating system for a life run on three pillars: tasks with subtasks, habits with
"never miss twice" streaks, XP and levels, mood, journal, goals and alarms. One Rust server is
the source of truth; Dioxus clients (web, desktop, Android, iOS) and AI agents all use the same
tools.

## Layout

| Path | What |
|---|---|
| `src/` | The Dioxus app (UI). `src/server/` is the server-build entry point. |
| `crates/aos-core` | Domain types, rules (streaks, XP, rollups) and the SQLite store. |
| `crates/aos-tools` | The tool registry: each tool defined once, used by HTTP, CLI and MCP. |
| `crates/aos-server` | Axum API, MCP over HTTP, sync, device pairing, live events. |
| `crates/aos-cli` | The `aos` command and the MCP stdio server. |

## Server

Data lives in `%USERPROFILE%\.aos\aos.db` (override with `AOS_DB` or `--db`). Build output in
this repo goes to D: (`CARGO_TARGET_DIR`).

### Run

```powershell
# API only, on 127.0.0.1:8787
cargo run -p aos-server -- --addr 127.0.0.1:8787

# API + the web app (server-side rendering) in one process
dx build --platform web            # fills the `public/` folder with the web client
cargo run --features server        # listens on AOS_ADDR, or IP/PORT as `dx serve` sets them
```

Ctrl-C shuts down cleanly: open event streams close and background work gets up to 10 seconds.
Set `RUST_LOG=debug` for more logging.

### Pair a device

```powershell
aos pair new                       # prints a 6-digit code, valid 10 minutes
```

The device sends the code once and keeps the token it gets back:

```http
POST /api/pair/claim
{"code": "123456", "name": "iPhone", "platform": "ios"}
-> {"device": {...}, "token": "aos_..."}
```

Every other request carries `Authorization: Bearer aos_...`. A failed request never logs a
device out; only `aos pair revoke <id>` (or `POST /api/devices/{id}/revoke`) does. For a local
agent, skip the code: `aos pair token --name grok-bot` prints a token directly.

### Call tools from the CLI

The CLI reads and writes the database directly, so it works with or without the server running.
The server picks up CLI writes and pushes them to devices.

```powershell
aos                                          # today's overview
aos task add Write the report --area work --due tomorrow --xp-weight 2
aos task add Outline --parent 7k2m           # a subtask (ids accept unique prefixes)
aos task done 7k2m --evidence "commit abc123"
aos task skip 9d1q too big --genuine true
aos habit log journal floor
aos mood log 0.4 --energy 4 --labels calm,focused
aos journal add "Exact words, kept as written."
aos alarm set 06:00 Gym
aos goal earn laptop 25000                   # rupees; stored as paise
aos today --json                             # compact JSON for scripts and agents
aos tools                                    # every tool
aos describe task add                        # one tool's options
```

### Call tools over HTTP

```http
GET  /api/tools                     every tool with its JSON Schema
POST /api/tools/task_add            body: {"title": "Buy milk", "due": "today"}
GET  /api/sync/changes?since=0      rows changed after a version (incremental sync)
POST /api/sync/push                 {"ops": [{"op_id": "...", "tool": "task_add", "args": {...}}]}
GET  /api/events                    server-sent events (hello, change, resync)
GET  /api/ws                        the same events over a WebSocket
```

`/api/sync/push` applies a client's offline outbox; each op is remembered by `op_id`, so
resending after a dropped connection is safe.

### Call tools over MCP

Stdio (Claude Code, Codex, any MCP client on this PC):

```json
{ "mcpServers": { "agentic-os": { "command": "aos", "args": ["mcp"] } } }
```

Streamable HTTP (Grok Bot, or anything that can't spawn a process): `POST /mcp` with
`Authorization: Bearer <token>`. Tool names use underscores (`task_add`); `task.add` is accepted
too.

### Personal seed

A new database gets three generic pillars and nine areas. Your own pillars, areas, projects,
habits and goals go in `%USERPROFILE%\.aos\seed.local.toml` (never committed). It is applied to a
new database automatically, or any time with `aos seed`. See
[`crates/aos-core/seed.example.toml`](crates/aos-core/seed.example.toml).

## iPhone builds

`.github/workflows/ios.yml` builds the app on a GitHub macOS runner with `dx bundle --platform ios`, then
`ios/package.sh` finishes the bundle (Info.plist, app icon, App Store signature) into an IPA and uploads it to
TestFlight. `ios/signing.py` makes a short-lived distribution certificate and profile through the
App Store Connect API for each run and removes them afterwards. The build number is the app's highest
build number in App Store Connect plus one.

To use it with your own Apple account, set these repo secrets: `APPLE_TEAM_ID`, `ASC_KEY_ID`, `ASC_ISSUER_ID`,
`ASC_KEY_P8` (the contents of an App Manager API key) and `BUNDLE_ID` (an app you created in App Store Connect).

## License

MIT
