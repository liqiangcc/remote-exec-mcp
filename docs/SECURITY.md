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

Before transfer:
- normalize/validate absolute paths;
- reject parent traversal;
- enforce configured upload/download roots;
- handle symlink escape where the backend can verify it;
- apply maximum file size;
- avoid overwrite by default unless explicitly allowed.

## Remote account

The SSH account should be restricted independently from MCP. If privileged operations are required, use narrow sudo rules for specific commands rather than broad passwordless sudo/root login.

## Audit

Record at least:
- request/execution ID;
- timestamp;
- target;
- operation/task;
- sanitized parameters;
- policy decision;
- outcome;
- exit code where applicable;
- duration.

Never record credentials or raw secret values.

## Dangerous optional capabilities

These must never silently appear in the default profile:
- arbitrary shell execution;
- unrestricted sudo;
- arbitrary environment injection;
- unrestricted remote path access;
- host-key verification disablement;
- model-controlled changes to policy/credentials.
