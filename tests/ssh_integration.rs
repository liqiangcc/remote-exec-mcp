use std::sync::Arc;
use std::time::Duration;

use remote_exec_mcp::config::{AuthConfig, HostKeyPolicy, TargetTransportConfig};
use remote_exec_mcp::domain::CommandSpec;
use remote_exec_mcp::error::AppResult;
use remote_exec_mcp::execution::ssh::SshCommandExecutor;
use remote_exec_mcp::execution::{CommandExecutor, ExecutionLimits};
use remote_exec_mcp::secret::{SecretProvider, SecretRef, SecretValue};
use remote_exec_mcp::transport::ssh::SshTransport;
use remote_exec_mcp::transport::Transport;
use russh::keys;
use russh::server;
use russh::{Channel, ChannelId};
use tokio::net::TcpListener;

const TEST_SERVER_KEY: &str = r#"-----BEGIN OPENSSH PRIVATE KEY-----
b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZWQyNTUx
OQAAACCGyd3YJApvyX7ppn2SziSih7UnUMzDCCwjyxNBrUCmtgAAAIgAaKaGAGimhgAAAAtzc2gt
ZWQyNTUxOQAAACCGyd3YJApvyX7ppn2SziSih7UnUMzDCCwjyxNBrUCmtgAAAECnN/xvlah3L9lE
5eYp1VVVBLAb7qb4saQqzIGTs/kCWobJ3dgkCm/JfummfZLOJKKHtSdQzMMILCPLE0GtQKa2AAAA
AAECAwQF
-----END OPENSSH PRIVATE KEY-----"#;

#[derive(Debug, Clone, Copy)]
struct TestSecretProvider;

impl SecretProvider for TestSecretProvider {
    fn resolve(&self, _secret: &SecretRef) -> AppResult<SecretValue> {
        Ok(SecretValue::new("integration-password"))
    }
}

#[derive(Clone)]
struct TestServer;

impl server::Handler for TestServer {
    type Error = russh::Error;

    async fn auth_password(
        &mut self,
        user: &str,
        password: &str,
    ) -> Result<server::Auth, Self::Error> {
        if user == "integration" && password == "integration-password" {
            Ok(server::Auth::Accept)
        } else {
            Ok(server::Auth::reject())
        }
    }

    async fn channel_open_session(
        &mut self,
        _channel: Channel<server::Msg>,
        reply: server::ChannelOpenHandle,
        _session: &mut server::Session,
    ) -> Result<(), Self::Error> {
        reply.accept().await;
        Ok(())
    }

    async fn exec_request(
        &mut self,
        channel: ChannelId,
        _data: &[u8],
        session: &mut server::Session,
    ) -> Result<(), Self::Error> {
        session.channel_success(channel)?;
        session.data(channel, b"integration-ok\n".to_vec())?;
        session.extended_data(channel, 1, b"integration-stderr\n".to_vec())?;
        session.exit_status_request(channel, 0)?;
        session.eof(channel)?;
        session.close(channel)?;
        Ok(())
    }
}

#[tokio::test]
async fn password_transport_and_command_executor_work_end_to_end() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server_key = keys::decode_secret_key(TEST_SERVER_KEY, None).unwrap();
    let server_config = Arc::new(server::Config {
        auth_rejection_time: Duration::from_millis(10),
        auth_rejection_time_initial: Some(Duration::ZERO),
        keys: vec![server_key],
        ..Default::default()
    });

    let server_task = tokio::spawn(async move {
        let Ok((stream, _)) = listener.accept().await else {
            return;
        };

        // The fixture owns exactly one connection. Depending on the precise
        // close ordering, russh may surface UnexpectedEof while the peer is
        // shutting down after the exit status has already been delivered.
        // The client-side assertions below are the integration contract, so
        // teardown transport errors must not turn a successful exchange into
        // a flaky test panic.
        if let Ok(running) = server::run_stream(server_config, stream, TestServer).await {
            let _ = running.await;
        }
    });

    let directory = tempfile::tempdir().unwrap();
    let known_hosts_path = directory.path().join("known_hosts");
    let target = TargetTransportConfig::Ssh {
        host: address.ip().to_string(),
        port: address.port(),
        user: "integration".to_owned(),
        auth: AuthConfig::Password {
            secret_ref: "test:password".to_owned(),
        },
        host_key_policy: HostKeyPolicy::AcceptNew,
        known_hosts_path: Some(known_hosts_path.to_string_lossy().into_owned()),
        connect_timeout_seconds: 5,
    };

    let transport = SshTransport::new(TestSecretProvider);
    let mut session = transport.connect(&target).await.unwrap();
    let result = SshCommandExecutor
        .execute(
            &mut session,
            &CommandSpec {
                program: "printf".to_owned(),
                args: vec!["ignored-by-test-server".to_owned()],
            },
            ExecutionLimits::new(5),
        )
        .await
        .unwrap();

    assert!(result.success);
    assert_eq!(result.exit_code, Some(0));
    assert_eq!(result.stdout, "integration-ok\n");
    assert_eq!(result.stderr, "integration-stderr\n");
    assert!(known_hosts_path.exists());

    drop(session);
    let _ = server_task.await;
}
