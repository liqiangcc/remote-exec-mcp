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
- [x] SSH transport/session
- [x] key authentication through secret references
- [x] known_hosts verification
- [x] connection timeout
- [x] SSH command executor
- [x] bounded stdout/stderr
- [x] SFTP file transfer
- [x] transfer size/path limits

## Phase 3 — MCP adapter
- [x] stdio MCP server
- [x] list_targets
- [x] check_target
- [x] list_tasks
- [x] run_task
- [x] upload_file
- [x] download_file
- [x] stable application error codes preserved in MCP errors
- [x] local MCP-host upload/download roots

## Phase 4 — Production hardening
- [ ] concurrency limits
- [ ] cancellation
- [ ] structured audit persistence
- [ ] password/SSH-agent providers
- [ ] integration tests with disposable SSH server
- [ ] threat-model tests
- [ ] atomic overwrite semantics / orphan temp cleanup for interrupted transfers

## Phase 5 — Capability adapters/workflows
- [ ] systemd adapter
- [ ] Docker adapter
- [ ] Kubernetes adapter
- [ ] Java/JAR deployment workflow
- [ ] rollback workflow
- [ ] post-deploy verification hooks
