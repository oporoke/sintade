use std::path::Path;
use std::process::{Output, Stdio};
use std::time::Duration;

use std::ffi::OsString;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::Command;

use crate::domain::probe::ProbeReport;
use crate::domain::transcode::{Progress, read_progress_line};

/// Where FFmpeg and ffprobe are (`FFMPEG_PATH`, `FFPROBE_PATH`).
#[derive(Debug, Clone)]
pub struct MediaTools {
    pub ffmpeg: String,
    pub ffprobe: String,
}

impl Default for MediaTools {
    fn default() -> Self {
        Self {
            ffmpeg: "ffmpeg".to_string(),
            ffprobe: "ffprobe".to_string(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("could not start {tool}: {source}")]
    Spawn {
        tool: String,
        source: std::io::Error,
    },

    #[error("{tool} timed out after {after:?}")]
    TimedOut { tool: String, after: Duration },

    #[error("{tool} printed output media can't read: {reason}")]
    BadOutput { tool: String, reason: String },

    #[error("{tool} reported no progress for {after:?}")]
    Stalled { tool: String, after: Duration },

    #[error("{tool} failed ({status}): {stderr_tail}")]
    Failed {
        tool: String,
        status: String,
        stderr_tail: String,
    },
}

/// Runs `command` to completion, or kills it after `timeout` (`kill_on_drop`: the child dies
/// with the future, so a timed-out or cancelled job leaves no FFmpeg running).
pub async fn run(mut command: Command, timeout: Duration) -> Result<Output, ToolError> {
    let tool = command
        .as_std()
        .get_program()
        .to_string_lossy()
        .into_owned();
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let child = command.spawn().map_err(|source| ToolError::Spawn {
        tool: tool.clone(),
        source,
    })?;
    match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(result) => result.map_err(|source| ToolError::Spawn { tool, source }),
        Err(_) => Err(ToolError::TimedOut {
            tool,
            after: timeout,
        }),
    }
}

/// ffprobe gets this long; it reads headers and stream info, not the whole file.
const PROBE_TIMEOUT: Duration = Duration::from_secs(60);

/// What ffprobe made of a file.
#[derive(Debug)]
pub enum Probed {
    Readable(ProbeReport),
    /// ffprobe couldn't read it as media at all; `detail` is its first error line (for logs).
    Unreadable {
        detail: String,
    },
}

pub async fn probe(tools: &MediaTools, path: &Path) -> Result<Probed, ToolError> {
    let mut command = Command::new(&tools.ffprobe);
    command
        .args([
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
        ])
        .arg(path);
    let output = run(command, PROBE_TIMEOUT).await?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = stderr
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("ffprobe failed")
            .to_string();
        return Ok(Probed::Unreadable { detail });
    }
    serde_json::from_slice(&output.stdout)
        .map(Probed::Readable)
        .map_err(|error| ToolError::BadOutput {
            tool: tools.ffprobe.clone(),
            reason: error.to_string(),
        })
}

/// FFmpeg is killed if it reports no progress for this long (a hung decoder, a stuck disk).
pub const FFMPEG_STALL: Duration = Duration::from_secs(120);
/// How much of FFmpeg's stderr is kept for the error message.
const STDERR_TAIL_LINES: usize = 8;

/// Runs FFmpeg with `args` (which must include `-progress pipe:1`), logging progress against
/// `expected_ms` (the recording's length) and killing it if it stalls for [`FFMPEG_STALL`] or
/// runs past `deadline`.
pub async fn run_ffmpeg(
    tools: &MediaTools,
    args: Vec<OsString>,
    expected_ms: u32,
    deadline: Duration,
) -> Result<(), ToolError> {
    run_ffmpeg_with(tools, args, expected_ms, deadline, FFMPEG_STALL).await
}

