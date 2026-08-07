# remote-exec-mcp

A safe, auditable MCP server for giving AI agents controlled execution capabilities on remote systems without exposing an unrestricted shell.

## Core idea

> MCP describes intent. Policy authorizes it. Execution performs it. Transport only connects it. Audit records it.

The project deliberately separates these concerns so deployment, diagnostics, systemd, Docker, Kubernetes, and future workflows can reuse the same core without becoming coupled to SSH or to the MCP protocol.

## Architecture

```text
MCP Client
    |
    v
MCP Adapter                    JSON Schema + protocol conversion
    |
    v
RemoteExecRuntime              Composition/orchestration boundary
    |
    +--> Catalog
    +--> Policy -> Validation -> Planning
    +--> Audit
    +--> Concurrency limit
    |
    v
ExecutionPlan
    |
    +--> SshCommandExecutor
    +--> SftpFileTransfer
             |
             v
          SshSession
             |
             v
        Remote Linux host
```

The important invariants are:

1. **Task != Command** — a task expresses operator-approved intent; a planner resolves it into an execution plan.
2. **Transport != Executor** — SSH establishes a session; command execution and file transfer are separate capabilities over that session.
3. **MCP != Policy** — MCP tools only carry intent. Authorization and validation remain deterministic application logic.
4. **No raw shell tool** — `run_task` accepts a task name and typed parameters, never a caller-provided shell command.

## Implemented MCP tools

- `list_targets` — list configured target identifiers without network access.
- `check_target` — verify SSH reachability, host identity, and authentication.
- `list_tasks` — expose only tasks allowed for the selected target.
- `run_task` — authorize, validate, plan, and execute a declarative task.
- `upload_file` — SFTP upload with local/remote root, size, timeout, and overwrite controls.
- `download_file` — SFTP download with the same bounded policy model.

Domain failures are returned as structured tool errors with stable codes such as `unknown_target`, `task_not_allowed`, `invalid_parameter`, `connection_timeout`, and `transfer_path_denied`.

## Build

Requires a current stable Rust toolchain.

```bash
cargo build --release
```

The binary is produced at:

```text
target/release/remote-exec-mcp
```

## Configuration

Start from [`config/example.yaml`](config/example.yaml). A minimal configuration contains runtime security settings, named SSH targets, target policy, and declarative tasks.

```yaml
runtime:
  max_concurrency: 4
  transfer_timeout_seconds: 60
  audit_path: .remote-exec-mcp/audit.jsonl
  allowed_local_upload_roots: [/tmp/remote-exec-mcp-staging]
  allowed_local_download_roots: [/tmp/remote-exec-mcp-staging]

targets:
  test:
    transport:
      type: ssh
      host: 10.0.0.10
      port: 22
      user: deploy
      auth:
        type: key
        secret_ref: env:REMOTE_EXEC_SSH_KEY
      host_key_policy: strict
    policy:
      allowed_tasks: [service-status]
      allowed_upload_roots: [/opt/apps]
      allowed_download_roots: [/var/log/apps]
      max_transfer_bytes: 104857600

tasks:
  service-status:
    description: Check a systemd service status
    parameters:
      service:
        type: string
        pattern: "^[a-zA-Z0-9_.@-]+$"
        required: true
    execution:
      type: command
      program: systemctl
      args: ["status", "{{service}}", "--no-pager"]
    timeout_seconds: 15
```

`REMOTE_EXEC_SSH_KEY` contains the private-key contents. Do not commit private keys or put them in ordinary task/config records. `host_key_policy: strict` is the default; `accept-new` is explicit and still rejects changed host keys.

File transfer is fail-closed: local roots must be configured in `runtime`, remote roots must be configured in target policy, and `max_transfer_bytes` must be set for that target. Configured local directories must already exist so they can be canonicalized safely.

## Run over stdio

The server uses the official Rust MCP SDK and stdio transport. Standard output is reserved for MCP JSON-RPC; diagnostics are written to standard error.

```bash
export REMOTE_EXEC_SSH_KEY="$(cat ~/.ssh/id_ed25519)"
target/release/remote-exec-mcp --config /absolute/path/to/config.yaml
```

You can also set `REMOTE_EXEC_MCP_CONFIG` and omit `--config`.

A generic MCP client entry looks like:

```json
{
  "mcpServers": {
    "remote-exec": {
      "command": "/absolute/path/to/remote-exec-mcp",
      "args": ["--config", "/absolute/path/to/config.yaml"]
    }
  }
}
```

Pass secret environment variables to the process through your operating system or a secret manager rather than embedding secret values in this client configuration.

## Execution safety

A request such as:

```json
{
  "target": "test",
  "task": "service-status",
  "parameters": {"service": "demo.service"}
}
```

flows through:

```text
Target policy
  -> typed parameter validation
  -> TaskPlanner
  -> ExecutionPlan(program + argv)
  -> SSH command serialization
  -> bounded stdout/stderr + timeout
```

SSH `exec` ultimately carries a command string, so the executor alone serializes the already-approved `program + argv` plan with POSIX quoting. Callers cannot submit raw shell text.

## File-transfer safety

SFTP operations enforce both sides of the trust boundary:

- local paths are canonicalized and must remain inside configured runtime roots;
- remote paths must be absolute and inside target upload/download roots;
- `..`, NUL, `/` as an unrestricted root, and remote symlink escapes are rejected;
- transfer size is checked before and during streaming;
- overwrite is disabled by default;
- temporary files are used before final rename;
- transfer duration is bounded.

## Audit and concurrency

Network-backed operations are globally limited by `runtime.max_concurrency`. Mutating/network operations write JSONL audit records containing request ID, timestamp, target, operation/task, policy decision where known, outcome, exit code, and duration where applicable. Secret values and task parameter values are not written to audit records.

Audit initialization is fail-closed: if the configured audit path cannot be opened, the runtime does not start.

## Non-goals for the v0.1 core

- general-purpose remote terminal;
- unrestricted root/sudo execution;
- arbitrary environment injection;
- configuration-management replacement;
- embedding credentials in project configuration;
- pretending systemd/Docker/Kubernetes are transports. Higher-level workflows should reuse this safe core or their native APIs.

## Development

CI must pass all three gates:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

See [Design](docs/DESIGN.md), [Security](docs/SECURITY.md), and [Roadmap](docs/ROADMAP.md).
