use std::{path::Path, process::Stdio, time::Duration};

use anyhow::{anyhow, Context};
use tokio::{process::Command, time::timeout};

static MUTOOL_COMMAND_TIMEOUT: once_cell::sync::Lazy<Duration> =
    once_cell::sync::Lazy::new(|| {
        let timeout_ms = std::env::var("MUTOOL_COMMAND_TIMEOUT_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(120_000);
        Duration::from_millis(timeout_ms)
    });

pub async fn convert_pdf_to_grayscale_with_mupdf(
    input_path: &Path,
    output_path: &Path,
) -> anyhow::Result<()> {
    let program = std::env::var("MUTOOL_BIN").unwrap_or_else(|_| "mutool".to_string());
    let args = vec![
        "recolor".to_string(),
        "-c".to_string(),
        "gray".to_string(),
        "-o".to_string(),
        output_path.to_string_lossy().to_string(),
        input_path.to_string_lossy().to_string(),
    ];

    run_command(&program, &args).await.map(|_| ())
}

pub async fn ensure_mutool_recolor_support() -> anyhow::Result<()> {
    let program = std::env::var("MUTOOL_BIN").unwrap_or_else(|_| "mutool".to_string());
    let child = Command::new(&program)
        .arg("recolor")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                return anyhow!("mutool-not-found");
            }
            anyhow!(error).context(format!("failed to execute {}", program))
        })?;
    let output = timeout(*MUTOOL_COMMAND_TIMEOUT, child.wait_with_output())
        .await
        .map_err(|_| {
            anyhow!(
                "{} timed out after {} ms",
                program,
                MUTOOL_COMMAND_TIMEOUT.as_millis()
            )
        })?
        .with_context(|| format!("failed to execute {}", program))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_lowercase();
    let stderr = String::from_utf8_lossy(&output.stderr).to_lowercase();
    if stdout.contains("usage: mutool recolor") || stderr.contains("usage: mutool recolor") {
        return Ok(());
    }

    Err(anyhow!(
        "mutool-recolor-not-supported: install a mutool build that includes the `recolor` command"
    ))
}

async fn run_command(program: &str, args: &[String]) -> anyhow::Result<(String, String)> {
    let child = Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                return anyhow!("mutool-not-found");
            }
            anyhow!(error).context(format!("failed to execute {}", program))
        })?;
    let output = timeout(*MUTOOL_COMMAND_TIMEOUT, child.wait_with_output())
        .await
        .map_err(|_| {
            anyhow!(
                "{} timed out after {} ms",
                program,
                MUTOOL_COMMAND_TIMEOUT.as_millis()
            )
        })?
        .with_context(|| format!("failed to execute {}", program))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        let message = stderr.trim();
        let fallback = stdout.trim();
        let reason = if message.is_empty() {
            if fallback.is_empty() {
                format!("{} failed with status {}", program, output.status)
            } else {
                fallback.to_string()
            }
        } else {
            message.to_string()
        };

        return Err(anyhow!(reason));
    }

    Ok((stdout, stderr))
}
