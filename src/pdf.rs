use std::io::Cursor;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use image::{DynamicImage, GenericImageView, ImageFormat};
use lopdf::{Document, Object, ObjectId};
use tokio::fs;
use tokio::process::Command;

use crate::error::AppError;

const MAX_SIDE_PX: u32 = 1_536;
const MIN_DPI: u32 = 72;
const MAX_DPI: u32 = 300;

#[derive(Debug, Clone)]
pub struct PageSpec {
    pub number: u32,
    pub width_pts: f32,
    pub height_pts: f32,
}

#[derive(Debug, Clone)]
pub struct PdfInfo {
    pub title: Option<String>,
    pub author: Option<String>,
    pub creator: Option<String>,
    pub producer: Option<String>,
    pub page_count: usize,
    pub pages: Vec<PageSpec>,
}

#[derive(Debug, Clone)]
pub struct EncodedPage {
    pub width: u32,
    pub height: u32,
    pub webp_bytes: Vec<u8>,
    pub base64_webp: String,
    pub base64_png: String,
}

pub fn inspect_pdf(path: &Path) -> Result<PdfInfo, AppError> {
    let document =
        Document::load(path).map_err(|error| AppError::PdfProcessingError(error.to_string()))?;
    let pages = document.get_pages();
    let mut page_specs = Vec::with_capacity(pages.len());

    for (page_number, object_id) in pages {
        let (width_pts, height_pts) = extract_page_size(&document, object_id)?;
        page_specs.push(PageSpec {
            number: page_number,
            width_pts,
            height_pts,
        });
    }

    Ok(PdfInfo {
        title: extract_info_value(&document, b"Title"),
        author: extract_info_value(&document, b"Author"),
        creator: extract_info_value(&document, b"Creator"),
        producer: extract_info_value(&document, b"Producer"),
        page_count: page_specs.len(),
        pages: page_specs,
    })
}

pub async fn rasterize_page(input: &Path, page: &PageSpec) -> Result<DynamicImage, AppError> {
    let dpi = calculate_adaptive_dpi(page);
    let tempdir = tempfile::tempdir()?;
    let prefix = tempdir.path().join("page");

    let status = Command::new("pdftoppm")
        .arg("-f")
        .arg(page.number.to_string())
        .arg("-l")
        .arg(page.number.to_string())
        .arg("-r")
        .arg(dpi.to_string())
        .arg("-png")
        .arg(input)
        .arg(&prefix)
        .status()
        .await
        .map_err(|error| {
            AppError::PdfProcessingError(format!("pdftoppm konnte nicht gestartet werden: {error}"))
        })?;

    if !status.success() {
        return Err(AppError::PdfProcessingError(format!(
            "pdftoppm ist fuer Seite {} mit Status {} fehlgeschlagen",
            page.number, status
        )));
    }

    let image_path = find_rendered_png(&prefix).await?;
    let bytes = fs::read(image_path).await?;
    let image = image::load_from_memory_with_format(&bytes, ImageFormat::Png)?;
    Ok(image)
}

pub fn encode_page_as_webp(image: &DynamicImage) -> Result<EncodedPage, AppError> {
    let rgba = image.to_rgba8();
    let (width, height) = image.dimensions();
    let encoder = webp::Encoder::from_rgba(rgba.as_raw(), width, height);
    let webp = encoder.encode(80.0);
    let webp_bytes = webp.to_vec();
    let base64_webp = BASE64_STANDARD.encode(&webp_bytes);
    let mut png_bytes = Vec::new();
    image.write_to(&mut Cursor::new(&mut png_bytes), ImageFormat::Png)?;
    let base64_png = BASE64_STANDARD.encode(&png_bytes);

    Ok(EncodedPage {
        width,
        height,
        webp_bytes,
        base64_webp,
        base64_png,
    })
}

async fn find_rendered_png(prefix: &Path) -> Result<PathBuf, AppError> {
    let parent = prefix.parent().unwrap_or_else(|| Path::new("."));
    let prefix_name = prefix
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| AppError::PdfProcessingError("Ungueltiger Temp-Dateiname".into()))?;
    let expected_prefix = format!("{prefix_name}-");
    let mut entries = fs::read_dir(parent).await?;

    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(OsStr::to_str) else {
            continue;
        };
        if file_name.starts_with(&expected_prefix) && path.extension() == Some(OsStr::new("png")) {
            return Ok(path);
        }
    }

    Err(AppError::PdfProcessingError(
        "Gerenderte PNG-Datei wurde nicht gefunden".into(),
    ))
}

fn calculate_adaptive_dpi(page: &PageSpec) -> u32 {
    let longest_side_pts = page.width_pts.max(page.height_pts).max(1.0);
    let raw_dpi = ((MAX_SIDE_PX as f32 * 72.0) / longest_side_pts).round() as u32;
    raw_dpi.clamp(MIN_DPI, MAX_DPI)
}

fn extract_page_size(document: &Document, object_id: ObjectId) -> Result<(f32, f32), AppError> {
    let object = document
        .get_object(object_id)
        .map_err(|error| AppError::PdfProcessingError(error.to_string()))?;
    let dict = object
        .as_dict()
        .map_err(|error| AppError::PdfProcessingError(error.to_string()))?;
    let mediabox = dict
        .get(b"MediaBox")
        .or_else(|_| dict.get(b"CropBox"))
        .map_err(|_| {
            AppError::PdfProcessingError(
                "Keine MediaBox oder CropBox fuer PDF-Seite gefunden".into(),
            )
        })?;
    let array = mediabox
        .as_array()
        .map_err(|error| AppError::PdfProcessingError(error.to_string()))?;

    if array.len() != 4 {
        return Err(AppError::PdfProcessingError(
            "MediaBox/CropBox enthaelt keine vier Koordinaten".into(),
        ));
    }

    let llx = object_to_f32(&array[0])?;
    let lly = object_to_f32(&array[1])?;
    let urx = object_to_f32(&array[2])?;
    let ury = object_to_f32(&array[3])?;

    Ok(((urx - llx).abs(), (ury - lly).abs()))
}

fn object_to_f32(object: &Object) -> Result<f32, AppError> {
    match object {
        Object::Integer(value) => Ok(*value as f32),
        Object::Real(value) => Ok(*value),
        _ => Err(AppError::PdfProcessingError(
            "Ungueltiger numerischer Wert in PDF-Box".into(),
        )),
    }
}

fn extract_info_value(document: &Document, key: &[u8]) -> Option<String> {
    let info_reference = document.trailer.get(b"Info").ok()?.as_reference().ok()?;
    let info_dict = document.get_dictionary(info_reference).ok()?;
    let value = info_dict.get(key).ok()?;
    pdf_object_to_string(value)
}

fn pdf_object_to_string(object: &Object) -> Option<String> {
    match object {
        Object::String(bytes, _) => Some(String::from_utf8_lossy(bytes).trim().to_string()),
        Object::Name(bytes) => Some(String::from_utf8_lossy(bytes).trim().to_string()),
        _ => None,
    }
}