pub async fn run_ffmpeg_with(
    tools: &MediaTools,
    args: Vec<OsString>,
    expected_ms: u32,
    deadline: Duration,
    stall: Duration,
) -> Result<(), ToolError> {
    let tool = tools.ffmpeg.clone();
    let mut child = Command::new(&tools.ffmpeg)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|source| ToolError::Spawn {
            tool: tool.clone(),
            source,
        })?;
    let stdout = child.stdout.take().ok_or_else(|| ToolError::BadOutput {
        tool: tool.clone(),
        reason: "no stdout".to_string(),
    })?;
    let mut stderr = child.stderr.take().ok_or_else(|| ToolError::BadOutput {
        tool: tool.clone(),
        reason: "no stderr".to_string(),
    })?;
    // Drain stderr concurrently (a full pipe would block FFmpeg), keeping its tail.
    let stderr_task = tokio::spawn(async move {
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text).await;
        let lines: Vec<&str> = text
            .lines()
            .filter(|line| !line.trim().is_empty())
            .collect();
        lines[lines.len().saturating_sub(STDERR_TAIL_LINES)..].join(" | ")
    });

    let watch = async {
        let mut lines = BufReader::new(stdout).lines();
        let mut progress = Progress::default();
        let mut logged_tenth = 0u64;
        loop {
            match tokio::time::timeout(stall, lines.next_line()).await {
                Err(_) => {
                    return Err(ToolError::Stalled {
                        tool: tool.clone(),
                        after: stall,
                    });
                }
                Ok(Ok(Some(line))) => {
                    if read_progress_line(&line, &mut progress) && expected_ms > 0 {
                        let tenth = progress.out_time_ms * 10 / u64::from(expected_ms);
                        if tenth > logged_tenth && !progress.done {
                            logged_tenth = tenth;
                            tracing::info!(
                                percent = (tenth * 10).min(100),
                                out_time_ms = progress.out_time_ms,
                                "ffmpeg progress"
                            );
                        }
                    }
                }
                Ok(Ok(None)) => return Ok(()), // stdout closed: FFmpeg is exiting
                Ok(Err(error)) => {
                    return Err(ToolError::BadOutput {
                        tool: tool.clone(),
                        reason: error.to_string(),
                    });
                }
            }
        }
    };
    match tokio::time::timeout(deadline, watch).await {
        Err(_) => {
            return Err(ToolError::TimedOut {
                tool,
                after: deadline,
            });
        }
        Ok(Err(error)) => return Err(error),
        Ok(Ok(())) => {}
    }
    let status = child.wait().await.map_err(|source| ToolError::Spawn {
        tool: tool.clone(),
        source,
    })?;
    let stderr_tail = stderr_task.await.unwrap_or_default();
    if status.success() {
        Ok(())
    } else {
        Err(ToolError::Failed {
            tool,
            status: status.to_string(),
            stderr_tail,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::ffmpeg_available;

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    #[tokio::test]
    async fn ffmpeg_runs_to_completion_with_progress() {
        if !ffmpeg_available().await {
            return;
        }
        run_ffmpeg(
            &MediaTools::default(),
            os(&[
                "-hide_banner",
                "-nostdin",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=160x90:duration=2",
                "-progress",
                "pipe:1",
                "-nostats",
                "-f",
                "null",
                "-",
            ]),
            2_000,
            Duration::from_secs(60),
        )
        .await
        .expect("ffmpeg ran");
    }

    #[tokio::test]
    async fn a_failing_ffmpeg_reports_its_stderr() {
        if !ffmpeg_available().await {
            return;
        }
        let result = run_ffmpeg(
            &MediaTools::default(),
            os(&[
                "-hide_banner",
                "-nostdin",
                "-i",
                "/nonexistent/in.webm",
                "-progress",
                "pipe:1",
                "out.mp4",
            ]),
            1_000,
            Duration::from_secs(60),
        )
        .await;
        let Err(ToolError::Failed { stderr_tail, .. }) = result else {
            panic!("expected a failure, got {result:?}");
        };
        assert!(stderr_tail.contains("No such file"), "{stderr_tail}");
    }

    #[tokio::test]
    async fn a_stalled_ffmpeg_is_killed() {
        if !ffmpeg_available().await {
            return;
        }
        // Reads a live source in real time: no progress block within 300 ms of starting.
        let started = std::time::Instant::now();
        let result = run_ffmpeg_with(
            &MediaTools::default(),
            os(&[
                "-hide_banner",
                "-nostdin",
                "-re",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=duration=30",
                "-progress",
                "pipe:1",
                "-stats_period",
                "10",
                "-f",
                "null",
                "-",
            ]),
            30_000,
            Duration::from_secs(60),
            Duration::from_millis(300),
        )
        .await;
        assert!(
            matches!(result, Err(ToolError::Stalled { .. })),
            "{result:?}"
        );
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[tokio::test]
    async fn a_slow_tool_is_killed_at_its_timeout() {
        let mut command = Command::new("sleep");
        command.arg("5");
        let started = std::time::Instant::now();
        let result = run(command, Duration::from_millis(200)).await;
        assert!(
            matches!(result, Err(ToolError::TimedOut { .. })),
            "{result:?}"
        );
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[tokio::test]
    async fn a_missing_tool_is_a_spawn_error() {
        let tools = MediaTools {
            ffprobe: "/nonexistent/ffprobe".to_string(),
            ..MediaTools::default()
        };
        let result = probe(&tools, Path::new("x.webm")).await;
        assert!(matches!(result, Err(ToolError::Spawn { .. })), "{result:?}");
    }
}
