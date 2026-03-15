use std::path::PathBuf;

use directories::ProjectDirs;
use tokio::fs;

use crate::error::AppError;

#[derive(Debug, Clone)]
pub struct Cache {
    enabled: bool,
    root: PathBuf,
}

impl Cache {
    pub async fn new(enabled: bool) -> Result<Self, AppError> {
        let root = if enabled {
            let dirs = ProjectDirs::from("dev", "unfold", "pdf-ocr")
                .ok_or(AppError::CacheDirUnavailable)?;
            let root = dirs.cache_dir().join("pages");
            fs::create_dir_all(&root).await?;
            root
        } else {
            std::env::temp_dir().join("pdf-ocr-disabled-cache")
        };

        Ok(Self { enabled, root })
    }

    pub async fn load(&self, hash: &str) -> Result<Option<String>, AppError> {
        if !self.enabled {
            return Ok(None);
        }

        let path = self.path_for(hash);
        match fs::read_to_string(path).await {
            Ok(content) => Ok(Some(content)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub async fn save(&self, hash: &str, markdown: &str) -> Result<(), AppError> {
        if !self.enabled {
            return Ok(());
        }

        fs::write(self.path_for(hash), markdown).await?;
        Ok(())
    }

    fn path_for(&self, hash: &str) -> PathBuf {
        self.root.join(format!("{hash}.md"))
    }
}
