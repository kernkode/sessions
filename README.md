# Sessions

A desktop console to run and watch several CLI coding agents — **Claude Code**,
**Codex**, **pi** and a plain terminal — in one window, with live token metrics
(tok/s, context window, cost) and TOML configuration under `~/.sessions`.

Models, providers and credentials are managed by each CLI with its own config;
this app only launches the process, keeps it alive and reads its telemetry.

Built with Tauri 2 (Rust) + React. The binary is a few MB and the app idles at
~30 MB of RAM instead of the hundreds an Electron-based alternative would use.

---

## Requirements

| Tool | What for |
|---|---|
| Node 20+ and npm | frontend (Vite + React) |
| Stable Rust 1.77+ | Tauri backend |
| WebView2 (Windows) · WebKitGTK (Linux) | window engine |
| The CLIs you want to use | `claude`, `codex`, `pi`… on the PATH (or auto-installed, see below) |

If Node or a CLI is missing, the app tries to install it on first launch
(`winget` for Node, `npm i -g` for the agents that declare an `install` argv).

## Getting started

```bash
npm install
npm run app:dev        # development with hot reload
npm run app:build      # production installer / binary
```

Other useful commands:

```bash
npm run build          # type-check and build the frontend
npm run rs:test        # backend tests (70 unit + 4 e2e)
node scripts/ui-e2e.mjs # frontend e2e assertions over the running app (CDP)
npm run icons          # regenerate the icon set
npm run dev:free       # free port 5273 if a previous Vite is still alive
```

---

## Layout

Code (identifiers, comments, tests) and all user-facing text are in English.

```
sessions/
├─ src/                     frontend (React + TypeScript)
│  ├─ term/pool.ts          xterm terminal pool (outside React)
│  ├─ state/store.ts        global state (zustand)
│  ├─ components/           title bar, sidebar, metrics, dialogs, palette
│  └─ lib/                  IPC, types, formatting
├─ src-tauri/
│  ├─ src/pty/              PTY, ring buffer and output pump
│  ├─ src/metrics/          per-agent token readers (claude, codex, pi)
│  ├─ src/config/           TOML loading and validation
│  ├─ src/git.rs            opencode-style checkpoints (undo/redo)
│  ├─ src/launcher.rs       agent → command, arguments and environment
│  ├─ src/store.rs          projects, sessions and scrollback
│  ├─ src/commands.rs       API exposed to the frontend
│  └─ assets/*.default.toml templates copied to ~/.sessions
└─ scripts/                 icons, UI probes and the frontend e2e
```

---

## `~/.sessions`

Created on first launch and **never overwritten** afterwards:

```
~/.sessions/
├─ config.toml           appearance, terminal, performance, resume, shortcuts
├─ agents.toml           CLIs the app can launch
├─ state/projects.json   registered projects and sessions
├─ scrollback/           per-session history
└─ logs/
```

`SESSIONS_HOME=/other/path` uses a different location (handy for tests or a
portable setup).

After editing any `.toml`, apply changes with **Ctrl+Shift+R** or the *Reload*
button in Settings. A broken file never blocks startup: the app reports the
problem and falls back to factory defaults.

### Providers, models and keys

Not managed here: each CLI configures them where it always has. `agents.toml`
only describes **how to launch the process**: executable, arguments and fixed
environment variables. Point a CLI at your own gateway via its config or
`[agent.env]`.

### Agents

```toml
[[agent]]
id = "claude"
name = "Claude Code"
command = "claude"
command_windows = "claude.cmd"      # npm shims on Windows are .cmd
resume_args = ["--resume", "{session_id}"]
continue_args = ["--continue"]
install = ["npm", "install", "-g", "@anthropic-ai/claude-code"]
metrics = "claude-jsonl"            # where the tokens come from
```

`metrics` accepts `claude-jsonl`, `codex-rollout`, `pi-jsonl` or `none`. Adding a
new CLI is just another `[[agent]]` block; agents without telemetry still show
activity and output throughput. Factory agents: Claude Code, Codex, pi and a
normal terminal for git, builds and one-off commands. `enabled = false` keeps a
block in the file without offering it in the UI.

---

## Metrics: where they come from

They are not estimated; they are read from each agent's own log.

| Agent | Source | Data |
|---|---|---|
| Claude Code | `~/.claude/projects/<cwd>/<id>.jsonl` | per-turn `message.usage` (input, output, cache, thinking), model, effort |
| Codex | `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` | `token_count` events, `model_context_window`, turn model |
| pi | `~/.pi/agent/sessions/<cwd>/<ts>_<id>.jsonl` | per-message usage, model, thinking level |
| terminal | — | PTY activity and bytes/s |

Files are read incrementally by offset, never whole, and polling adapts: fast
while the session produces output, slow at rest.

* **tok/s** — output tokens of the last turn over its duration, smoothed with a
  moving average; the peak is shown too.
* **Context window** and **model** — whatever the agent reports; hidden otherwise.
* **Cost** — whatever the agent reports.
* **PTY bytes/s** — an activity signal valid even for agents without telemetry.

"Working" is detected from the agent's busy hints (spinner/text) with a fallback
on recent JSONL activity, so a CLI update that changes the spinner does not
silently break it.

### Auto-resume and auto-relaunch

Processes do not survive a restart, so on launch the app relaunches saved
sessions on its own: agents that support it resume the conversation
(`claude --resume <id>`, `codex resume <id>`); the rest start fresh in the same
directory.

```toml
[app]
restore_sessions = true     # master switch
auto_resume = "active"      # active (last used) | all | none
auto_relaunch = true        # relaunch a session whose process ended on its own
```

`active` is the default on purpose: each agent is a Node process that can hover
around 700 MB, so `all` only makes sense with few sessions. With `none` (or
`restore_sessions = false`) sessions appear as *Ended* and reopen manually.

