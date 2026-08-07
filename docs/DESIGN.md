# Design

## 1. Goal

`remote-exec-mcp` is a reusable remote operations foundation for AI agents. The core must remain useful whether the eventual operation is deployment, diagnostics, service management, file transfer, Docker, Kubernetes, or another system.

The architecture follows Separation of Concerns and Dependency Inversion: protocol and infrastructure stay at the edges; domain intent and application policy remain independent from MCP and SSH.

## 2. Responsibility map

### Protocol layer — MCP adapter

Responsibilities:
- decode MCP tool requests;
- map them to application requests;
- serialize application responses.

Must not:
- build shell commands;
- connect over SSH;
- read secrets directly;
- decide authorization.

### Application layer — use-case orchestration

Responsibilities:
- create request/execution IDs;
- load target/task metadata;
- request policy decisions;
- ask a planner for an `ExecutionPlan`;
- invoke execution ports;
- emit audit lifecycle events;
- map internal errors to stable application errors.

It coordinates components but does not implement SSH, shell parsing, or secret storage.

### Domain layer — intent and plans

Core concepts:
- `TargetId`: logical destination identity;
- `TaskRequest`: approved task name + typed parameters;
- `TaskDefinition`: public task schema, without a shell command;
- `ExecutionPlan`: already-resolved operation ready for an executor;
- `ExecutionOperation`: command/upload/download/etc.;
- `ExecutionResult`: normalized result independent of transport.

Key rule:

> A task expresses **what** the operator allows; an execution plan expresses **how** that approved task will be carried out.

### Security layer — policy and secrets

`PolicyEngine` answers whether an operation is permitted. It is independent of executors.

`SecretProvider` resolves opaque secret references. Targets/configuration store references such as `ssh-key:prod`, never secret values in public metadata.

### Execution layer — capability ports

Execution is split from connectivity.

Conceptually:

```rust
trait Transport {
    type Session;
    async fn connect(&self, target: &Target) -> Result<Self::Session>;
}

trait CommandExecutor<S> {
    async fn execute(&self, session: &mut S, command: &CommandSpec)
        -> Result<ExecutionResult>;
}

trait FileTransfer<S> {
    async fn upload(&self, session: &mut S, transfer: &TransferSpec)
        -> Result<TransferResult>;
}
```

SSH implements `Transport`. An SSH-backed command executor may execute commands over an SSH session, but these are separate abstractions.

### Infrastructure layer — adapters

Examples:
- SSH transport;
- local transport;
- SSH command executor;
- SFTP/SCP file transfer;
- filesystem/config loader;
- Vault/environment/agent secret providers;
- JSONL/SQLite audit sinks.

Infrastructure depends inward on ports/domain contracts; domain code must not import SSH/MCP-specific types.

## 3. Dependency direction

```text
MCP Adapter -----------+
                       v
                 Application
                  /   |   \
                 v    v    v
              Domain Policy Audit Port
                 ^      ^      ^
                 |      |      |
          Infrastructure Adapters
```

The domain layer has no dependency on:
- MCP SDK;
- SSH library;
- Docker SDK;
- Kubernetes SDK;
- concrete audit storage;
- concrete secret manager.

## 4. Task planning

Model input:

```text
run_task(target="test", task="restart-service", service="demo")
```

Application flow:

```text
TaskRequest
   |
   +--> validate typed parameters
   +--> evaluate target/task policy
   +--> TaskPlanner
            |
            v
      ExecutionPlan
            |
            v
        Executor
```

The model never sends:

```text
systemctl restart demo
```

The operator-owned task implementation may map `restart-service` to systemd today and to another backend later without changing the MCP contract.

## 5. Transport vs executor

A transport answers: **How do I establish access to this target?**

An executor answers: **How do I perform this operation through an established capability/session?**

Therefore this is intentionally rejected:

```text
SSH = connect + command policy + task resolution + file transfer + deployment
```

Instead:

```text
SSH Transport -> Session
                    |-- CommandExecutor
                    |-- FileTransfer
```

## 6. Docker and Kubernetes

Docker/Kubernetes are not transports. They are capability adapters.

Possible implementations include:

```text
KubernetesAdapter -> Kubernetes API
KubernetesAdapter -> local kubectl executor
KubernetesAdapter -> SSH session -> kubectl executor
```

All three can satisfy the same higher-level deployment use case while using different infrastructure.

## 7. Policy boundary

Policy is evaluated before execution. Executors do not contain production-environment rules or AI-specific authorization logic.

```text
Request -> PolicyDecision -> ExecutionPlan -> Executor
```

A denied request never reaches the executor.

## 8. Audit boundary

Audit is coordinated by the application layer as a cross-cutting concern:

```text
request accepted
 -> audit STARTED
 -> policy/planning/execution
 -> audit SUCCEEDED | FAILED | DENIED
```

Executors return facts; they do not decide what should be audited.

## 9. Configuration ownership

Configuration is operator-owned. The default MCP surface is read-only with respect to:
- target connection details;
- trust roots/known hosts;
- task implementations;
- allowlists;
- secret references.

The AI may inspect sanitized target/task metadata but cannot mutate its own authority.

## 10. Extension strategy

New features should enter through a new use case, port, or adapter rather than by adding conditionals to SSH code.

Examples:
- `systemd` adapter;
- Docker adapter;
- Kubernetes adapter;
- Java/JAR deployment workflow;
- rollback workflow;
- post-deploy verification;
- remote diagnostics.
