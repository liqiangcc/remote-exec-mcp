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
  secret_ref: ssh-key:prod
```

A `SecretProvider` resolves the reference at runtime. MCP responses, task definitions, and audit events never contain private keys, passwords, tokens, or secret environment values.

## SSH host verification

Strict known-host verification is the default. Development-only relaxation such as `accept-new` must be explicit. Disabling verification entirely should require a separate unsafe profile, if supported at all.

## Command safety

The model selects a named task and typed parameters. Operator-owned planning metadata resolves that request into a program + argv execution plan.

Avoid shell concatenation. Prefer:

```text
program = systemctl
argv = [restart, validated_service]
```

A future shell-backed task must be an explicit higher-risk capability with stronger policy controls.

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
