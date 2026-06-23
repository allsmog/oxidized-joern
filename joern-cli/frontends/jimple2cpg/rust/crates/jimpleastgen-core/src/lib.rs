//! # jimpleastgen scope (read before treating this as an "astgen")
//!
//! Unlike the other oxidized backends, `jimpleastgen` is **not** an AST
//! generator and does **not** produce Jimple. The bytecode -> Jimple lowering
//! (and therefore all of the JVM instruction-set, type-system and method-body
//! semantics) remains owned by **Soot** on the Scala side
//! (`io.joern.jimple2cpg.JimpleParserBackend.Soot`). This crate is a thin
//! input-preparation layer that replaces only `extractClassesInPackageLayout`:
//! it walks directories / `jar` / `war` / `ear` / `zip` archives, unpacks
//! `.class` and config files into a package-layout output directory, and emits
//! a `manifest.json` describing the extracted set.
//!
//! Consequences for "parity":
//!   * The parity boundary is the *extracted class set + manifest*, validated
//!     by `OxidizedJimpleCpgTests` against the CPG Soot already produces — not a
//!     JSON AST differential like the other frontends.
//!   * The constant-pool reader in [`parse_class_internal_name`] handles every
//!     standard JVM tag (1, 3-12, 15-20) purely to recover `this_class` -> name
//!     for the output path; it deliberately does not model the full class file.
//!   * Per-class failures are recorded in `manifest.skipped` (see
//!     [`Generator::skip`]) rather than aborting the run, so a malformed or
//!     future-tag class degrades gracefully instead of dropping the whole input.
//!
//! A true Rust replacement for Soot (bytecode -> Jimple IR, incl. `.apk`/`.dex`)
//! is a separate, much larger effort and is intentionally out of scope here.

use anyhow::{bail, Context, Result};
use ignore::WalkBuilder;
use regex::Regex;
use serde::Serialize;
use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use zip::ZipArchive;

const CONFIG_EXTENSIONS: &[&str] = &[
    "xml",
    "properties",
    "yaml",
    "yml",
    "tf",
    "tfvars",
    "vm",
    "jsp",
    "conf",
    "mf",
];

#[derive(Debug)]
pub struct GenerateOptions<'a> {
    pub input: &'a Path,
    pub out: &'a Path,
    pub recurse: bool,
    pub depth: usize,
    pub exclude: Option<&'a Regex>,
}

#[derive(Debug, Serialize)]
pub struct Manifest {
    pub backend: &'static str,
    pub version: &'static str,
    pub input: String,
    pub classes: Vec<ClassEntry>,
    pub config_files: Vec<ConfigEntry>,
    pub skipped: Vec<SkippedEntry>,
}

#[derive(Debug, Serialize)]
pub struct ClassEntry {
    pub source_path: String,
    pub output_path: String,
    pub internal_name: String,
    pub fully_qualified_name: String,
    pub byte_length: usize,
}

#[derive(Debug, Serialize)]
pub struct ConfigEntry {
    pub source_path: String,
    pub output_path: String,
}

#[derive(Debug, Serialize)]
pub struct SkippedEntry {
    pub path: String,
    pub reason: String,
}

pub fn generate_manifest(options: GenerateOptions<'_>) -> Result<Manifest> {
    fs::create_dir_all(options.out).with_context(|| {
        format!(
            "failed to create output directory {}",
            options.out.display()
        )
    })?;
    let mut generator = Generator {
        input_root: input_root(options.input),
        out: options.out.to_path_buf(),
        recurse: options.recurse,
        depth: options.depth,
        exclude: options.exclude,
        classes: Vec::new(),
        config_files: Vec::new(),
        skipped: Vec::new(),
    };

    generator.process_input(options.input)?;
    Ok(Manifest {
        backend: "oxidized-jimpleastgen",
        version: env!("CARGO_PKG_VERSION"),
        input: options.input.display().to_string(),
        classes: generator.classes,
        config_files: generator.config_files,
        skipped: generator.skipped,
    })
}

pub fn write_manifest(path: &Path, manifest: &Manifest) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(manifest)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, bytes).with_context(|| format!("failed to write manifest {}", path.display()))
}

struct Generator<'a> {
    input_root: PathBuf,
    out: PathBuf,
    recurse: bool,
    depth: usize,
    exclude: Option<&'a Regex>,
    classes: Vec<ClassEntry>,
    config_files: Vec<ConfigEntry>,
    skipped: Vec<SkippedEntry>,
}

