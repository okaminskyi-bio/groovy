use regex::Regex;
use std::fs;
use std::io::Read;
use std::path::Path;
use zip::ZipArchive;

pub fn extract_template_text(path: &Path) -> Result<String, String> {
    let file = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut archive = ZipArchive::new(file).map_err(|e| e.to_string())?;
    let mut doc_xml = archive
        .by_name("word/document.xml")
        .map_err(|e| format!("document.xml missing: {e}"))?;

    let mut xml = String::new();
    doc_xml
        .read_to_string(&mut xml)
        .map_err(|e| format!("cannot read XML: {e}"))?;

    let token_re =
        Regex::new(r#"(?s)<w:t[^>]*>(.*?)</w:t>|<w:tab\s*/>|<w:br\s*/>|<w:cr\s*/>|</w:p>"#)
            .map_err(|e| e.to_string())?;

    let mut out = String::new();
    for cap in token_re.captures_iter(&xml) {
        if let Some(text_cap) = cap.get(1) {
            out.push_str(&decode_xml_entities(text_cap.as_str()));
        } else if let Some(full) = cap.get(0) {
            if full.as_str().starts_with("<w:tab") {
                out.push('\t');
            } else {
                out.push('\n');
            }
        }
    }

    let trimmed = out.trim().to_string();
    if trimmed.is_empty() {
        return Err("No text content extracted from DOCX.".to_string());
    }
    Ok(trimmed)
}

fn decode_xml_entities(input: &str) -> String {
    input
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}
