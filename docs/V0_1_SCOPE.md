# v0.1 Completion Boundary

`remote-exec-mcp` v0.1 is complete when the safe generic remote-execution core is usable end-to-end through MCP without exposing an unrestricted shell.

## Included in v0.1

- stdio MCP server;
- target/task discovery;
- target connectivity check;
- declarative task execution through policy -> validation -> planning;
- SSH key and password authentication through `SecretProvider` references;
- strict known-host verification with explicit `accept-new` mode;
- bounded command time/stdout/stderr;
- bounded SFTP upload/download with local and remote path roots;
- global process concurrency limit;
- durable JSONL audit persistence;
- stable machine-readable errors;
- unit/regression tests for command quoting, validation, policy, host-key behavior, path boundaries, transfer bounds, secret redaction, and audit serialization;
- a disposable in-process SSH integration test covering secret resolution, host-key handling, password authentication, session establishment, command execution, stdout/stderr, and exit status;
- runnable configuration and MCP client documentation.

## Explicitly outside the v0.1 core

These are future capability/hardening extensions rather than unfinished prerequisites for the generic MCP core:

- SSH-agent identity-selection provider;
- process-tree/remote-PID hard cancellation semantics after a timeout;
- crash-proof atomic replacement on every remote filesystem and orphan-temp garbage collection after process death;
- systemd/Docker/Kubernetes semantic adapters;
- Java/JAR deployment, rollback, and post-deploy workflows;
- a distributed/global rate limiter across multiple MCP server processes.

Keeping these outside v0.1 preserves separation of concerns: the generic remote-execution MCP stays small and auditable, while domain workflows can depend on it without turning it into a monolithic operations server.
