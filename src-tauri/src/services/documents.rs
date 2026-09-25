//! Profile document files: validation, text extraction and storage.
//!
//! Files are copied into `<app data>/profile-documents/` under a random
//! name; the database keeps their metadata and extracted text. A file's
//! type is decided by its content (magic bytes), not only its extension.

use std::{
    io::{Cursor, Read},
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};

use crate::{
    error::{AppError, AppResult},
    models::profile::DocumentFormat,
};

/// Largest file accepted.
pub const MAX_FILE_BYTES: u64 = 20 * 1024 * 1024;
/// Extracted text is cut to this length.
pub const MAX_TEXT_CHARS: usize = 100_000;
/// Largest decompressed `word/document.xml` read from a DOCX.
const MAX_DOCX_XML_BYTES: u64 = 30 * 1024 * 1024;

pub const FOLDER: &str = "profile-documents";

/// Where profile documents are stored.
pub fn folder(data_dir: &Path) -> PathBuf {
    data_dir.join(FOLDER)
}

/// Decides the format from the content, checked against the extension.
pub fn detect_format(file_name: &str, bytes: &[u8]) -> AppResult<DocumentFormat> {
    let extension = Path::new(file_name)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_lowercase)
        .unwrap_or_default();
    let unsupported =
        || AppError::validation("Use a PDF, Word (.docx), text, Markdown, PNG or JPEG file.");
    let format = match extension.as_str() {
        "pdf" => DocumentFormat::Pdf,
        "docx" => DocumentFormat::Docx,
        "txt" | "text" => DocumentFormat::Text,
        "md" | "markdown" => DocumentFormat::Markdown,
        "png" => DocumentFormat::Png,
        "jpg" | "jpeg" => DocumentFormat::Jpeg,
        "doc" => {
            return Err(AppError::validation(
                "Old Word documents (.doc) are not supported. Save the file as .docx or PDF.",
            ))
        }
        _ => return Err(unsupported()),
    };
    let head = &bytes[..bytes.len().min(1024)];
    let matches = match format {
        DocumentFormat::Pdf => head.windows(5).any(|w| w == b"%PDF-"),
        DocumentFormat::Docx => bytes.starts_with(b"PK\x03\x04"),
        DocumentFormat::Png => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        DocumentFormat::Jpeg => bytes.starts_with(&[0xFF, 0xD8, 0xFF]),
        DocumentFormat::Text | DocumentFormat::Markdown => {
            !bytes[..bytes.len().min(8192)].contains(&0)
        }
    };
    if !matches {
        return Err(AppError::validation(format!(
            "This file is not a valid .{extension} file."
        )));
    }
    Ok(format)
}

/// Extracted plain text, or `None` when the file has none (images, scans).
pub fn extract_text(format: DocumentFormat, bytes: &[u8]) -> AppResult<Option<String>> {
    let text = match format {
        DocumentFormat::Pdf => pdf_text(bytes)?,
        DocumentFormat::Docx => docx_text(bytes)?,
        DocumentFormat::Text | DocumentFormat::Markdown => {
            let text = String::from_utf8_lossy(bytes);
            text.trim_start_matches('\u{feff}').to_string()
        }
        DocumentFormat::Png | DocumentFormat::Jpeg => return Ok(None),
    };
    let text = tidy(&text);
    Ok((!text.is_empty()).then_some(text))
}

/// Normalized whitespace: no trailing spaces, at most one blank line.
fn tidy(text: &str) -> String {
    let mut out = String::new();
    let mut blank = 0;
    for line in text.lines() {
        let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if line.is_empty() {
            blank += 1;
            if blank > 1 || out.is_empty() {
                continue;
            }
        } else {
            blank = 0;
        }
        out.push_str(&line);
        out.push('\n');
    }
    out.trim_end().chars().take(MAX_TEXT_CHARS).collect()
}

fn pdf_text(bytes: &[u8]) -> AppResult<String> {
    let unreadable = || AppError::validation("ReMa could not read this PDF.");
    // The PDF parser can panic on malformed files; contain that.
    let owned = bytes.to_vec();
    std::panic::catch_unwind(move || pdf_extract::extract_text_from_mem(&owned))
        .map_err(|_| unreadable())?
        .map_err(|_| unreadable())
}

