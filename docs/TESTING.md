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

A disposable in-process SSH integration test is tracked separately because it validates multiple infrastructure components together rather than replacing the deterministic unit-level threat tests above.
