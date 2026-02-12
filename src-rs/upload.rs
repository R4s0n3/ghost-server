use std::{path::PathBuf, time::SystemTime};

use axum::extract::Multipart;
use thiserror::Error;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct UploadedFile {
    pub temp_path: PathBuf,
    pub original_name: String,
}

#[derive(Debug, Error)]
pub enum UploadError {
    #[error("File not found")]
    MissingFile,
    #[error("Only PDF files are supported")]
    UnsupportedFileType,
    #[error("File is too large")]
    FileTooLarge,
    #[error("Failed to parse upload")]
    MultipartError,
    #[error("Failed to persist upload")]
    IoError,
}

pub async fn save_pdf_from_multipart(
    mut multipart: Multipart,
    max_size_bytes: usize,
) -> Result<UploadedFile, UploadError> {
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| UploadError::MultipartError)?
    {
        if field.name() != Some("file") {
            continue;
        }

        let original_name = field
            .file_name()
            .map(ToString::to_string)
            .unwrap_or_else(|| "document.pdf".to_string());
        let mime_type = field.content_type().map(ToString::to_string);

        let is_pdf = mime_type.as_deref() == Some("application/pdf")
            || original_name.to_ascii_lowercase().ends_with(".pdf");

        if !is_pdf {
            return Err(UploadError::UnsupportedFileType);
        }

        let temp_path = std::env::temp_dir().join(format!(
            "ghost-upload-{}-{}.pdf",
            Uuid::new_v4(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|duration| duration.as_millis())
                .unwrap_or(0)
        ));

        let mut file = tokio::fs::File::create(&temp_path)
            .await
            .map_err(|_| UploadError::IoError)?;

        let mut total_size = 0usize;
        let mut field = field;
        while let Some(chunk) = field
            .chunk()
            .await
            .map_err(|_| UploadError::MultipartError)?
        {
            total_size += chunk.len();
            if total_size > max_size_bytes {
                let _ = tokio::fs::remove_file(&temp_path).await;
                return Err(UploadError::FileTooLarge);
            }
            file.write_all(&chunk)
                .await
                .map_err(|_| UploadError::IoError)?;
        }

        file.flush().await.map_err(|_| UploadError::IoError)?;

        return Ok(UploadedFile {
            temp_path,
            original_name,
        });
    }

    Err(UploadError::MissingFile)
}

pub async fn remove_file_if_exists(path: &PathBuf) {
    if let Err(error) = tokio::fs::remove_file(path).await {
        if error.kind() != std::io::ErrorKind::NotFound {
            tracing::error!(path = %path.display(), error = %error, "failed to delete temp file");
        }
    }
}