If a relaunch fails — the CLI is gone from PATH, the directory moved — the record
is **not** lost: the app reports the reason and the session stays *Ended*, ready
to retry.

### Sessions that are not resumed

An ended session shows its last real screen, rebuilt by replaying the bytes in an
off-screen emulator of the original size and serialising its buffer (raw replay
would look broken: agents draw with absolute positioning and sequences like
`CSI ?25l`). A notice explains the transcript does not accept input and offers
**Relaunch** (fresh process) or **Resume** (`--resume` with the CLI session id).

---

## Git checkpoints (undo/redo)

Each session's workspace is snapshotted as `sessions-checkpoint:` commits; the
header shows the branch with ↩/↪ buttons to move between checkpoints
(`reset --hard` + `clean -fd`, with a confirmation when the tree is dirty). The
`.sessions/` directory is excluded from checkpoints via `.git/info/exclude`. A
checkpoint is also taken automatically before relaunching a session.

---

## Performance

The output pipeline is designed for agents that write a lot, very often:

```
[reader thread/session] read(64K) ─► ring buffer (rehydration)
        └─ bounded channel ─► [1 pump thread] coalesces per session and sends
                              every flush_interval_ms or at max_chunk_bytes
                              ─► binary channel ─► xterm.write()
[1 supervisor thread] try_wait() every 300 ms ─► exit event + handle close
```

* **Coalescing**: thousands of small writes become ~80 messages/s per session.
* **Binary transport**: output travels as bytes over a Tauri `Channel`, no JSON
  round-trip.
* **One pump and one supervisor thread** for all sessions, plus one reader per
  PTY; thread count does not grow with load.
* **Bounded queue**: if the UI stalls, readers slow down instead of eating memory.
* **React off the hot path**: output goes straight to xterm; sidebar cards
  subscribe only to their own metrics.
* **Terminals are never re-mounted**: each session keeps its xterm instance and
  is hidden with CSS; ended ones are released by LRU (`max_live_terminals`) and
  rehydrated from the ring buffer.
* **WebGL renderer** with DOM fallback.
* **Bundled fonts** (Inter variable + JetBrains Mono): identical terminal metrics
  on any machine; xterm re-measures once the fonts finish loading.

All of this is tuned in `[performance]` and `[terminal]` of `config.toml`; values
are clamped on load so an unlucky edit cannot degrade the app.

### ConPTY (Windows) notes

1. ConPTY emits `ESC[6n` at startup and produces **no output until answered**.
   The emulator itself answers, which is why a live session's terminal is never
   released and the answer is regenerated on rehydration.
2. The PTY reader **never sees EOF** when the child exits; the supervisor detects
   the end with `try_wait` and closes the handles, unblocking the reader.

Hence history and the live stream must never overlap: attaching is an output-
thread operation that takes the snapshot and the byte mark together.

---

## Shortcuts

| Shortcut | Action |
|---|---|
| `Ctrl+Shift+T` | new session |
| `Ctrl+Shift+W` | close the active session |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` | next / previous session |
| `Ctrl+Shift+B` | sidebar |
| `Ctrl+Shift+M` | metrics bar |
| `Ctrl+Shift+F` | search in the terminal (live as you type) |
| `Ctrl+Shift+K` | clear the terminal |
| `Ctrl+Shift+R` | reload configuration |
| `Ctrl+,` | settings |
| `Ctrl+K` | command palette |

Shift combos are used on purpose: `Ctrl+C`, `Ctrl+R`, `Ctrl+W` and `Ctrl+F`
belong to the agent and reach the PTY untouched.

---

## Testing

```bash
npm run rs:test      # 70 unit + 4 end-to-end (Rust)
npm run build        # types + frontend
node scripts/ui-e2e.mjs   # frontend assertions over the running app
```

They cover factory TOML parsing, per-agent command/environment building, the
ring buffer, the PTY lifecycle (ConPTY DSR dialogue and EOF-less exit detection),
output coalescing, the metrics readers, git checkpoints, persistence and hot
reload. To inspect or drive the real UI inside WebView2:

```bash
WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222 npm run app:dev
node scripts/ui-probe.mjs "document.body.innerText"
node scripts/ui-shot.mjs capture.png
```

---

## Common problems

**`Error: Port 5273 is already in use`** when running `npm run app:dev`.
A previous dev server is still alive. The port is fixed on purpose
(`strictPort`): the window points at that exact URL. Free it with:

```bash
npm run dev:free      # kills only the node process holding 5273
```

**`error LNK2001: unresolved external symbol anon.…`** while compiling.
Corrupted incremental artifacts, usually from an interrupted link. Fix:

```bash
rm -rf src-tauri/target/debug/incremental    # or cargo clean -p sessions
```

**An agent shows as "not installed"** in the new-session dialog. The app looks
the executable up on `PATH` honouring `PATHEXT` on Windows. Adjust `command` /
`command_windows` in `agents.toml` (npm-installed CLIs are `.cmd`) or give an
absolute path.

**A session is blank on startup.** Nobody answered ConPTY's `ESC[6n]`. This
happens if a live session's terminal is released; the app never does, but if you
touch `max_live_terminals` keep it at 2 or more.

---

## Security

* The app stores no API keys: each CLI uses its own. A token placed in
  `[agent.env]` lives in `agents.toml`, in your user folder.
* `agents.toml` **runs processes and defines their environment**: treat it as
  code and never load third-party files unreviewed.
* Auto-install only runs `winget`/`npm i -g` for agents that declare an
  `install` argv and whose executable is missing; failures never block startup.
* The window uses a restrictive CSP and only the Tauri permissions the app needs
  (folder dialog, opening paths and window control).
