use std::process;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use russh_sftp::client::SftpSession;
use russh_sftp::protocol::OpenFlags;
use tokio::fs::{self, File, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::timeout;

use crate::domain::TransferSpec;
use crate::error::{AppError, AppResult, ErrorCode};
use crate::execution::{FileTransfer, TransferConstraints, TransferResult};
use crate::transport::ssh::SshSession;

#[derive(Debug, Default, Clone, Copy)]
pub struct SftpFileTransfer;

impl SftpFileTransfer {
    async fn upload_bounded(
        &self,
        session: &mut SshSession,
        transfer: &TransferSpec,
        constraints: TransferConstraints,
    ) -> AppResult<TransferResult> {
        let constraints = constraints.validate()?;
        let duration = Duration::from_secs(constraints.timeout_seconds);

        timeout(
            duration,
            self.upload_inner(session, transfer, &constraints),
        )
        .await
        .map_err(|_| {
            AppError::new(
                ErrorCode::TransferTimeout,
                format!(
                    "upload exceeded transfer timeout of {} seconds",
                    constraints.timeout_seconds
                ),
            )
        })?
    }

    async fn download_bounded(
        &self,
        session: &mut SshSession,
        transfer: &TransferSpec,
        constraints: TransferConstraints,
    ) -> AppResult<TransferResult> {
        let constraints = constraints.validate()?;
        let duration = Duration::from_secs(constraints.timeout_seconds);

        timeout(
            duration,
            self.download_inner(session, transfer, &constraints),
        )
        .await
        .map_err(|_| {
            AppError::new(
                ErrorCode::TransferTimeout,
                format!(
                    "download exceeded transfer timeout of {} seconds",
                    constraints.timeout_seconds
                ),
            )
        })?
    }

    async fn upload_inner(
        &self,
        session: &mut SshSession,
        transfer: &TransferSpec,
        constraints: &TransferConstraints,
    ) -> AppResult<TransferResult> {
        let metadata = fs::metadata(&transfer.source).await.map_err(|error| {
            transfer_failed(format!(
                "failed to inspect upload source {}: {error}",
                transfer.source
            ))
        })?;
        if !metadata.is_file() {
            return Err(AppError::new(
                ErrorCode::InvalidRequest,
                "upload source must be a regular local file",
            ));
        }
        ensure_size_allowed(metadata.len(), constraints.max_bytes)?;

        let sftp = open_sftp(session, constraints.timeout_seconds).await?;
        let destination = authorize_remote_path(
            &sftp,
            &transfer.destination,
            &constraints.allowed_remote_roots,
            true,
        )
        .await?;

        if sftp
            .try_exists(destination.clone())
            .await
            .map_err(|error| transfer_failed(format!("failed to inspect remote destination: {error}")))?
            && !transfer.overwrite
        {
            let _ = sftp.close().await;
            return Err(AppError::new(
                ErrorCode::DestinationExists,
                "remote destination already exists and overwrite is disabled",
            ));
        }

        let temp_destination = remote_temp_path(&destination);
        authorize_remote_path(
            &sftp,
            &temp_destination,
            &constraints.allowed_remote_roots,
            true,
        )
        .await?;

        let mut local = File::open(&transfer.source).await.map_err(|error| {
            transfer_failed(format!(
                "failed to open upload source {}: {error}",
                transfer.source
            ))
        })?;
        let mut remote = sftp
            .open_with_flags(
                temp_destination.clone(),
                OpenFlags::CREATE | OpenFlags::EXCLUDE | OpenFlags::WRITE,
            )
            .await
            .map_err(|error| transfer_failed(format!("failed to create remote temp file: {error}")))?;

        let max_plus_one = constraints.max_bytes.saturating_add(1);
        let mut limited = (&mut local).take(max_plus_one);
        let bytes = tokio::io::copy(&mut limited, &mut remote)
            .await
            .map_err(|error| transfer_failed(format!("failed while uploading file: {error}")))?;

        if bytes > constraints.max_bytes {
            let _ = remote.shutdown().await;
            let _ = sftp.remove_file(temp_destination.clone()).await;
            let _ = sftp.close().await;
            return Err(too_large(bytes, constraints.max_bytes));
        }

        remote
            .flush()
            .await
            .map_err(|error| transfer_failed(format!("failed to flush remote file: {error}")))?;
        remote
            .sync_all()
            .await
            .map_err(|error| transfer_failed(format!("failed to sync remote file: {error}")))?;
        remote
            .shutdown()
            .await
            .map_err(|error| transfer_failed(format!("failed to close remote file: {error}")))?;

        let destination_exists = sftp
            .try_exists(destination.clone())
            .await
            .map_err(|error| transfer_failed(format!("failed to recheck remote destination: {error}")))?;
        if destination_exists {
            if !transfer.overwrite {
                let _ = sftp.remove_file(temp_destination.clone()).await;
                let _ = sftp.close().await;
                return Err(AppError::new(
                    ErrorCode::DestinationExists,
                    "remote destination appeared during upload and overwrite is disabled",
                ));
            }
            sftp.remove_file(destination.clone()).await.map_err(|error| {
                transfer_failed(format!("failed to replace remote destination: {error}"))
            })?;
        }

        if let Err(error) = sftp
            .rename(temp_destination.clone(), destination.clone())
            .await
        {
            let _ = sftp.remove_file(temp_destination).await;
            let _ = sftp.close().await;
            return Err(transfer_failed(format!(
                "failed to move uploaded temp file into place: {error}"
            )));
        }

        sftp.close()
            .await
            .map_err(|error| transfer_failed(format!("failed to close SFTP session: {error}")))?;

        Ok(TransferResult {
            bytes_transferred: bytes,
        })
    }

    async fn download_inner(
        &self,
        session: &mut SshSession,
        transfer: &TransferSpec,
        constraints: &TransferConstraints,
    ) -> AppResult<TransferResult> {
        reject_local_symlink_destination(&transfer.destination).await?;
        if !transfer.overwrite && fs::try_exists(&transfer.destination).await.map_err(|error| {
            transfer_failed(format!("failed to inspect local destination: {error}"))
        })? {
            return Err(AppError::new(
                ErrorCode::DestinationExists,
                "local destination already exists and overwrite is disabled",
            ));
        }

        let sftp = open_sftp(session, constraints.timeout_seconds).await?;
        let source = authorize_remote_path(
            &sftp,
            &transfer.source,
            &constraints.allowed_remote_roots,
            false,
        )
        .await?;
        let metadata = sftp
            .metadata(source.clone())
            .await
            .map_err(|error| transfer_failed(format!("failed to inspect remote source: {error}")))?;
        if let Some(size) = metadata.size {
            ensure_size_allowed(size, constraints.max_bytes)?;
        }

        let mut remote = sftp
            .open(source)
            .await
            .map_err(|error| transfer_failed(format!("failed to open remote source: {error}")))?;
        let temp_destination = local_temp_path(&transfer.destination);
        let mut local = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_destination)
            .await
            .map_err(|error| {
                transfer_failed(format!(
                    "failed to create local temp file {}: {error}",
                    temp_destination
                ))
            })?;

        let max_plus_one = constraints.max_bytes.saturating_add(1);
        let mut limited = (&mut remote).take(max_plus_one);
        let bytes = tokio::io::copy(&mut limited, &mut local)
            .await
            .map_err(|error| transfer_failed(format!("failed while downloading file: {error}")))?;

        if bytes > constraints.max_bytes {
            let _ = local.shutdown().await;
            let _ = fs::remove_file(&temp_destination).await;
            let _ = remote.shutdown().await;
            let _ = sftp.close().await;
            return Err(too_large(bytes, constraints.max_bytes));
        }

        local
            .flush()
            .await
            .map_err(|error| transfer_failed(format!("failed to flush local file: {error}")))?;
        local
            .sync_all()
            .await
            .map_err(|error| transfer_failed(format!("failed to sync local file: {error}")))?;
        remote
            .shutdown()
            .await
            .map_err(|error| transfer_failed(format!("failed to close remote source: {error}")))?;

        if transfer.overwrite && fs::try_exists(&transfer.destination).await.map_err(|error| {
            transfer_failed(format!("failed to recheck local destination: {error}"))
        })? {
            reject_local_symlink_destination(&transfer.destination).await?;
            fs::remove_file(&transfer.destination).await.map_err(|error| {
                transfer_failed(format!("failed to replace local destination: {error}"))
            })?;
        }

        if let Err(error) = fs::rename(&temp_destination, &transfer.destination).await {
            let _ = fs::remove_file(&temp_destination).await;
            let _ = sftp.close().await;
            return Err(transfer_failed(format!(
                "failed to move downloaded temp file into place: {error}"
            )));
        }

        sftp.close()
            .await
            .map_err(|error| transfer_failed(format!("failed to close SFTP session: {error}")))?;

        Ok(TransferResult {
            bytes_transferred: bytes,
        })
    }
}

