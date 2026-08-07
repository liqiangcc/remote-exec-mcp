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
- [x] typed parameter validation
- [x] task planner -> execution plan
- [x] policy engine interface
- [x] audit sink interface
- [x] secret provider interface
- [x] application use-case orchestration

## Phase 2 — SSH infrastructure
- [ ] SSH transport/session
- [ ] key authentication through secret references
- [ ] known_hosts verification
- [ ] connection timeout
- [ ] SSH command executor
- [ ] bounded stdout/stderr
- [ ] SFTP/SCP file transfer
- [ ] transfer size/path limits

## Phase 3 — MCP adapter
- [ ] list_targets
- [ ] check_target
- [ ] list_tasks
- [ ] run_task
- [ ] upload_file
- [ ] download_file

## Phase 4 — Production hardening
- [ ] concurrency limits
- [ ] cancellation
- [ ] structured audit persistence
- [ ] password/SSH-agent providers
- [ ] integration tests with disposable SSH server
- [ ] threat-model tests

## Phase 5 — Capability adapters/workflows
- [ ] systemd adapter
- [ ] Docker adapter
- [ ] Kubernetes adapter
- [ ] Java/JAR deployment workflow
- [ ] rollback workflow
- [ ] post-deploy verification hooks
