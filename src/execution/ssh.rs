use std::time::{Duration, Instant};

use russh::ChannelMsg;
use tokio::time::timeout;

use crate::domain::{CommandSpec, ExecutionResult};
use crate::error::{AppError, AppResult, ErrorCode};
use crate::execution::{CommandExecutor, ExecutionLimits};
use crate::transport::ssh::SshSession;

#[derive(Debug, Default, Clone, Copy)]
pub struct SshCommandExecutor;

impl SshCommandExecutor {
    async fn execute_ssh(
        &self,
        session: &mut SshSession,
        command: &CommandSpec,
        limits: ExecutionLimits,
    ) -> AppResult<ExecutionResult> {
        let limits = limits.validate()?;
        let remote_command = render_remote_command(command)?;
        let started = Instant::now();

        let mut channel = session.open_session_channel().await.map_err(|error| {
            AppError::new(
                ErrorCode::ExecutionFailed,
                format!("failed to open SSH command channel: {error}"),
            )
        })?;

        let duration = Duration::from_secs(limits.timeout_seconds);
        let execution = timeout(duration, async {
            channel
                .exec(true, remote_command.into_bytes())
                .await
                .map_err(|error| {
                    AppError::new(
                        ErrorCode::ExecutionFailed,
                        format!("failed to request SSH command execution: {error}"),
                    )
                })?;

            // The command contract has no stdin. Sending EOF avoids leaving a
            // remote process blocked waiting for input.
            let _ = channel.eof().await;

            let mut stdout = BoundedOutput::new(limits.max_stdout_bytes);
            let mut stderr = BoundedOutput::new(limits.max_stderr_bytes);
            let mut exit_code = None;
            let mut terminated_by_signal = false;

            while let Some(message) = channel.wait().await {
                match message {
                    ChannelMsg::Data { data } => stdout.push(data.as_ref()),
                    ChannelMsg::ExtendedData { ext, data } if ext == 1 => {
                        stderr.push(data.as_ref())
                    }
                    ChannelMsg::ExitStatus { exit_status } => {
                        exit_code = i32::try_from(exit_status).ok();
                    }
                    ChannelMsg::ExitSignal { .. } => {
                        terminated_by_signal = true;
                        exit_code = None;
                    }
                    ChannelMsg::Failure => {
                        return Err(AppError::new(
                            ErrorCode::ExecutionFailed,
                            "remote SSH exec request was rejected",
                        ));
                    }
                    ChannelMsg::Close => break,
                    _ => {}
                }
            }

            let (stdout, stdout_truncated) = stdout.finish();
            let (stderr, stderr_truncated) = stderr.finish();
            let success = !terminated_by_signal && exit_code == Some(0);

            Ok(ExecutionResult {
                success,
                exit_code,
                stdout,
                stderr,
                duration_ms: started.elapsed().as_millis(),
                stdout_truncated,
                stderr_truncated,
            })
        })
        .await;

        match execution {
            Ok(result) => result,
            Err(_) => {
                // Best-effort channel close bounds this executor's ownership.
                // Stronger remote-process cancellation remains a separate
                // production-hardening capability.
                let _ = channel.close().await;
                Err(AppError::new(
                    ErrorCode::ExecutionTimeout,
                    format!(
                        "remote command exceeded execution timeout of {} seconds",
                        limits.timeout_seconds
                    ),
                ))
            }
        }
    }
}

impl CommandExecutor<SshSession> for SshCommandExecutor {
    fn execute(
        &self,
        session: &mut SshSession,
        command: &CommandSpec,
        limits: ExecutionLimits,
    ) -> impl std::future::Future<Output = AppResult<ExecutionResult>> + Send {
        self.execute_ssh(session, command, limits)
    }
}

fn render_remote_command(command: &CommandSpec) -> AppResult<String> {
    if command.program.is_empty() {
        return Err(AppError::new(
            ErrorCode::InvalidTaskDefinition,
            "command program must not be empty",
        ));
    }

    if command.program.contains('\0') || command.args.iter().any(|arg| arg.contains('\0')) {
        return Err(AppError::new(
            ErrorCode::InvalidTaskDefinition,
            "command program and arguments must not contain NUL bytes",
        ));
    }

    let mut rendered = shell_quote(&command.program);
    for arg in &command.args {
        rendered.push(' ');
        rendered.push_str(&shell_quote(arg));
    }

    Ok(rendered)
}

fn shell_quote(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('\'');

    for character in value.chars() {
        if character == '\'' {
            quoted.push_str("'\"'\"'");
        } else {
            quoted.push(character);
        }
    }

    quoted.push('\'');
    quoted
}

#[derive(Debug)]
struct BoundedOutput {
    bytes: Vec<u8>,
    limit: usize,
    truncated: bool,
}

impl BoundedOutput {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::with_capacity(limit.min(4096)),
            limit,
            truncated: false,
        }
    }

    fn push(&mut self, data: &[u8]) {
        let remaining = self.limit.saturating_sub(self.bytes.len());
        let take = remaining.min(data.len());
        self.bytes.extend_from_slice(&data[..take]);
        self.truncated |= take < data.len();
    }

    fn finish(self) -> (String, bool) {
        (
            String::from_utf8_lossy(&self.bytes).into_owned(),
            self.truncated,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_program_and_argv_without_raw_shell_concatenation() {
        let command = CommandSpec {
            program: "printf".to_owned(),
            args: vec!["%s".to_owned(), "a;$(id)".to_owned(), "x'y".to_owned()],
        };

        assert_eq!(
            render_remote_command(&command).unwrap(),
            r#"'printf' '%s' 'a;$(id)' 'x'"'"'y'"#
        );
    }

    #[test]
    fn rejects_nul_in_structured_command() {
        let command = CommandSpec {
            program: "printf".to_owned(),
            args: vec!["bad\0arg".to_owned()],
        };

        let error = render_remote_command(&command).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidTaskDefinition);
    }

    #[test]
    fn bounded_output_keeps_prefix_and_marks_truncation() {
        let mut output = BoundedOutput::new(5);
        output.push(b"abc");
        output.push(b"def");

        let (text, truncated) = output.finish();
        assert_eq!(text, "abcde");
        assert!(truncated);
    }

    #[test]
    fn zero_sized_output_is_bounded() {
        let mut output = BoundedOutput::new(0);
        output.push(b"data");

        let (text, truncated) = output.finish();
        assert!(text.is_empty());
        assert!(truncated);
    }
}