impl FileTransfer<SshSession> for SftpFileTransfer {
    fn upload(
        &self,
        session: &mut SshSession,
        transfer: &TransferSpec,
        constraints: TransferConstraints,
    ) -> impl std::future::Future<Output = AppResult<TransferResult>> + Send {
        self.upload_bounded(session, transfer, constraints)
    }

    fn download(
        &self,
        session: &mut SshSession,
        transfer: &TransferSpec,
        constraints: TransferConstraints,
    ) -> impl std::future::Future<Output = AppResult<TransferResult>> + Send {
        self.download_bounded(session, transfer, constraints)
    }
}

async fn open_sftp(session: &mut SshSession, timeout_seconds: u64) -> AppResult<SftpSession> {
    let channel = session.open_session_channel().await.map_err(|error| {
        transfer_failed(format!("failed to open SSH channel for SFTP: {error}"))
    })?;
    channel
        .request_subsystem(true, "sftp")
        .await
        .map_err(|error| transfer_failed(format!("failed to request SFTP subsystem: {error}")))?;

    let sftp = SftpSession::new(channel.into_stream())
        .await
        .map_err(|error| transfer_failed(format!("failed to initialize SFTP session: {error}")))?;
    sftp.set_timeout(timeout_seconds);
    Ok(sftp)
}

