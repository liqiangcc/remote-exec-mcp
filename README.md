# remote-exec-mcp

A safe, auditable MCP foundation for giving AI agents controlled execution capabilities on remote systems without exposing an unrestricted shell.

## Core idea

> MCP describes intent. Policy authorizes it. Execution performs it. Transport only connects it. Audit records it.

The project deliberately separates these concerns so deployment, diagnostics, systemd, Docker, Kubernetes, and future adapters can reuse the same core without becoming coupled to SSH or to the MCP protocol.

## Architectural boundaries

```text
MCP Adapter                       Protocol concern
    |
Application Use Cases             Orchestration concern
    |
Domain                            Intent + execution plan
    |
Policy --------------------+      Authorization concern
    |                      |
Execution Ports            Audit  Execution / cross-cutting concern
    |
Infrastructure Adapters           Implementation concern
    |-- SSH Transport
    |-- Local Transport (later)
    |-- Command Executor
    |-- File Transfer
    |-- Secret Provider
    |-- Audit Sink

Capability adapters (later):
    |-- systemd
    |-- Docker
    |-- Kubernetes
    |-- deployment workflows
```

Three invariants are intentionally enforced:

1. **Task != Command** — a task expresses operator-approved intent; a planner resolves it into an execution plan.
2. **Transport != Executor** — SSH establishes a session; command execution and file transfer are separate capabilities over that session.
3. **Docker/Kubernetes != Transport** — they are higher-level capability adapters and may use APIs, local execution, or SSH-backed execution.

## MVP capabilities

- named targets;
- SSH transport with strict host verification;
- predefined task catalog;
- typed task parameters;
- policy checks before execution;
- bounded command output and execution time;
- upload/download inside configured roots;
- secret references instead of embedded credentials;
- structured audit events.

No unrestricted shell tool is exposed by default.

## Example configuration

```yaml
targets:
  test:
    transport:
      type: ssh
      host: 10.0.0.10
      port: 22
      user: deploy
      auth:
        type: key
        secret_ref: ssh-key:test
      host_key_policy: strict
    policy:
      allowed_tasks: [service-status, restart-service]
      allowed_upload_roots: [/opt/apps]
      allowed_download_roots: [/var/log/apps]

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

The task is the public intent. `execution` is operator-owned implementation metadata used by the planner; the model does not submit raw command strings.

## Proposed MCP tools

- `list_targets`
- `check_target`
- `list_tasks`
- `run_task`
- `upload_file`
- `download_file`

The MCP layer never owns SSH logic, authorization rules, secret storage, or command construction.

## Non-goals for MVP

- general-purpose remote terminal;
- unrestricted root/sudo execution;
- Kubernetes-specific deployment server;
- configuration-management replacement;
- secret-manager implementation.

See [Design](docs/DESIGN.md), [Security](docs/SECURITY.md), and [Roadmap](docs/ROADMAP.md).
