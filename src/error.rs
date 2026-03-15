use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Server nicht erreichbar unter {0}. Laeuft deepseek-ocr.rs im Server-Modus?")]
    ServerUnreachable(String),
    #[error("PDF Fehler: {0}")]
    PdfProcessingError(String),
    #[error("Inferenz fehlgeschlagen: {0}")]
    InferenceError(String),
    #[error("Timeout bei Seite {0}")]
    Timeout(u32),
    #[error("I/O Fehler: {0}")]
    Io(#[from] std::io::Error),
    #[error("HTTP Fehler: {0}")]
    Http(#[from] reqwest::Error),
    #[error("JSON Fehler: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Bildverarbeitung fehlgeschlagen: {0}")]
    Image(#[from] image::ImageError),
    #[error("Kein Cache-Verzeichnis verfuegbar")]
    CacheDirUnavailable,
}
