use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use russh::client;
use russh::keys::{self, known_hosts, PrivateKeyWithHashAlg, PublicKey};
use tokio::time::timeout;

use crate::config::{AuthConfig, HostKeyPolicy, TargetTransportConfig};
use crate::error::{AppError, AppResult, ErrorCode};
use crate::secret::{SecretProvider, SecretRef};
use crate::transport::{ConnectionInfo, Transport};

pub struct SshTransport<P> {
    secrets: P,
}

impl<P> SshTransport<P> {
    pub fn new(secrets: P) -> Self {
        Self { secrets }
    }
}

pub struct SshSession {
    handle: client::Handle<HostKeyHandler>,
    remote_identity: String,
}

impl SshSession {
    pub fn is_closed(&self) -> bool {
        self.handle.is_closed()
    }

    pub fn remote_identity(&self) -> &str {
        &self.remote_identity
    }

    pub(crate) async fn open_session_channel(
        &self,
    ) -> Result<russh::Channel<client::Msg>, russh::Error> {
        self.handle.channel_open_session().await
    }
}

impl<P> SshTransport<P>
where
    P: SecretProvider,
{
    async fn connect_ssh(&self, target: &TargetTransportConfig) -> AppResult<SshSession> {
        let TargetTransportConfig::Ssh {
            host,
            port,
            user,
            auth,
            host_key_policy,
            known_hosts_path,
            connect_timeout_seconds,
        } = target;

        if *connect_timeout_seconds == 0 {
            return Err(AppError::new(
                ErrorCode::InvalidConfiguration,
                "SSH connect timeout must be greater than zero",
            ));
        }

        let verifier = HostKeyVerifier::new(
            host.clone(),
            *port,
            host_key_policy.clone(),
            known_hosts_path.as_ref().map(PathBuf::from),
        );
        let handler = HostKeyHandler { verifier };
        let duration = Duration::from_secs(*connect_timeout_seconds);
        let config = Arc::new(client::Config {
            inactivity_timeout: Some(duration),
            ..Default::default()
        });

        let connect = client::connect(config, (host.as_str(), *port), handler);
        let mut handle = timeout(duration, connect)
            .await
            .map_err(|_| {
                AppError::new(
                    ErrorCode::ConnectionTimeout,
                    format!("SSH connection timed out for {host}:{port}"),
                )
            })?
            .map_err(map_client_error)?;

        let authentication = match auth {
            AuthConfig::Key { secret_ref } => {
                let secret_ref = SecretRef(secret_ref.clone());
                let private_key = self.secrets.resolve(&secret_ref)?;
                let private_key =
                    keys::decode_secret_key(private_key.expose(), None).map_err(|_| {
                        AppError::new(
                            ErrorCode::InvalidPrivateKey,
                            format!(
                                "failed to decode SSH private key from reference {}",
                                secret_ref.0
                            ),
                        )
                    })?;

                timeout(duration, async {
                    let hash_algorithm = handle.best_supported_rsa_hash().await?.flatten();
                    let key = PrivateKeyWithHashAlg::new(Arc::new(private_key), hash_algorithm);
                    handle.authenticate_publickey(user.clone(), key).await
                })
                .await
                .map_err(|_| authentication_timeout(user, host, *port))?
                .map_err(|error| authentication_error("public-key", error))?
            }
            AuthConfig::Password { secret_ref } => {
                let secret_ref = SecretRef(secret_ref.clone());
                let password = self.secrets.resolve(&secret_ref)?;
                timeout(
                    duration,
                    handle.authenticate_password(user.clone(), password.expose().to_owned()),
                )
                .await
                .map_err(|_| authentication_timeout(user, host, *port))?
                .map_err(|error| authentication_error("password", error))?
            }
        };

        if !authentication.success() {
            return Err(AppError::new(
                ErrorCode::AuthenticationFailed,
                format!("SSH authentication was rejected for {user}@{host}:{port}"),
            ));
        }

        Ok(SshSession {
            handle,
            remote_identity: format!("{user}@{host}:{port}"),
        })
    }

    async fn check_ssh(&self, target: &TargetTransportConfig) -> AppResult<ConnectionInfo> {
        let session = self.connect_ssh(target).await?;
        let remote_identity = session.remote_identity().to_owned();
        drop(session);

        Ok(ConnectionInfo {
            reachable: true,
            remote_identity: Some(remote_identity),
        })
    }
}

fn authentication_timeout(user: &str, host: &str, port: u16) -> AppError {
    AppError::new(
        ErrorCode::ConnectionTimeout,
        format!("SSH authentication timed out for {user}@{host}:{port}"),
    )
}

fn authentication_error(method: &str, error: russh::Error) -> AppError {
    AppError::new(
        ErrorCode::AuthenticationFailed,
        format!("SSH {method} authentication failed: {error}"),
    )
}

