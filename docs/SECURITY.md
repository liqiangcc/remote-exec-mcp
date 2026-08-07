# Security Model

## Threat model

The MCP server may receive incorrect, overly broad, or adversarial instructions from an AI client. Connectivity to a host must never imply unrestricted authority on that host.

## Principles

1. Deny by default.
2. Do not expose a general shell in the default profile.
3. Validate every model-controlled parameter before planning.
4. Authorize before execution.
5. Keep secrets behind opaque references/providers.
6. Enforce least privilege in both MCP policy and remote OS accounts.
7. Bound time, output, concurrency, and file sizes.
8. Verify target identity.
9. Audit attempted privileged operations.
10. Keep security policy independent from transport/executor implementations.

## MCP adapter boundary

The MCP adapter is a protocol boundary, not a second execution engine. It defines tool schemas, maps stable application errors into MCP errors, and delegates operations to `RemoteExecService`.

The stdio server reserves stdout for MCP protocol frames; diagnostic logging is written to stderr.

`run_task` does not construct commands. It delegates through the existing application pipeline:

```text
MCP tool
   -> RemoteExecService
   -> catalog / policy
   -> typed validation
   -> planning
   -> ExecutionPlan
   -> CommandExecutor
   -> SSH Session
```

`upload_file` and `download_file` require two independent path boundaries:

- remote upload/download roots constrain paths on the SSH target;
- local upload/download roots constrain paths on the host running the MCP server.

The filesystem root cannot be configured as an allowed local transfer root. Local upload sources are canonicalized before authorization, and local download destination parents are canonicalized before authorization. File transfer is disabled unless `max_transfer_bytes` is explicitly configured for the target.

## Authorization pipeline

```text
MCP Request
   -> schema validation
   -> target/task lookup
   -> policy evaluation
   -> execution planning
   -> executor
```

A denied request must never be transformed into an executable command.

## Secrets

Configuration stores references:

```yaml
auth:
  type: key
  secret_ref: env:REMOTE_EXEC_SSH_KEY
```

A `SecretProvider` resolves the reference at runtime. The first provider reads explicitly prefixed `env:` references. Additional providers such as Vault or a password manager can implement the same interface later.

Resolved secret values use a redacted debug representation and are zeroized when dropped. MCP responses, task definitions, and audit events never contain private keys, passwords, tokens, or secret environment values.

## SSH host verification

Strict known-host verification is the default. The SSH client checks the server public key against either the configured `known_hosts_path` or the current user's standard `~/.ssh/known_hosts` file.

Development-only `accept-new` is explicit: an unknown host key may be recorded, but a changed key is still rejected. Disabling verification entirely is not supported by the default transport.

Connection and public-key authentication are bounded by the configured SSH connection timeout. A timeout is surfaced separately from authentication failure and host-key rejection.

## Command safety

The model selects a named task and typed parameters. Operator-owned planning metadata resolves that request into a program + argv execution plan.

Prefer:

```text
program = systemctl
argv = [restart, validated_service]
```

SSH `exec` transports a command string and the server passes it to a shell. The SSH command executor is therefore the only layer allowed to serialize the structured `program + argv` plan: every token is POSIX single-quoted, embedded single quotes are escaped, and NUL bytes are rejected. The executor never accepts model-provided raw shell text.

This quoting step is a transport serialization detail, not authorization. Policy and validation must already have completed before the command reaches the executor.

Standard output and standard error are captured independently with bounded byte limits. Truncation is reported in `ExecutionResult` instead of allowing unbounded memory growth. Command execution is also time-bounded; timeout handling performs a best-effort SSH channel close. Strong remote-process cancellation remains a separate production-hardening capability.

A future explicitly shell-backed task must be a separate higher-risk capability with stronger policy controls.

## Filesystem safety

The file-transfer adapter uses SFTP over an already-authenticated SSH session. Policy owns the allowed upload/download roots and maximum byte count; the SFTP adapter only enforces those constraints.

Remote paths are treated as POSIX paths independent of the MCP host operating system. Before transfer the adapter:
- requires absolute remote paths;
- rejects `..` traversal and NUL bytes;
- rejects `/` as a configured root;
- canonicalizes configured roots on the remote server;
- canonicalizes an existing remote source/destination, or the parent directory for a new upload;
- re-checks the canonical path against the canonical roots, preventing symlink escape;
- enforces `max_transfer_bytes` from metadata when available and again while streaming;
- transfers through bounded timeouts;
- avoids overwrite by default.

Uploads first write to a uniquely named temporary file in the authorized destination directory and only rename it into place after the byte limit and sync checks succeed. Downloads similarly use a local temporary file so ordinary failures do not expose a partially written destination.

Overwrite is intentionally explicit. Current cross-platform replacement semantics may remove the old destination immediately before rename; fully atomic replacement and cleanup of orphan temporary files after forced interruption are production-hardening work.

## Runtime admission control

Remote operations share one application-level concurrency limiter. The default maximum is four active operations and operators may configure a different positive value with `runtime.max_concurrent_operations`.

Admission is fail-fast rather than queued. When all permits are occupied, a new remote operation is rejected with the stable `concurrency_limit_exceeded` error. This prevents an AI client from creating an unbounded in-process backlog while remote systems are slow or unavailable.

`list_targets` and `list_tasks` are local catalog reads and do not consume a remote-operation permit. `check_target`, `run_task`, `upload_file`, and `download_file` do.

## Remote account

The SSH account should be restricted independently from MCP. If privileged operations are required, use narrow sudo rules for specific commands rather than broad passwordless sudo/root login.

## Audit

Audit persistence is disabled by default. Operators may explicitly enable an append-only JSON Lines sink:

```yaml
runtime:
  audit:
    type: jsonl
    path: ./data/audit.jsonl
```

Audit records include:
- request/execution ID;
- timestamp;
- target;
- operation/task;
- sanitized parameter names only;
- policy decision where available;
- outcome and stable error code where applicable;
- exit code for command execution where applicable;
- transferred byte count for file transfers where applicable;
- duration.

Parameter values are never persisted. File-transfer source/destination paths are also not persisted by the current audit model. Credentials and raw secret values must never appear in audit output.

The audit lifecycle deliberately has asymmetric failure semantics:

1. Before a remote operation starts, the `started` record must be persisted successfully. If that write fails, the remote operation is rejected and never begins.
2. After the remote operation outcome is known, the final audit record is best-effort. If that write fails, the failure is sent to stderr, but the already-known remote-operation result is preserved.

The second rule prevents a successful side-effecting operation from being reported as failed merely because its final audit write failed; otherwise an Agent could retry the operation and duplicate the side effect.

Rejected requests are also written as best-effort audit events when auditing is enabled.

## Dangerous optional capabilities

These must never silently appear in the default profile:
- arbitrary shell execution;
- unrestricted sudo;
- arbitrary environment injection;
- unrestricted remote path access;
- arbitrary model-controlled local filesystem paths;
- host-key verification disablement;
- model-controlled changes to policy/credentials.