impl Generator<'_> {
    fn process_input(&mut self, input: &Path) -> Result<()> {
        if input.is_file() {
            return self.process_file(input);
        }

        if input.is_dir() {
            for entry in WalkBuilder::new(input).hidden(false).build() {
                let entry = entry?;
                let path = entry.path();
                if path.is_file() {
                    self.process_file(path)?;
                }
            }
            return Ok(());
        }

        bail!("input path does not exist: {}", input.display())
    }

    fn process_file(&mut self, path: &Path) -> Result<()> {
        if self.is_excluded(path) {
            return Ok(());
        }

        if is_class_path(path) {
            match fs::read(path)
                .with_context(|| format!("failed to read class file {}", path.display()))
                .and_then(|bytes| self.process_class_bytes(&bytes, path.display().to_string()))
            {
                Ok(()) => {}
                Err(err) => self.skip(path.display().to_string(), err.to_string()),
            }
        } else if is_config_path(path) {
            match fs::read(path)
                .with_context(|| format!("failed to read config file {}", path.display()))
                .and_then(|bytes| {
                    let relative = path.strip_prefix(&self.input_root).unwrap_or(path);
                    self.write_config_bytes(&bytes, path.display().to_string(), relative)
                }) {
                Ok(()) => {}
                Err(err) => self.skip(path.display().to_string(), err.to_string()),
            }
        } else if is_zip_like_path(path) {
            match fs::read(path)
                .with_context(|| format!("failed to read archive {}", path.display()))
                .and_then(|bytes| {
                    let depth = if self.recurse { self.depth } else { 0 };
                    self.process_zip_bytes(&bytes, path.display().to_string(), depth)
                }) {
                Ok(()) => {}
                Err(err) => self.skip(path.display().to_string(), err.to_string()),
            }
        }

        Ok(())
    }

    fn process_zip_bytes(
        &mut self,
        bytes: &[u8],
        archive_path: String,
        depth: usize,
    ) -> Result<()> {
        let reader = Cursor::new(bytes);
        let mut archive = ZipArchive::new(reader)
            .with_context(|| format!("failed to open archive {archive_path}"))?;

        for i in 0..archive.len() {
            let mut entry = archive.by_index(i)?;
            if entry.is_dir() {
                continue;
            }
            let entry_name = normalized_zip_name(entry.name());
            let source_path = format!("{archive_path}!/{entry_name}");
            if is_zip_slip(&entry_name) || self.is_excluded_label(&source_path) {
                continue;
            }

            if is_class_name(&entry_name) {
                let mut entry_bytes = Vec::with_capacity(entry.size() as usize);
                entry.read_to_end(&mut entry_bytes)?;
                if let Err(err) = self.process_class_bytes(&entry_bytes, source_path.clone()) {
                    self.skip(source_path, err.to_string());
                }
            } else if is_config_name(&entry_name) {
                let mut entry_bytes = Vec::with_capacity(entry.size() as usize);
                entry.read_to_end(&mut entry_bytes)?;
                if let Err(err) = self.write_config_bytes(
                    &entry_bytes,
                    source_path.clone(),
                    Path::new(&entry_name),
                ) {
                    self.skip(source_path, err.to_string());
                }
            } else if depth > 0 && is_zip_like_name(&entry_name) {
                let mut entry_bytes = Vec::with_capacity(entry.size() as usize);
                entry.read_to_end(&mut entry_bytes)?;
                if let Err(err) =
                    self.process_zip_bytes(&entry_bytes, source_path.clone(), depth - 1)
                {
                    self.skip(source_path, err.to_string());
                }
            }
        }

        Ok(())
    }

    fn process_class_bytes(&mut self, bytes: &[u8], source_path: String) -> Result<()> {
        let internal_name = parse_class_internal_name(bytes)
            .with_context(|| format!("failed to parse JVM class name from {source_path}"))?;
        let target = self.out.join(format!("{internal_name}.class"));
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&target, bytes)
            .with_context(|| format!("failed to write {}", target.display()))?;

        self.classes.push(ClassEntry {
            source_path,
            output_path: target.display().to_string(),
            fully_qualified_name: internal_name.replace('/', "."),
            internal_name,
            byte_length: bytes.len(),
        });
        Ok(())
    }

    fn write_config_bytes(
        &mut self,
        bytes: &[u8],
        source_path: String,
        relative_path: &Path,
    ) -> Result<()> {
        let target = self.out.join(relative_path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&target, bytes)
            .with_context(|| format!("failed to write {}", target.display()))?;
        self.config_files.push(ConfigEntry {
            source_path,
            output_path: target.display().to_string(),
        });
        Ok(())
    }

    fn skip(&mut self, path: String, reason: String) {
        self.skipped.push(SkippedEntry { path, reason });
    }

    fn is_excluded(&self, path: &Path) -> bool {
        self.is_excluded_label(&path.display().to_string())
    }

    fn is_excluded_label(&self, label: &str) -> bool {
        self.exclude.is_some_and(|regex| regex.is_match(label))
    }
}

#[derive(Debug)]
enum ConstantPoolEntry {
    Utf8(String),
    Class(u16),
}

