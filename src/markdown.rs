use std::path::Path;

use chrono::Utc;
use regex::Regex;

use crate::pdf::PdfInfo;

pub fn clean_markdown(raw: &str) -> String {
    let fence_start = Regex::new(r"(?s)^\s*```(?:markdown)?\s*").expect("valid regex");
    let fence_end = Regex::new(r"(?s)\s*```\s*$").expect("valid regex");
    let ocr_artifacts =
        Regex::new(r"(?m)^[A-Za-z_]+\[\[[^\]]+\]\]\s*$\n?").expect("valid regex");
    let blank_lines = Regex::new(r"\n{3,}").expect("valid regex");

    let normalized = raw.replace("\r\n", "\n");
    let without_start = fence_start.replace(&normalized, "");
    let without_fences = fence_end.replace(&without_start, "");
    let without_artifacts = ocr_artifacts.replace_all(&without_fences, "");
    let normalized_bullets = without_artifacts
        .lines()
        .flat_map(normalize_compact_bullets)
        .collect::<Vec<_>>()
        .join("\n");
    let trimmed = normalized_bullets.trim();

    blank_lines.replace_all(trimmed, "\n\n").into_owned()
}

pub fn assemble_markdown(pdf: &PdfInfo, source_pdf: &Path, pages: &[(u32, String)]) -> String {
    let body = pages
        .iter()
        .map(|(page_number, markdown)| format!("<!-- page: {page_number} -->\n{markdown}"))
        .collect::<Vec<_>>()
        .join("\n\n");

    let toc = build_toc(&body);
    let content = if toc.is_empty() {
        body
    } else {
        format!("## Table of Contents\n\n{toc}\n\n{body}")
    };

    format!(
        "{}\n{}\n",
        build_frontmatter(pdf, source_pdf),
        content.trim()
    )
}

fn build_frontmatter(pdf: &PdfInfo, source_pdf: &Path) -> String {
    let mut lines = vec![
        "---".to_string(),
        format!(
            "source_pdf: {}",
            yaml_string(&source_pdf.display().to_string())
        ),
        format!("pages: {}", pdf.page_count),
        format!("generated_at: {}", yaml_string(&Utc::now().to_rfc3339())),
    ];

    if let Some(title) = pdf.title.as_deref().filter(|value| !value.is_empty()) {
        lines.push(format!("title: {}", yaml_string(title)));
    }

    if let Some(author) = pdf.author.as_deref().filter(|value| !value.is_empty()) {
        lines.push(format!("author: {}", yaml_string(author)));
    }

    if let Some(creator) = pdf.creator.as_deref().filter(|value| !value.is_empty()) {
        lines.push(format!("creator: {}", yaml_string(creator)));
    }

    if let Some(producer) = pdf.producer.as_deref().filter(|value| !value.is_empty()) {
        lines.push(format!("producer: {}", yaml_string(producer)));
    }

    lines.push("---".to_string());
    lines.join("\n")
}

fn build_toc(markdown: &str) -> String {
    let heading_regex = Regex::new(r"^(#{1,6})\s+(.+?)\s*$").expect("valid regex");
    let mut entries = Vec::new();

    for line in markdown.lines() {
        let Some(captures) = heading_regex.captures(line) else {
            continue;
        };
        let level = captures
            .get(1)
            .map(|value| value.as_str().len())
            .unwrap_or(1);
        let title = captures
            .get(2)
            .map(|value| value.as_str().trim())
            .unwrap_or("");

        if title.is_empty() {
            continue;
        }

        let indent = "  ".repeat(level.saturating_sub(1));
        entries.push(format!("{indent}- [{title}](#{})", slugify(title)));
    }

    entries.join("\n")
}

fn slugify(input: &str) -> String {
    let lowered = input.to_lowercase();
    let mut slug = String::with_capacity(lowered.len());
    let mut previous_dash = false;

    for character in lowered.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character);
            previous_dash = false;
        } else if !previous_dash {
            slug.push('-');
            previous_dash = true;
        }
    }

    slug.trim_matches('-').to_string()
}

fn yaml_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn normalize_compact_bullets(line: &str) -> Vec<String> {
    let trimmed = line.trim_start();
    if !trimmed.starts_with("- ") || trimmed.matches("- ").count() < 2 {
        return vec![line.to_string()];
    }

    trimmed
        .split("- ")
        .filter(|part| !part.trim().is_empty())
        .map(|part| format!("- {}", part.trim()))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::pdf::PdfInfo;

    use super::{assemble_markdown, clean_markdown};

    #[test]
    fn cleans_code_fence_wrapped_markdown() {
        let cleaned = clean_markdown("```markdown\n# Title\n\nContent\n```");
        assert_eq!(cleaned, "# Title\n\nContent");
    }

    #[test]
    fn removes_ocr_artifacts_and_expands_compact_bullets() {
        let cleaned = clean_markdown(
            "sub_title[[1, 2, 3, 4]]\n## Items\ntext[[1, 2, 3, 4]]\n- Alpha- Beta- Gamma",
        );

        assert_eq!(cleaned, "## Items\n- Alpha\n- Beta\n- Gamma");
    }

    #[test]
    fn builds_table_of_contents_for_headings() {
        let pdf = PdfInfo {
            title: None,
            author: None,
            creator: None,
            producer: None,
            page_count: 1,
            pages: Vec::new(),
        };
        let markdown = assemble_markdown(
            &pdf,
            Path::new("demo.pdf"),
            &[(1, "# Heading\n\n## Sub Heading".to_string())],
        );

        assert!(markdown.contains("## Table of Contents"));
        assert!(markdown.contains("[Heading](#heading)"));
        assert!(markdown.contains("[Sub Heading](#sub-heading)"));
    }
}
