# Security Model

## Threat model

The MCP server may receive incorrect, overly broad, or adversarial instructions from an AI client. Connectivity to a host must never imply unrestricted authority on that host.

## Principles

1. Deny by default.
2. Do not expose a general shell in the default profile.
3. Validate every model-controlled task parameter before planning.
4. Authorize target/task intent before execution.
5. Keep secrets behind opaque references/providers.
6. Enforce least privilege in both MCP policy and remote OS accounts.
7. Bound time, output, concurrency, and file sizes.
8. Verify target identity.
9. Audit network-backed and mutating operations.
10. Keep security policy independent from transport/executor implementations.

## Authorization pipeline

```text
MCP Request
   -> MCP schema decoding
   -> target/task lookup
   -> target policy evaluation
   -> typed parameter validation
   -> execution planning
   -> executor
```

A denied request must never be transformed into an executable command.

## MCP boundary

The shipped MCP adapter exposes six tools: `list_targets`, `check_target`, `list_tasks`, `run_task`, `upload_file`, and `download_file`.

The adapter owns protocol schemas and response conversion only. It does not construct shell commands, resolve credentials, decide policy, or reach into SSH directly. Domain failures are exposed as structured tool errors with stable machine-readable error codes.

`run_task` has no raw-command field. The caller provides only a target, an operator-defined task name, and typed task parameters.

## Secrets

Configuration stores references:

```yaml
auth:
  type: key
  secret_ref: env:REMOTE_EXEC_SSH_KEY
```

A `SecretProvider` resolves the reference at runtime. The environment provider reads explicitly prefixed `env:` references. The referenced variable contains private-key contents, not a path.

Resolved secret values use a redacted debug representation and are zeroized when dropped. MCP responses, task definitions, and audit events never contain private keys, passwords, tokens, or secret environment values.

## SSH host verification

Strict known-host verification is the default. The SSH client checks the server public key against either the configured `known_hosts_path` or the current user's standard `~/.ssh/known_hosts` file.

Development-only `accept-new` is explicit: an unknown host key may be recorded, but a changed key is still rejected. Disabling verification entirely is not supported by the default transport.

Connection and public-key authentication are bounded by the configured SSH connection timeout. A timeout is surfaced separately from authentication failure and host-key rejection.

## Command safety

The model selects a named task and typed parameters. Operator-owned planning metadata resolves that request into a `program + argv` execution plan.

Prefer:

```text
program = systemctl
argv = [restart, validated_service]
```

SSH `exec` transports a command string and the server passes it to a shell. The SSH command executor is therefore the only layer allowed to serialize the structured `program + argv` plan: every token is POSIX single-quoted, embedded single quotes are escaped, and NUL bytes are rejected. The executor never accepts model-provided raw shell text.

This quoting step is a transport serialization detail, not authorization. Policy and validation must already have completed before the command reaches the executor.

Standard output and standard error are captured independently with bounded byte limits. Truncation is reported in `ExecutionResult` instead of allowing unbounded memory growth. Command execution is time-bounded; timeout handling performs a best-effort SSH channel close. Strong remote-process termination after an adversarial remote program ignores channel closure is intentionally not claimed by the v0.1 core.

An explicitly shell-backed task, if ever added, must be a separate higher-risk capability with stronger policy controls.

## Filesystem safety

File transfer uses SFTP over an authenticated SSH session. Policy owns the allowed remote upload/download roots and maximum byte count; the SFTP adapter enforces those constraints. The runtime independently owns the local filesystem boundary.

Remote paths are treated as POSIX paths independent of the MCP host operating system. Before transfer the adapter:

- requires absolute remote paths;
- rejects `..` traversal and NUL bytes;
- rejects `/` as a configured root;
- canonicalizes configured roots on the remote server;
- canonicalizes an existing remote source/destination, or the parent directory for a new upload;
- re-checks the canonical path against canonical roots, preventing symlink escape;
- enforces `max_transfer_bytes` from metadata when available and again while streaming;
- applies bounded transfer timeouts;
- avoids overwrite by default.

The MCP runtime separately canonicalizes local upload sources or local download parents and requires them to remain inside explicitly configured `runtime.allowed_local_upload_roots` / `runtime.allowed_local_download_roots`. Empty local-root lists disable the corresponding file capability. Existing local symlink destinations are rejected by the SFTP adapter.

Uploads first write to a uniquely named temporary file in the authorized destination directory and rename it into place only after byte-limit and sync checks succeed. Downloads similarly use a local temporary file so ordinary failures do not expose a partially written destination.

Overwrite is explicit. Current cross-platform replacement may remove an existing destination immediately before rename, so the core does not claim crash-proof atomic replacement on every SFTP server/filesystem. Orphan cleanup after process-level interruption remains a future hardening option.

## Concurrency and denial-of-service bounds

All network-backed MCP operations acquire a permit from a global semaphore configured by `runtime.max_concurrency`. A zero value is rejected at startup. Command output, task duration, transfer duration, and transfer bytes are bounded separately.

This is a process-level concurrency boundary, not a distributed rate limiter. Multiple server processes should be controlled independently by the deployment environment if a shared global limit is needed.

## Remote account

The SSH account should be restricted independently from MCP. If privileged operations are required, use narrow sudo rules for specific commands rather than broad passwordless sudo/root login.

## Audit

The runtime persists JSONL audit events for network-backed and mutating operations. Records include:

- request ID;
- timestamp;
- target;
- operation/task;
- policy decision once known;
- outcome;
- exit code where applicable;
- duration where applicable.

A `started` record has no policy decision because authorization may not have happened yet. A policy denial records `deny`; post-authorization execution outcomes record `allow`.

Task parameter values are intentionally omitted by default. This minimizes accidental leakage while preserving enough information to correlate an action with the configured task and target. Secrets are never audited.

Audit initialization is fail-closed: if the configured audit file cannot be opened, the runtime does not start. An audit write error is surfaced instead of being silently ignored.

## Dangerous optional capabilities

These must never silently appear in the default profile:

- arbitrary shell execution;
- unrestricted sudo;
- arbitrary environment injection;
- unrestricted remote path access;
- unrestricted model-controlled local filesystem paths;
- host-key verification disablement;
- model-controlled changes to policy/credentials.