fn docx_text(bytes: &[u8]) -> AppResult<String> {
    use quick_xml::events::Event;

    let unreadable = || AppError::validation("ReMa could not read this Word document.");
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| unreadable())?;
    let mut xml = Vec::new();
    archive
        .by_name("word/document.xml")
        .map_err(|_| unreadable())?
        .take(MAX_DOCX_XML_BYTES)
        .read_to_end(&mut xml)
        .map_err(|_| unreadable())?;

    let mut reader = quick_xml::Reader::from_reader(xml.as_slice());
    let mut text = String::new();
    let mut in_text = false;
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => match e.local_name().as_ref() {
                b"t" => in_text = true,
                b"tab" => text.push('\t'),
                b"br" | b"cr" => text.push('\n'),
                _ => {}
            },
            Ok(Event::Empty(e)) => match e.local_name().as_ref() {
                b"tab" => text.push('\t'),
                b"br" | b"cr" => text.push('\n'),
                _ => {}
            },
            Ok(Event::End(e)) => match e.local_name().as_ref() {
                b"t" => in_text = false,
                b"p" => text.push('\n'),
                _ => {}
            },
            Ok(Event::Text(t)) if in_text => {
                text.push_str(&t.decode().map_err(|_| unreadable())?);
            }
            Ok(Event::GeneralRef(r)) if in_text => {
                // &amp; &lt; &gt; &quot; &apos; and numeric references.
                let name = r.decode().map_err(|_| unreadable())?;
                let resolved = match name.as_ref() {
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    "apos" => Some('\''),
                    other => other
                        .strip_prefix("#x")
                        .and_then(|h| u32::from_str_radix(h, 16).ok())
                        .or_else(|| other.strip_prefix('#').and_then(|d| d.parse().ok()))
                        .and_then(char::from_u32),
                };
                text.extend(resolved);
            }
            Ok(Event::Eof) => break,
            Err(_) => return Err(unreadable()),
            _ => {}
        }
        buf.clear();
    }
    Ok(text)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// A new random file name, e.g. `3f9a…c2.pdf`.
pub fn generated_file_name(format: DocumentFormat) -> AppResult<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|e| AppError::internal(e.to_string()))?;
    let id: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    Ok(format!("{id}.{}", format.extension()))
}

/// Reads a user-chosen file after checking it is a regular, non-empty file
/// within the size limit.
pub fn read_source(path: &Path) -> AppResult<(String, Vec<u8>)> {
    let metadata = std::fs::metadata(path)
        .map_err(|_| AppError::validation("The file could not be opened."))?;
    if !metadata.is_file() {
        return Err(AppError::validation("Choose a file, not a folder."));
    }
    if metadata.len() == 0 {
        return Err(AppError::validation("The file is empty."));
    }
    if metadata.len() > MAX_FILE_BYTES {
        return Err(AppError::validation(format!(
            "The file is larger than {} MB.",
            MAX_FILE_BYTES / 1024 / 1024
        )));
    }
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("document")
        .to_string();
    let bytes = std::fs::read(path)?;
    Ok((name, bytes))
}

/// Writes a stored copy. Never overwrites an existing file.
pub fn store(folder: &Path, file_name: &str, bytes: &[u8]) -> AppResult<PathBuf> {
    use std::io::Write;

    std::fs::create_dir_all(folder)?;
    let path = folder.join(file_name);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(path)
}

/// The stored file for a generated name. Rejects anything that is not a
/// plain file name, so a corrupt row can never point outside the folder.
pub fn stored_path(folder: &Path, file_name: &str) -> AppResult<PathBuf> {
    let plain = !file_name.is_empty()
        && file_name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.')
        && !file_name.starts_with('.');
    if !plain {
        return Err(AppError::internal("invalid stored document name"));
    }
    Ok(folder.join(file_name))
}

#[cfg(test)]
pub mod samples {
    //! Small, valid documents built in memory for tests.

    use std::io::Write;

