# Architecture Decisions

## AD-001 — MCP is an adapter, not the domain

The MCP layer owns schemas, tool descriptions, and protocol conversion. It never owns SSH, authorization, secret resolution, or command construction.

## AD-002 — Tasks are public intent; commands are internal plans

Clients select declarative tasks and typed parameters. `PlanningService` authorizes and validates the request before producing `ExecutionPlan`. No MCP tool exposes arbitrary shell text.

## AD-003 — Transport and capabilities remain separate

`SshTransport` establishes authenticated sessions. `SshCommandExecutor` and `SftpFileTransfer` use those sessions without becoming transport concerns.

## AD-004 — Policy is fail-closed

Target task allowlists, remote upload/download roots, maximum transfer bytes, local file roots, strict host verification, timeouts, and process concurrency are explicit deterministic boundaries. Missing file-transfer limits disable transfer rather than silently widening access.

## AD-005 — Secrets are references

Authentication configuration contains `secret_ref`. The current environment provider resolves key/password values at runtime. Secret material is redacted in debug output, zeroized where owned by the core, omitted from MCP responses, and never written to audit records.

## AD-006 — Audit is part of runtime correctness

Network-backed operations emit a start record and a terminal record. Audit initialization and writes fail closed instead of silently dropping security evidence.

## AD-007 — Higher-level operations remain separate

systemd, Docker, Kubernetes, deployment, rollback, and verification are semantic workflow layers. They may reuse this core but should not be folded into SSH transport or generic MCP protocol code.
