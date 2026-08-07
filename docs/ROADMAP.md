# Roadmap

## Phase 0 — Architecture bootstrap
- [x] Define project boundary
- [x] Separate protocol/application/domain/infrastructure
- [x] Separate Task from Command/ExecutionPlan
- [x] Separate Transport from Executor/FileTransfer
- [x] Define security and audit boundaries
- [x] Add configuration example

## Phase 1 — Core domain and application
- [x] Stable error model
- [x] Target/task catalog
- [x] Typed parameter validation
- [x] Task planner -> execution plan
- [x] Policy engine interface
- [x] Audit sink interface
- [x] Secret provider interface
- [x] Application use-case orchestration

## Phase 2 — SSH infrastructure
- [x] SSH transport/session
- [x] Key authentication through secret references
- [x] Password authentication through secret references
- [x] known_hosts verification
- [x] Connection/authentication timeout
- [x] SSH command executor
- [x] Bounded stdout/stderr
- [x] SFTP file transfer
- [x] Transfer size/path limits

## Phase 3 — MCP adapter
- [x] stdio MCP server
- [x] list_targets
- [x] check_target
- [x] list_tasks
- [x] run_task
- [x] upload_file
- [x] download_file
- [x] Structured MCP error mapping

## Phase 4 — v0.1 production hardening
- [x] Process-level concurrency limits
- [x] Command/transfer timeouts and best-effort channel cancellation
- [x] Structured JSONL audit persistence
- [x] Local filesystem allowlist boundary for MCP file tools
- [x] Threat-model regression tests for task validation, command quoting, host keys, local/remote paths, and transfer limits
- [x] Fail-closed audit/config initialization
- [x] Disposable in-process SSH integration test

## Post-v0.1 hardening options

These are useful extensions, but they are not prerequisites for the generic v0.1 remote-execution core. See `V0_1_SCOPE.md`.

- [ ] SSH-agent identity provider / identity-selection policy
- [ ] Strong remote-process/process-tree termination semantics after timeout
- [ ] Crash-proof atomic replacement on every remote filesystem
- [ ] Orphan temporary-file garbage collection after process-level interruption
- [ ] Cross-process/distributed rate limiting

## Capability adapters/workflows

These are deliberately separate higher-level concerns rather than unfinished transport work:

- [ ] systemd semantic adapter
- [ ] Docker adapter
- [ ] Kubernetes adapter
- [ ] Java/JAR deployment workflow
- [ ] Rollback workflow
- [ ] Post-deploy verification hooks