async fn authorize_remote_path(
    sftp: &SftpSession,
    raw_path: &str,
    roots: &[String],
    allow_missing_leaf: bool,
) -> AppResult<String> {
    let normalized = normalize_posix_absolute(raw_path)?;
    let mut canonical_roots = Vec::with_capacity(roots.len());
    for root in roots {
        let root = normalize_posix_absolute(root).map_err(|error| {
            AppError::new(
                ErrorCode::InvalidConfiguration,
                format!("invalid configured remote root {root}: {}", error.message),
            )
        })?;
        if root == "/" {
            return Err(AppError::new(
                ErrorCode::InvalidConfiguration,
                "remote root '/' is not allowed because it grants unrestricted filesystem access",
            ));
        }
        let canonical = sftp.canonicalize(root.clone()).await.map_err(|error| {
            AppError::new(
                ErrorCode::InvalidConfiguration,
                format!("failed to canonicalize configured remote root {root}: {error}"),
            )
        })?;
        let canonical = normalize_posix_absolute(&canonical).map_err(|error| {
            AppError::new(ErrorCode::InvalidConfiguration, error.message)
        })?;
        if canonical == "/" {
            return Err(AppError::new(
                ErrorCode::InvalidConfiguration,
                "configured remote root resolves to '/' and is therefore unrestricted",
            ));
        }
        canonical_roots.push(canonical);
    }

    let exists = sftp
        .try_exists(normalized.clone())
        .await
        .map_err(|error| transfer_failed(format!("failed to inspect remote path: {error}")))?;
    let canonical_candidate = if exists {
        sftp.canonicalize(normalized.clone())
            .await
            .map_err(|error| transfer_failed(format!("failed to canonicalize remote path: {error}")))?
    } else if allow_missing_leaf {
        let (parent, leaf) = split_parent_leaf(&normalized)?;
        let canonical_parent = sftp.canonicalize(parent.clone()).await.map_err(|error| {
            transfer_failed(format!("failed to canonicalize remote parent {parent}: {error}"))
        })?;
        join_posix(&normalize_posix_absolute(&canonical_parent)?, &leaf)
    } else {
        return Err(transfer_failed(format!(
            "remote source does not exist: {normalized}"
        )));
    };
    let canonical_candidate = normalize_posix_absolute(&canonical_candidate)?;

    if canonical_roots
        .iter()
        .any(|root| posix_path_within(&canonical_candidate, root))
    {
        Ok(canonical_candidate)
    } else {
        Err(AppError::new(
            ErrorCode::TransferPathDenied,
            format!("remote path is outside configured roots: {raw_path}"),
        ))
    }
}

