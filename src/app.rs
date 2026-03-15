use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use futures::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use sha2::{Digest, Sha256};
use tokio::fs;
use tokio::sync::Semaphore;

use crate::cache::Cache;
use crate::cli::Cli;
use crate::error::AppError;
use crate::markdown::{assemble_markdown, clean_markdown};
use crate::pdf::{encode_page_as_webp, inspect_pdf, rasterize_page};
use crate::server::OcrClient;

#[derive(Debug, Clone)]
pub struct RunSummary {
    pub output_path: PathBuf,
    pub pages_written: usize,
    pub partial: bool,
}

enum PageOutcome {
    Completed { page_number: u32, markdown: String },
    Skipped { page_number: u32 },
}

pub async fn run(cli: Cli) -> Result<RunSummary, AppError> {
    let output_dir = cli
        .output
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    fs::create_dir_all(output_dir).await?;

    let cache = Cache::new(!cli.no_cache).await?;
    let client = OcrClient::new(cli.server.clone())?;
    let pdf = inspect_pdf(&cli.input)?;

    if pdf.page_count == 0 {
        return Err(AppError::PdfProcessingError(
            "PDF enthaelt keine verarbeitbaren Seiten".into(),
        ));
    }

    let progress = ProgressBar::new(pdf.page_count as u64);
    progress.set_style(
        ProgressStyle::with_template(
            "{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {pos}/{len} {msg}",
        )
        .expect("valid progress template"),
    );
    progress.set_message("Warte auf Server...");

    client.health_check().await?;

    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_listener = spawn_shutdown_listener(shutdown.clone());
    let semaphore = Arc::new(Semaphore::new(cli.concurrent.max(1)));
    let progress_for_tasks = progress.clone();
    let input = cli.input.clone();
    let model = cli.model.clone();
    let cache_for_tasks = cache.clone();
    let client_for_tasks = client.clone();

    let stream = futures::stream::iter(pdf.pages.clone())
        .map(|page| {
            let input = input.clone();
            let model = model.clone();
            let cache = cache_for_tasks.clone();
            let client = client_for_tasks.clone();
            let semaphore = semaphore.clone();
            let shutdown = shutdown.clone();
            let progress = progress_for_tasks.clone();

            async move {
                if shutdown.load(Ordering::SeqCst) {
                    return Ok::<PageOutcome, AppError>(PageOutcome::Skipped {
                        page_number: page.number,
                    });
                }

                progress.set_message(format!("Rasterisiere Seite {}", page.number));
                let image = rasterize_page(&input, &page).await?;
                let encoded = encode_page_as_webp(&image)?;
                let hash = hash_page(&encoded.webp_bytes);

                if let Some(markdown) = cache.load(&hash).await? {
                    return Ok::<PageOutcome, AppError>(PageOutcome::Completed {
                        page_number: page.number,
                        markdown,
                    });
                }

                let _permit = semaphore.acquire().await.expect("semaphore closed");

                if shutdown.load(Ordering::SeqCst) {
                    return Ok::<PageOutcome, AppError>(PageOutcome::Skipped {
                        page_number: page.number,
                    });
                }

                progress.set_message(format!(
                    "Generiere Text fuer Seite {} ({}x{})",
                    page.number, encoded.width, encoded.height
                ));
                let markdown = client
                    .ocr_markdown(
                        &model,
                        page.number,
                        &encoded.base64_webp,
                        &encoded.base64_png,
                    )
                    .await?;
                let markdown = clean_markdown(&markdown);
                cache.save(&hash, &markdown).await?;

                Ok::<PageOutcome, AppError>(PageOutcome::Completed {
                    page_number: page.number,
                    markdown,
                })
            }
        })
        .buffer_unordered(cli.concurrent.max(1));

    tokio::pin!(stream);

    let mut completed_pages = BTreeMap::new();

    while let Some(result) = stream.next().await {
        match result? {
            PageOutcome::Completed {
                page_number,
                markdown,
            } => {
                completed_pages.insert(page_number, markdown);
                progress.inc(1);
            }
            PageOutcome::Skipped { page_number } => {
                progress.set_message(format!("Seite {page_number} uebersprungen wegen Shutdown"));
            }
        }
    }

    shutdown_listener.abort();

    let ordered_pages = completed_pages.into_iter().collect::<Vec<_>>();
    let partial = shutdown.load(Ordering::SeqCst) && ordered_pages.len() < pdf.page_count;
    let output_path = if partial {
        partial_output_path(&cli.output)
    } else {
        cli.output.clone()
    };

    progress.set_message("Schreibe Markdown...");
    let markdown = assemble_markdown(&pdf, &cli.input, &ordered_pages);
    fs::write(&output_path, markdown).await?;
    progress.finish_with_message(if partial {
        "Partial-Markdown gespeichert"
    } else {
        "Markdown gespeichert"
    });

    Ok(RunSummary {
        output_path,
        pages_written: ordered_pages.len(),
        partial,
    })
}

fn hash_page(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn partial_output_path(output: &Path) -> PathBuf {
    let file_name = output
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("output.md");
    let partial_name = if let Some((stem, extension)) = file_name.rsplit_once('.') {
        format!("{stem}~partial.{extension}")
    } else {
        format!("{file_name}~partial.md")
    };

    output.with_file_name(partial_name)
}

fn spawn_shutdown_listener(shutdown: Arc<AtomicBool>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        shutdown.store(true, Ordering::SeqCst);
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::partial_output_path;

    #[test]
    fn appends_partial_suffix_before_extension() {
        let partial = partial_output_path(Path::new("notes/output.md"));
        assert_eq!(partial, Path::new("notes/output~partial.md"));
    }
}
