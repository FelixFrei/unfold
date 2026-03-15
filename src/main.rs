mod app;
mod cache;
mod cli;
mod error;
mod markdown;
mod pdf;
mod server;

use clap::Parser;
use cli::Cli;

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    match app::run(cli).await {
        Ok(summary) => {
            if summary.partial {
                println!(
                    "Verarbeitung unterbrochen, Partial-Output geschrieben nach {}",
                    summary.output_path.display()
                );
            } else {
                println!(
                    "Markdown fuer {} Seiten geschrieben nach {}",
                    summary.pages_written,
                    summary.output_path.display()
                );
            }
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
