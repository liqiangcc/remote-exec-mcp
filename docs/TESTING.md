# Testing

The project uses an evidence chain rather than relying on architecture claims alone.

## Required CI gates

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

## Security regression coverage

Tests cover:

- target/task policy denial;
- typed parameter validation;
- static command program enforcement;
- POSIX quoting of model-controlled argv values;
- output truncation and execution-limit validation;
- secret redaction and zeroization wrapper behavior;
- strict/accept-new host-key behavior;
- remote path normalization/root checks and size limits;
- local upload/download root checks;
- audit event serialization and policy-decision semantics;
- password-auth configuration parsing.

## Disposable SSH integration test

`tests/ssh_integration.rs` starts an in-process `russh` server on an ephemeral loopback port with a generated test-only host key. The client resolves a test password through the same `EnvSecretProvider` used by the runtime, persists the generated host key through explicit `accept-new`, establishes a real SSH session, and executes a structured command through `SshCommandExecutor`.

The integration chain is:

```text
EnvSecretProvider
  -> SshTransport
  -> accept-new host-key persistence
  -> password authentication
  -> SshSession
  -> SshCommandExecutor
  -> stdout/stderr/exit status
```

No external SSH server or production credential is required. This integration test complements rather than replaces the deterministic unit-level threat tests above.