fn normalize_posix_absolute(path: &str) -> AppResult<String> {
    if path.contains('\0') {
        return Err(AppError::new(
            ErrorCode::TransferPathDenied,
            "remote path must not contain NUL bytes",
        ));
    }
    if !path.starts_with('/') {
        return Err(AppError::new(
            ErrorCode::TransferPathDenied,
            "remote path must be absolute",
        ));
    }

    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                return Err(AppError::new(
                    ErrorCode::TransferPathDenied,
                    "parent path traversal is not allowed",
                ));
            }
            value => parts.push(value),
        }
    }

    if parts.is_empty() {
        Ok("/".to_owned())
    } else {
        Ok(format!("/{}", parts.join("/")))
    }
}

fn split_parent_leaf(path: &str) -> AppResult<(String, String)> {
    let Some(index) = path.rfind('/') else {
        return Err(AppError::new(
            ErrorCode::TransferPathDenied,
            "remote path must be absolute",
        ));
    };
    let leaf = &path[index + 1..];
    if leaf.is_empty() {
        return Err(AppError::new(
            ErrorCode::TransferPathDenied,
            "remote transfer path must identify a file",
        ));
    }
    let parent = if index == 0 { "/" } else { &path[..index] };
    Ok((parent.to_owned(), leaf.to_owned()))
}

fn join_posix(parent: &str, leaf: &str) -> String {
    if parent == "/" {
        format!("/{leaf}")
    } else {
        format!("{}/{leaf}", parent.trim_end_matches('/'))
    }
}

fn posix_path_within(candidate: &str, root: &str) -> bool {
    candidate == root
        || candidate
            .strip_prefix(root)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn ensure_size_allowed(size: u64, max_bytes: u64) -> AppResult<()> {
    if size <= max_bytes {
        return Ok(());
    }
    Err(too_large(size, max_bytes))
}

fn too_large(size: u64, max_bytes: u64) -> AppError {
    AppError::new(
        ErrorCode::TransferTooLarge,
        format!("transfer size {size} exceeds configured maximum {max_bytes}"),
    )
}

fn transfer_failed(message: impl Into<String>) -> AppError {
    AppError::new(ErrorCode::TransferFailed, message)
}

fn unique_suffix() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    format!("{}.{}", process::id(), nanos)
}

fn remote_temp_path(destination: &str) -> String {
    format!("{destination}.remote-exec-mcp.{}.part", unique_suffix())
}

fn local_temp_path(destination: &str) -> String {
    format!("{destination}.remote-exec-mcp.{}.part", unique_suffix())
}

async fn reject_local_symlink_destination(path: &str) -> AppResult<()> {
    match fs::symlink_metadata(path).await {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(AppError::new(
            ErrorCode::InvalidRequest,
            "local destination must not be a symbolic link",
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(transfer_failed(format!(
            "failed to inspect local destination {path}: {error}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_remote_posix_path_without_host_os_semantics() {
        assert_eq!(
            normalize_posix_absolute("/opt//apps/./demo.jar").unwrap(),
            "/opt/apps/demo.jar"
        );
    }

    #[test]
    fn rejects_parent_traversal_and_relative_remote_paths() {
        assert_eq!(
            normalize_posix_absolute("/opt/apps/../secret")
                .unwrap_err()
                .code,
            ErrorCode::TransferPathDenied
        );
        assert_eq!(
            normalize_posix_absolute("opt/apps/demo.jar")
                .unwrap_err()
                .code,
            ErrorCode::TransferPathDenied
        );
    }

    #[test]
    fn root_match_is_component_aware() {
        assert!(posix_path_within("/opt/apps/demo.jar", "/opt/apps"));
        assert!(posix_path_within("/opt/apps", "/opt/apps"));
        assert!(!posix_path_within("/opt/apps2/demo.jar", "/opt/apps"));
    }

    #[test]
    fn size_limit_is_stable_and_fail_closed() {
        assert!(ensure_size_allowed(100, 100).is_ok());
        assert_eq!(
            ensure_size_allowed(101, 100).unwrap_err().code,
            ErrorCode::TransferTooLarge
        );
    }
}
