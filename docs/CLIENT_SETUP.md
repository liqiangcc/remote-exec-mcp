# MCP Client Setup

1. Build the server with `cargo build --release`.
2. Copy `config/example.yaml` to a private local configuration file and edit target/policy values.
3. Ensure local staging roots and the selected `known_hosts` file/directory exist.
4. Export the referenced authentication secret environment variable.
5. Configure the MCP client to launch `remote-exec-mcp --config /absolute/path/to/config.yaml` over stdio.
6. Start with `list_targets`, then `list_tasks`, then `check_target` before using `run_task` or file transfer.
7. Keep the remote SSH account least-privileged and use narrow sudo rules when privileged tasks are required.

For password authentication, use:

```yaml
auth:
  type: password
  secret_ref: env:REMOTE_EXEC_SSH_PASSWORD
```

For key authentication, use:

```yaml
auth:
  type: key
  secret_ref: env:REMOTE_EXEC_SSH_KEY
```

Never place the actual password/private key in YAML or an MCP client JSON configuration. The `env:` value is an opaque reference resolved by the server process.