pub fn parse_class_internal_name(bytes: &[u8]) -> Result<String> {
    let mut reader = ClassReader { bytes, offset: 0 };
    let magic = reader.read_u4()?;
    if magic != 0xCAFEBABE {
        bail!("not a JVM class file")
    }

    let _minor = reader.read_u2()?;
    let _major = reader.read_u2()?;
    let cp_count = reader.read_u2()? as usize;
    let mut constant_pool: Vec<Option<ConstantPoolEntry>> = Vec::with_capacity(cp_count);
    constant_pool.push(None);

    let mut index = 1;
    while index < cp_count {
        let tag = reader.read_u1()?;
        match tag {
            1 => {
                let len = reader.read_u2()? as usize;
                let value = reader.read_bytes(len)?;
                constant_pool.push(Some(ConstantPoolEntry::Utf8(
                    String::from_utf8_lossy(value).into_owned(),
                )));
            }
            3 | 4 => {
                reader.skip(4)?;
                constant_pool.push(None);
            }
            5 | 6 => {
                reader.skip(8)?;
                constant_pool.push(None);
                constant_pool.push(None);
                index += 1;
            }
            7 => {
                constant_pool.push(Some(ConstantPoolEntry::Class(reader.read_u2()?)));
            }
            8 | 16 | 19 | 20 => {
                reader.skip(2)?;
                constant_pool.push(None);
            }
            9 | 10 | 11 | 12 | 17 | 18 => {
                reader.skip(4)?;
                constant_pool.push(None);
            }
            15 => {
                reader.skip(3)?;
                constant_pool.push(None);
            }
            _ => bail!("unsupported constant pool tag {tag}"),
        }
        index += 1;
    }

    let _access_flags = reader.read_u2()?;
    let this_class = reader.read_u2()? as usize;
    let name_index = match constant_pool.get(this_class).and_then(Option::as_ref) {
        Some(ConstantPoolEntry::Class(name_index)) => *name_index as usize,
        _ => bail!("this_class does not point to a CONSTANT_Class entry"),
    };
    match constant_pool.get(name_index).and_then(Option::as_ref) {
        Some(ConstantPoolEntry::Utf8(name)) if !name.is_empty() => Ok(name.clone()),
        _ => bail!("class name does not point to a CONSTANT_Utf8 entry"),
    }
}

struct ClassReader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> ClassReader<'a> {
    fn read_u1(&mut self) -> Result<u8> {
        Ok(*self.read_bytes(1)?.first().expect("read one byte"))
    }

    fn read_u2(&mut self) -> Result<u16> {
        let bytes = self.read_bytes(2)?;
        Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
    }

    fn read_u4(&mut self) -> Result<u32> {
        let bytes = self.read_bytes(4)?;
        Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn read_bytes(&mut self, len: usize) -> Result<&'a [u8]> {
        let end = self
            .offset
            .checked_add(len)
            .context("class file offset overflow")?;
        if end > self.bytes.len() {
            bail!("unexpected end of class file")
        }
        let slice = &self.bytes[self.offset..end];
        self.offset = end;
        Ok(slice)
    }

    fn skip(&mut self, len: usize) -> Result<()> {
        self.read_bytes(len).map(|_| ())
    }
}

fn input_root(input: &Path) -> PathBuf {
    if input.is_dir() {
        input.to_path_buf()
    } else {
        input
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf()
    }
}

fn normalized_zip_name(name: &str) -> String {
    name.replace('\\', "/")
}

fn is_zip_slip(name: &str) -> bool {
    name.split('/').any(|part| part == "..")
}

fn extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
}

fn name_extension(name: &str) -> Option<String> {
    Path::new(name)
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
}

fn is_class_path(path: &Path) -> bool {
    extension(path).as_deref() == Some("class")
}

fn is_class_name(name: &str) -> bool {
    name_extension(name).as_deref() == Some("class")
}

fn is_config_path(path: &Path) -> bool {
    extension(path).is_some_and(|ext| CONFIG_EXTENSIONS.contains(&ext.as_str()))
}

fn is_config_name(name: &str) -> bool {
    name_extension(name).is_some_and(|ext| CONFIG_EXTENSIONS.contains(&ext.as_str()))
}

fn is_zip_like_path(path: &Path) -> bool {
    extension(path).is_some_and(|ext| matches!(ext.as_str(), "jar" | "zip" | "war" | "ear"))
}

fn is_zip_like_name(name: &str) -> bool {
    name_extension(name).is_some_and(|ext| matches!(ext.as_str(), "jar" | "zip" | "war" | "ear"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_class_name_from_minimal_class_file() {
        let bytes = minimal_class("pkg/Foo");
        let name = parse_class_internal_name(&bytes).unwrap();
        assert_eq!(name, "pkg/Foo");
    }

    fn minimal_class(internal_name: &str) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0xCAFEBABEu32.to_be_bytes());
        bytes.extend_from_slice(&0u16.to_be_bytes());
        bytes.extend_from_slice(&52u16.to_be_bytes());
        bytes.extend_from_slice(&3u16.to_be_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&(internal_name.len() as u16).to_be_bytes());
        bytes.extend_from_slice(internal_name.as_bytes());
        bytes.push(7);
        bytes.extend_from_slice(&1u16.to_be_bytes());
        bytes.extend_from_slice(&0x0021u16.to_be_bytes());
        bytes.extend_from_slice(&2u16.to_be_bytes());
        bytes.extend_from_slice(&0u16.to_be_bytes());
        bytes
    }
}
