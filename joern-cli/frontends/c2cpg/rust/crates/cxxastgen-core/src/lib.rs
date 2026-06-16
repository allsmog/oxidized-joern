use anyhow::{Context, Result};
use serde::Serialize;
use std::fs;
use std::path::Path;

pub const SCHEMA_VERSION: u32 = 1;
pub const BACKEND_NAME: &str = "oxidized-cxxastgen-scaffold";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParseOptions {
    pub include_paths: Vec<String>,
    pub defines: Vec<String>,
    pub compilation_database: Option<String>,
    pub skip_function_bodies: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SourceLanguage {
    C,
    Cpp,
    Header,
    Preprocessed,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CxxAstDocument {
    pub schema_version: u32,
    pub backend: &'static str,
    pub path: String,
    pub language: SourceLanguage,
    pub source_bytes: u64,
    pub source_lines: usize,
    pub options: ParseOptions,
}

pub fn parse_file(path: &Path, options: &ParseOptions) -> Result<CxxAstDocument> {
    let source = fs::read_to_string(path)
        .with_context(|| format!("failed to read C/C++ source '{}'", path.display()))?;
    let metadata = fs::metadata(path)
        .with_context(|| format!("failed to read metadata for '{}'", path.display()))?;

    Ok(CxxAstDocument {
        schema_version: SCHEMA_VERSION,
        backend: BACKEND_NAME,
        path: normalize_path(path),
        language: language_for_path(path),
        source_bytes: metadata.len(),
        source_lines: source.lines().count(),
        options: options.clone(),
    })
}

pub fn write_json(path: &Path, document: &CxxAstDocument) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create output directory '{}'", parent.display()))?;
    }

    let json =
        serde_json::to_vec_pretty(document).context("failed to serialize cxxastgen document")?;
    fs::write(path, json).with_context(|| format!("failed to write '{}'", path.display()))
}

pub fn language_for_path(path: &Path) -> SourceLanguage {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    match extension.as_str() {
        "c" => SourceLanguage::C,
        "cc" | "cpp" | "cxx" | "c++" => SourceLanguage::Cpp,
        "h" | "hh" | "hpp" | "hxx" | "ipp" => SourceLanguage::Header,
        "i" | "ii" => SourceLanguage::Preprocessed,
        _ => SourceLanguage::Unknown,
    }
}

pub fn is_cxx_input(path: &Path) -> bool {
    !matches!(language_for_path(path), SourceLanguage::Unknown)
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_common_c_and_cpp_extensions() {
        assert_eq!(language_for_path(Path::new("main.c")), SourceLanguage::C);
        assert_eq!(
            language_for_path(Path::new("main.cpp")),
            SourceLanguage::Cpp
        );
        assert_eq!(
            language_for_path(Path::new("main.hxx")),
            SourceLanguage::Header
        );
        assert_eq!(
            language_for_path(Path::new("main.i")),
            SourceLanguage::Preprocessed
        );
        assert_eq!(
            language_for_path(Path::new("README.md")),
            SourceLanguage::Unknown
        );
    }
}