impl<P> Transport for SshTransport<P>
where
    P: SecretProvider,
{
    type Session = SshSession;

    fn connect(
        &self,
        target: &TargetTransportConfig,
    ) -> impl std::future::Future<Output = AppResult<Self::Session>> + Send {
        self.connect_ssh(target)
    }

    fn check(
        &self,
        target: &TargetTransportConfig,
    ) -> impl std::future::Future<Output = AppResult<ConnectionInfo>> + Send {
        self.check_ssh(target)
    }
}

#[derive(Debug)]
enum SshClientError {
    Ssh(russh::Error),
    HostKey(AppError),
}

impl From<russh::Error> for SshClientError {
    fn from(error: russh::Error) -> Self {
        Self::Ssh(error)
    }
}

impl fmt::Display for SshClientError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ssh(error) => write!(formatter, "{error}"),
            Self::HostKey(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for SshClientError {}

fn map_client_error(error: SshClientError) -> AppError {
    match error {
        SshClientError::HostKey(error) => error,
        SshClientError::Ssh(error) => AppError::new(
            ErrorCode::ConnectionFailed,
            format!("SSH connection failed: {error}"),
        ),
    }
}

struct HostKeyHandler {
    verifier: HostKeyVerifier,
}

impl client::Handler for HostKeyHandler {
    type Error = SshClientError;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKey,
    ) -> Result<bool, Self::Error> {
        self.verifier
            .verify(server_public_key)
            .map(|_| true)
            .map_err(SshClientError::HostKey)
    }
}

#[derive(Debug, Clone)]
struct HostKeyVerifier {
    host: String,
    port: u16,
    policy: HostKeyPolicy,
    known_hosts_path: Option<PathBuf>,
}

impl HostKeyVerifier {
    fn new(
        host: String,
        port: u16,
        policy: HostKeyPolicy,
        known_hosts_path: Option<PathBuf>,
    ) -> Self {
        Self {
            host,
            port,
            policy,
            known_hosts_path,
        }
    }

    fn verify(&self, public_key: &PublicKey) -> AppResult<()> {
        let known = self.check(public_key).map_err(|error| {
            AppError::new(
                ErrorCode::HostKeyRejected,
                format!(
                    "SSH host key verification failed for {}:{}: {error}",
                    self.host, self.port
                ),
            )
        })?;

        if known {
            return Ok(());
        }

        match self.policy {
            HostKeyPolicy::Strict => Err(AppError::new(
                ErrorCode::HostKeyRejected,
                format!(
                    "SSH host key is not trusted for {}:{}",
                    self.host, self.port
                ),
            )),
            HostKeyPolicy::AcceptNew => self.learn(public_key).map_err(|error| {
                AppError::new(
                    ErrorCode::HostKeyRejected,
                    format!(
                        "failed to record SSH host key for {}:{}: {error}",
                        self.host, self.port
                    ),
                )
            }),
        }
    }

    fn check(&self, public_key: &PublicKey) -> Result<bool, keys::Error> {
        if let Some(path) = &self.known_hosts_path {
            known_hosts::check_known_hosts_path(&self.host, self.port, public_key, path)
        } else {
            known_hosts::check_known_hosts(&self.host, self.port, public_key)
        }
    }

    fn learn(&self, public_key: &PublicKey) -> Result<(), keys::Error> {
        if let Some(path) = &self.known_hosts_path {
            known_hosts::learn_known_hosts_path(&self.host, self.port, public_key, path)
        } else {
            known_hosts::learn_known_hosts(&self.host, self.port, public_key)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn public_key() -> PublicKey {
        keys::parse_public_key_base64(
            "AAAAC3NzaC1lZDI1NTE5AAAAIJdD7y3aLq454yWBdwLWbieU1ebz9/cu7/QEXn9OIeZJ",
        )
        .unwrap()
    }

    #[test]
    fn strict_policy_rejects_unknown_host_key() {
        let directory = tempfile::tempdir().unwrap();
        let verifier = HostKeyVerifier::new(
            "localhost".to_owned(),
            13265,
            HostKeyPolicy::Strict,
            Some(directory.path().join("known_hosts")),
        );

        let error = verifier.verify(&public_key()).unwrap_err();
        assert_eq!(error.code, ErrorCode::HostKeyRejected);
    }

    #[test]
    fn accept_new_records_key_that_strict_policy_can_verify() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("known_hosts");
        let key = public_key();

        HostKeyVerifier::new(
            "localhost".to_owned(),
            13265,
            HostKeyPolicy::AcceptNew,
            Some(path.clone()),
        )
        .verify(&key)
        .unwrap();

        HostKeyVerifier::new(
            "localhost".to_owned(),
            13265,
            HostKeyPolicy::Strict,
            Some(path),
        )
        .verify(&key)
        .unwrap();
    }
}