    /// A one-page PDF showing each line with Helvetica.
    pub fn pdf(lines: &[&str]) -> Vec<u8> {
        let mut content = String::from("BT /F1 12 Tf 72 720 Td 14 TL\n");
        for line in lines {
            let escaped = line
                .replace('\\', "\\\\")
                .replace('(', "\\(")
                .replace(')', "\\)");
            content.push_str(&format!("({escaped}) Tj T*\n"));
        }
        content.push_str("ET");
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
             /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>"
                .to_string(),
            format!(
                "<< /Length {} >>\nstream\n{content}\nendstream",
                content.len() + 1
            ),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica \
             /Encoding /WinAnsiEncoding >>"
                .to_string(),
        ];
        let mut pdf = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (i, object) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.extend(format!("{} 0 obj\n{object}\nendobj\n", i + 1).bytes());
        }
        let xref = pdf.len();
        pdf.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).bytes());
        for offset in offsets {
            pdf.extend(format!("{offset:010} 00000 n \n").bytes());
        }
        pdf.extend(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .bytes(),
        );
        pdf
    }

    /// A minimal DOCX with one paragraph per line.
    pub fn docx(paragraphs: &[&str]) -> Vec<u8> {
        let body: String = paragraphs
            .iter()
            .map(|p| {
                let escaped = p
                    .replace('&', "&amp;")
                    .replace('<', "&lt;")
                    .replace('>', "&gt;");
                format!("<w:p><w:r><w:t xml:space=\"preserve\">{escaped}</w:t></w:r></w:p>")
            })
            .collect();
        let xml = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
             <w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">\
             <w:body>{body}</w:body></w:document>"
        );
        let mut out = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut out));
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("[Content_Types].xml", options).unwrap();
            zip.write_all(b"<Types/>").unwrap();
            zip.start_file("word/document.xml", options).unwrap();
            zip.write_all(xml.as_bytes()).unwrap();
            zip.finish().unwrap();
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_formats_by_content() {
        let pdf = samples::pdf(&["Hello"]);
        assert_eq!(detect_format("cv.PDF", &pdf).unwrap(), DocumentFormat::Pdf);
        assert_eq!(
            detect_format("cv.docx", &samples::docx(&["x"])).unwrap(),
            DocumentFormat::Docx
        );
        assert_eq!(
            detect_format("cv.md", b"# CV").unwrap(),
            DocumentFormat::Markdown
        );
        assert_eq!(
            detect_format("cv.txt", b"CV").unwrap(),
            DocumentFormat::Text
        );
        // A renamed file is rejected.
        assert!(detect_format("cv.pdf", b"not a pdf").is_err());
        assert!(detect_format("cv.docx", &pdf).is_err());
        assert!(detect_format("cv.txt", b"bin\0ary").is_err());
        assert!(detect_format("cv.exe", b"MZ").is_err());
        assert!(detect_format("cv.doc", b"x").is_err());
    }

    #[test]
    fn extracts_text_from_pdf_docx_and_text() {
        let pdf = samples::pdf(&["Ana Tester", "Data Engineer (Rust)"]);
        let text = extract_text(DocumentFormat::Pdf, &pdf).unwrap().unwrap();
        assert!(text.contains("Ana Tester"), "{text:?}");
        assert!(text.contains("Data Engineer (Rust)"), "{text:?}");

        let docx = samples::docx(&["Ana Tester", "Skills: Rust & SQL"]);
        assert_eq!(
            extract_text(DocumentFormat::Docx, &docx).unwrap().unwrap(),
            "Ana Tester\nSkills: Rust & SQL"
        );

        let md = "\u{feff}# Ana\n\n\n\n- Rust   and SQL  \n";
        assert_eq!(
            extract_text(DocumentFormat::Markdown, md.as_bytes())
                .unwrap()
                .unwrap(),
            "# Ana\n\n- Rust and SQL"
        );
        assert_eq!(extract_text(DocumentFormat::Png, b"\x89PNG").unwrap(), None);
    }

    #[test]
    fn broken_files_fail_cleanly() {
        assert!(extract_text(DocumentFormat::Pdf, b"%PDF-1.4 garbage").is_err());
        assert!(extract_text(DocumentFormat::Docx, b"PK\x03\x04 garbage").is_err());
    }

    #[test]
    fn stored_paths_stay_inside_the_folder() {
        let folder = Path::new("/data/profile-documents");
        assert!(stored_path(folder, "abc123.pdf").is_ok());
        for bad in ["../x.pdf", "a/b.pdf", "", ".hidden", "x\\y.pdf"] {
            assert!(stored_path(folder, bad).is_err(), "{bad}");
        }
        let name = generated_file_name(DocumentFormat::Pdf).unwrap();
        assert_eq!(name.len(), 36);
        assert!(stored_path(folder, &name).is_ok());
    }
}
