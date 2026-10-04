use std::path::Path;
use std::process::{Output, Stdio};
use std::time::Duration;

use tokio::process::Command;

use crate::domain::probe::ProbeReport;

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

#[cfg(test)]
mod tests {
    use super::*;

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
