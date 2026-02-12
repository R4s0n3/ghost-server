use std::{
    path::Path,
    process::Stdio,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use anyhow::{anyhow, Context};
use regex::Regex;
use serde::Serialize;
use tokio::{process::Command, time::timeout};

static HAS_LOGGED_PDFINFO_FALLBACK: AtomicBool = AtomicBool::new(false);
static GHOSTSCRIPT_COMMAND_TIMEOUT: once_cell::sync::Lazy<Duration> =
    once_cell::sync::Lazy::new(|| {
        let timeout_ms = std::env::var("GHOSTSCRIPT_COMMAND_TIMEOUT_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(120_000);
        Duration::from_millis(timeout_ms)
    });

#[derive(Debug, Clone, Serialize)]
pub struct ColorProfile {
    pub page: i64,
    pub c: f64,
    pub m: f64,
    pub y: f64,
    pub k: f64,
    #[serde(rename = "type")]
    pub ink_type: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PdfAnalysis {
    pub file_name: String,
    pub page_count: i64,
    pub has_formfields: bool,
    #[serde(rename = "colorProfiles")]
    pub color_profiles: Vec<ColorProfile>,
}

pub async fn run_command(program: &str, args: &[String]) -> anyhow::Result<(String, String)> {
    let child = Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("failed to execute {}", program))?;
    let output = timeout(*GHOSTSCRIPT_COMMAND_TIMEOUT, child.wait_with_output())
        .await
        .map_err(|_| {
            anyhow!(
                "{} timed out after {} ms",
                program,
                GHOSTSCRIPT_COMMAND_TIMEOUT.as_millis()
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

pub async fn get_pdf_page_count(file_path: &Path) -> anyhow::Result<i64> {
    if let Some(count) = try_get_pdf_page_count_with_pdfinfo(file_path).await? {
        return Ok(count);
    }

    let file_path_str = file_path.to_string_lossy().to_string();
    let args = vec![
        "-q".to_string(),
        "-dNODISPLAY".to_string(),
        "-dSAFER".to_string(),
        format!("--permit-file-read={}", file_path_str),
        "-c".to_string(),
        format!(
            "({}) (r) file runpdfbegin pdfpagecount = quit",
            file_path_str
        ),
    ];

    let (stdout, stderr) = run_command("gs", &args).await?;
    let raw = if stdout.trim().is_empty() {
        stderr.trim()
    } else {
        stdout.trim()
    };

    let page_count = raw
        .parse::<i64>()
        .map_err(|_| anyhow!("Invalid page count reported by Ghostscript."))?;

    if page_count <= 0 {
        return Err(anyhow!("Invalid page count reported by Ghostscript."));
    }

    Ok(page_count)
}

pub async fn analyze_pdf(
    file_path: &Path,
    page_count_override: Option<i64>,
) -> anyhow::Result<PdfAnalysis> {
    let file_path_str = file_path.to_string_lossy().to_string();

    let page_count = match page_count_override {
        Some(value) => value,
        None => get_pdf_page_count(file_path).await?,
    };

    let inkcov_args = vec![
        "-q".to_string(),
        "-o".to_string(),
        "-".to_string(),
        "-dSAFER".to_string(),
        "-dBATCH".to_string(),
        "-dNOPAUSE".to_string(),
        "-sDEVICE=inkcov".to_string(),
        file_path_str.clone(),
    ];
    let (inkcov_stdout, inkcov_stderr) = run_command("gs", &inkcov_args).await?;
    let inkcov_output = if inkcov_stderr.trim().is_empty() {
        inkcov_stdout
    } else if inkcov_stdout.trim().is_empty() {
        inkcov_stderr
    } else {
        format!("{}\n{}", inkcov_stdout, inkcov_stderr)
    };

    let mut color_profiles = parse_inkcov_profiles(&inkcov_output, page_count);
    if color_profiles.len() != page_count as usize {
        let sample = inkcov_output.chars().take(600).collect::<String>();
        tracing::warn!(
            expected = page_count,
            parsed = color_profiles.len(),
            sample = %sample,
            "inkcov output did not contain one profile per page; normalizing parsed data"
        );
        color_profiles = normalize_profiles(color_profiles, page_count);
    }

    // Avoid a second Ghostscript pass here. Some PDFs can hang on dDumpAnnots.
    // A raw byte scan is fast and works for our current form-field signal.
    let has_formfields = match tokio::fs::read(file_path).await {
        Ok(bytes) => bytes
            .windows(15)
            .any(|window| window == b"/Subtype /Widget"),
        Err(error) => {
            tracing::warn!(error = %error, "failed to read PDF for form-field detection");
            false
        }
    };

    let file_name = file_path
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| "document.pdf".to_string());

    Ok(PdfAnalysis {
        file_name,
        page_count,
        has_formfields,
        color_profiles,
    })
}

pub async fn convert_pdf_to_grayscale_file(
    input_path: &Path,
    output_path: &Path,
) -> anyhow::Result<()> {
    let args = vec![
        "-q".to_string(),
        "-dNOPAUSE".to_string(),
        "-dBATCH".to_string(),
        "-dSAFER".to_string(),
        "-sDEVICE=pdfwrite".to_string(),
        "-sColorConversionStrategy=Gray".to_string(),
        "-dProcessColorModel=/DeviceGray".to_string(),
        format!("-sOutputFile={}", output_path.to_string_lossy()),
        input_path.to_string_lossy().to_string(),
    ];

    run_command("gs", &args).await.map(|_| ())
}

pub fn sanitize_base_name(value: &str) -> String {
    static NON_SAFE_RE: once_cell::sync::Lazy<Regex> =
        once_cell::sync::Lazy::new(|| Regex::new(r"[^a-zA-Z0-9_-]+").expect("valid regex"));
    static EDGE_UNDERSCORE_RE: once_cell::sync::Lazy<Regex> =
        once_cell::sync::Lazy::new(|| Regex::new(r"^_+|_+$").expect("valid regex"));

    let replaced = NON_SAFE_RE.replace_all(value, "_");
    let trimmed = EDGE_UNDERSCORE_RE.replace_all(&replaced, "");
    let output = trimmed.to_string();
    if output.is_empty() {
        "document".to_string()
    } else {
        output.chars().take(80).collect()
    }
}

async fn try_get_pdf_page_count_with_pdfinfo(file_path: &Path) -> anyhow::Result<Option<i64>> {
    let args = vec![file_path.to_string_lossy().to_string()];
    let output = Command::new("pdfinfo").args(args).output().await;
    let output = match output {
        Ok(output) => output,
        Err(error) => {
            log_pdfinfo_fallback(&format!("spawn failed: {}", error));
            return Ok(None);
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let reason = if stderr.trim().is_empty() {
            format!("exit={}", output.status)
        } else {
            stderr.trim().to_string()
        };
        log_pdfinfo_fallback(&reason);
        return Ok(None);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let pages_regex = Regex::new(r"(?m)^\s*Pages:\s+(\d+)\s*$").expect("valid regex");
    let captures = match pages_regex.captures(&stdout) {
        Some(captures) => captures,
        None => {
            log_pdfinfo_fallback("missing Pages field in pdfinfo output");
            return Ok(None);
        }
    };

    let page_count = captures
        .get(1)
        .and_then(|value| value.as_str().parse::<i64>().ok());

    let page_count = match page_count {
        Some(value) if value > 0 => value,
        _ => {
            log_pdfinfo_fallback("invalid Pages value in pdfinfo output");
            return Ok(None);
        }
    };

    Ok(Some(page_count))
}

fn log_pdfinfo_fallback(reason: &str) {
    if HAS_LOGGED_PDFINFO_FALLBACK.swap(true, Ordering::SeqCst) {
        return;
    }
    tracing::info!(
        reason = reason,
        "pdfinfo page-count fast path unavailable; falling back to Ghostscript page counting"
    );
}

fn parse_inkcov_profiles(output: &str, page_count: i64) -> Vec<ColorProfile> {
    let mut profiles = Vec::new();
    for line in output.lines() {
        if let Some((c, m, y, k, ink_type)) = parse_inkcov_line(line) {
            let page = profiles.len() as i64 + 1;
            if page > page_count {
                break;
            }
            profiles.push(ColorProfile {
                page,
                c,
                m,
                y,
                k,
                ink_type,
            });
        }
    }

    profiles
}

fn parse_inkcov_line(line: &str) -> Option<(f64, f64, f64, f64, String)> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    if tokens.len() < 4 {
        return None;
    }

    let mut last_match: Option<(usize, f64, f64, f64, f64)> = None;
    for i in 0..=tokens.len().saturating_sub(4) {
        let c = parse_f64_token(tokens[i]);
        let m = parse_f64_token(tokens[i + 1]);
        let y = parse_f64_token(tokens[i + 2]);
        let k = parse_f64_token(tokens[i + 3]);
        if let (Some(c), Some(m), Some(y), Some(k)) = (c, m, y, k) {
            last_match = Some((i, c, m, y, k));
        }
    }

    let (index, c, m, y, k) = last_match?;
    let ink_type = if index + 4 < tokens.len() {
        tokens[index + 4..].join(" ")
    } else {
        String::new()
    };
    Some((c, m, y, k, ink_type))
}

fn parse_f64_token(token: &str) -> Option<f64> {
    if let Ok(value) = token.parse::<f64>() {
        return Some(value);
    }
    if token.contains(',') && !token.contains('.') {
        return token.replace(',', ".").parse::<f64>().ok();
    }
    None
}

fn normalize_profiles(mut profiles: Vec<ColorProfile>, page_count: i64) -> Vec<ColorProfile> {
    let expected = page_count.max(0) as usize;

    if profiles.len() > expected {
        profiles.truncate(expected);
    }

    for (index, profile) in profiles.iter_mut().enumerate() {
        profile.page = index as i64 + 1;
    }

    while profiles.len() < expected {
        profiles.push(ColorProfile {
            page: profiles.len() as i64 + 1,
            c: 0.0,
            m: 0.0,
            y: 0.0,
            k: 0.0,
            ink_type: String::new(),
        });
    }

    profiles
}
