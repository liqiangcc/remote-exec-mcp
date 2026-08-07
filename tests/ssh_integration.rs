use std::sync::Arc;
use std::time::Duration;

use remote_exec_mcp::config::{AuthConfig, HostKeyPolicy, TargetTransportConfig};
use remote_exec_mcp::domain::CommandSpec;
use remote_exec_mcp::execution::ssh::SshCommandExecutor;
use remote_exec_mcp::execution::{CommandExecutor, ExecutionLimits};
use remote_exec_mcp::secret::EnvSecretProvider;
use remote_exec_mcp::transport::ssh::SshTransport;
use remote_exec_mcp::transport::Transport;
use russh::keys::{Algorithm, PrivateKey};
use russh::server::{self, Msg, Session};
use russh::{Channel, ChannelId};
use tokio::net::TcpListener;

struct TestSshServer;

impl server::Handler for TestSshServer {
    type Error = russh::Error;

    async fn auth_password(
        &mut self,
        user: &str,
        password: &str,
    ) -> Result<server::Auth, Self::Error> {
        if user == "deploy" && password == "test-password" {
            Ok(server::Auth::Accept)
        } else {
            Ok(server::Auth::Reject {
                proceed_with_methods: None,
                partial_success: false,
            })
        }
    }

    async fn channel_open_session(
        &mut self,
        _channel: Channel<Msg>,
        reply: server::ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        reply.accept().await;
        Ok(())
    }

    async fn exec_request(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        if data == b"'printf' '%s' 'hello-from-ssh'" {
            session.channel_success(channel)?;
            session.data(channel, b"hello-from-ssh".to_vec())?;
            session.exit_status_request(channel, 0)?;
            session.eof(channel)?;
            session.close(channel)?;
        } else {
            session.channel_failure(channel)?;
        }
        Ok(())
    }
}

#[tokio::test]
async fn password_transport_executes_command_against_disposable_server() {
    let mut server_config = server::Config {
        auth_rejection_time: Duration::from_millis(1),
        auth_rejection_time_initial: Some(Duration::from_millis(1)),
        ..Default::default()
    };
    server_config.keys.push(
        PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519)
            .expect("test server key generation must succeed"),
    );
    let server_config = Arc::new(server_config);

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("test listener must bind");
    let address = listener.local_addr().expect("test listener has an address");

    let server_task = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.expect("test client must connect");
        server::run_stream(server_config, socket, TestSshServer)
            .await
            .expect("test SSH server session must run");
    });

    let env_name = format!("REMOTE_EXEC_TEST_PASSWORD_{}", std::process::id());
    std::env::set_var(&env_name, "test-password");

    let known_hosts_dir = tempfile::tempdir().expect("temporary known-hosts directory");
    let target = TargetTransportConfig::Ssh {
        host: "127.0.0.1".to_owned(),
        port: address.port(),
        user: "deploy".to_owned(),
        auth: AuthConfig::Password {
            secret_ref: format!("env:{env_name}"),
        },
        host_key_policy: HostKeyPolicy::AcceptNew,
        known_hosts_path: Some(
            known_hosts_dir
                .path()
                .join("known_hosts")
                .to_string_lossy()
                .into_owned(),
        ),
        connect_timeout_seconds: 5,
    };

    let transport = SshTransport::new(EnvSecretProvider);
    let mut session = transport
        .connect(&target)
        .await
        .expect("client must establish an authenticated SSH session");

    let result = SshCommandExecutor
        .execute(
            &mut session,
            &CommandSpec {
                program: "printf".to_owned(),
                args: vec!["%s".to_owned(), "hello-from-ssh".to_owned()],
            },
            ExecutionLimits::new(5),
        )
        .await
        .expect("command must execute through the SSH adapter");

    assert!(result.success);
    assert_eq!(result.exit_code, Some(0));
    assert_eq!(result.stdout, "hello-from-ssh");
    assert!(result.stderr.is_empty());
    assert!(!result.stdout_truncated);
    assert!(!result.stderr_truncated);

    std::env::remove_var(&env_name);
    drop(session);
    server_task.abort();
}
