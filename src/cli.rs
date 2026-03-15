use std::path::PathBuf;

use clap::{Parser, value_parser};

#[derive(Debug, Clone, Parser)]
#[command(
    name = "pdf-ocr",
    about = "OCR-Orchestrator fuer PDF-zu-Markdown ueber einen lokalen DeepSeek-HTTP-Server"
)]
pub struct Cli {
    #[arg(value_name = "PDF")]
    pub input: PathBuf,

    #[arg(short, long, value_name = "MARKDOWN")]
    pub output: PathBuf,

    #[arg(long, default_value = "http://localhost:8000/v1")]
    pub server: String,

    #[arg(long, default_value = "deepseek-ocr")]
    pub model: String,

    #[arg(long, default_value_t = 1, value_parser = value_parser!(usize))]
    pub concurrent: usize,

    #[arg(long)]
    pub no_cache: bool,
}
