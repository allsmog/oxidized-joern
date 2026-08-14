//! # jimpleastgen scope (read before treating this as an "astgen")
//!
//! Unlike the other oxidized backends, `jimpleastgen` does **not** yet produce
//! Jimple method-body ASTs. The bytecode -> Jimple lowering (and therefore the
//! JVM instruction-set control/data-flow semantics) remains outside this
//! crate. The pinned reference behavior comes from **Soot**.
//!
//! This crate owns JVM input discovery and declaration
//! metadata extraction: it walks directories / `jar` / `war` / `ear` / `zip`
//! archives, unpacks `.class` and config files into a package-layout output
//! directory, and emits a `manifest.json` with class/interface/super/field/
//! method metadata parsed directly from bytecode.
//!
//! Consequences for "parity":
//!   * The current parity boundary is the extracted class set plus declaration
//!     manifest, validated by tests against the CPG Soot already produces. This
//!     is still short of a JSON AST differential like the source frontends.
//!   * The class reader handles every standard JVM constant-pool tag (1, 3-12,
//!     15-20), parses the class-file declaration tables, and decodes method
//!     `Code` byte streams into JVM instructions. It still does not lower those
//!     instructions into Jimple statements.
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
const CAUGHT_EXCEPTION_REF: &str = "@caughtexception";

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
    pub super_internal_name: Option<String>,
    pub super_fully_qualified_name: Option<String>,
    pub interfaces: Vec<ClassReference>,
    pub minor_version: u16,
    pub major_version: u16,
    pub access_flags: u16,
    pub access_flags_text: Vec<&'static str>,
    pub source_file: Option<String>,
    pub signature: Option<String>,
    pub fields: Vec<FieldEntry>,
    pub methods: Vec<MethodEntry>,
    pub byte_length: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ClassReference {
    pub internal_name: String,
    pub fully_qualified_name: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FieldEntry {
    pub name: String,
    pub descriptor: String,
    pub type_name: Option<String>,
    pub access_flags: u16,
    pub access_flags_text: Vec<&'static str>,
    pub signature: Option<String>,
    pub constant_value: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MethodEntry {
    pub name: String,
    pub descriptor: String,
    pub parameter_types: Vec<String>,
    pub return_type: Option<String>,
    pub access_flags: u16,
    pub access_flags_text: Vec<&'static str>,
    pub signature: Option<String>,
    pub exceptions: Vec<ClassReference>,
    pub code: Option<MethodCodeEntry>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MethodCodeEntry {
    pub max_stack: u16,
    pub max_locals: u16,
    pub bytecode_length: usize,
    pub instructions: Vec<BytecodeInstructionEntry>,
    pub body_ir: Vec<MethodBodyIrEntry>,
    pub exception_table: Vec<ExceptionHandlerEntry>,
    pub line_numbers: Vec<LineNumberEntry>,
    pub local_variables: Vec<LocalVariableEntry>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MethodBodyIrEntry {
    pub offset: u32,
    pub operation: &'static str,
    pub code: String,
    pub result: Option<String>,
    pub target: Option<String>,
    pub method_full_name: Option<String>,
    pub signature: Option<String>,
    pub dispatch_type: Option<&'static str>,
    pub receiver: Option<String>,
    pub targets: Vec<u32>,
    pub arguments: Vec<String>,
    pub bootstrap_arguments: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BytecodeInstructionEntry {
    pub offset: u32,
    pub opcode: u8,
    pub mnemonic: &'static str,
    pub operands: Vec<BytecodeOperandEntry>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BytecodeOperandEntry {
    pub name: &'static str,
    pub kind: &'static str,
    pub value: i32,
    pub resolved: Option<ResolvedConstantPoolEntry>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ResolvedConstantPoolEntry {
    pub tag: &'static str,
    #[serde(rename = "class")]
    pub class_reference: Option<ClassReference>,
    pub name: Option<String>,
    pub descriptor: Option<String>,
    pub field_type: Option<String>,
    pub parameter_types: Vec<String>,
    pub return_type: Option<String>,
    pub value: Option<String>,
    pub reference_kind: Option<u8>,
    pub reference_kind_text: Option<&'static str>,
    pub reference_index: Option<u16>,
    pub bootstrap_method_attr_index: Option<u16>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ExceptionHandlerEntry {
    pub start_pc: u16,
    pub end_pc: u16,
    pub handler_pc: u16,
    pub catch_type: Option<ClassReference>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LineNumberEntry {
    pub start_pc: u16,
    pub line_number: u16,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LocalVariableEntry {
    pub start_pc: u16,
    pub length: u16,
    pub name: String,
    pub descriptor: String,
    pub type_name: Option<String>,
    pub signature: Option<String>,
    pub index: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedClass {
    pub internal_name: String,
    pub fully_qualified_name: String,
    pub super_internal_name: Option<String>,
    pub super_fully_qualified_name: Option<String>,
    pub interfaces: Vec<ClassReference>,
    pub minor_version: u16,
    pub major_version: u16,
    pub access_flags: u16,
    pub access_flags_text: Vec<&'static str>,
    pub source_file: Option<String>,
    pub signature: Option<String>,
    pub fields: Vec<FieldEntry>,
    pub methods: Vec<MethodEntry>,
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
        let parsed = parse_class_file(bytes)
            .with_context(|| format!("failed to parse JVM class file from {source_path}"))?;
        let target = self.out.join(format!("{}.class", parsed.internal_name));
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&target, bytes)
            .with_context(|| format!("failed to write {}", target.display()))?;

        self.classes.push(ClassEntry {
            source_path,
            output_path: target.display().to_string(),
            internal_name: parsed.internal_name,
            fully_qualified_name: parsed.fully_qualified_name,
            super_internal_name: parsed.super_internal_name,
            super_fully_qualified_name: parsed.super_fully_qualified_name,
            interfaces: parsed.interfaces,
            minor_version: parsed.minor_version,
            major_version: parsed.major_version,
            access_flags: parsed.access_flags,
            access_flags_text: parsed.access_flags_text,
            source_file: parsed.source_file,
            signature: parsed.signature,
            fields: parsed.fields,
            methods: parsed.methods,
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

#[derive(Debug, Clone)]
enum ConstantPoolEntry {
    Utf8(String),
    Integer(i32),
    Float(u32),
    Long(i64),
    Double(u64),
    Class(u16),
    String(u16),
    Fieldref {
        class_index: u16,
        name_and_type_index: u16,
    },
    Methodref {
        class_index: u16,
        name_and_type_index: u16,
    },
    InterfaceMethodref {
        class_index: u16,
        name_and_type_index: u16,
    },
    NameAndType {
        name_index: u16,
        descriptor_index: u16,
    },
    MethodHandle {
        reference_kind: u8,
        reference_index: u16,
    },
    MethodType {
        descriptor_index: u16,
    },
    Dynamic {
        bootstrap_method_attr_index: u16,
        name_and_type_index: u16,
    },
    InvokeDynamic {
        bootstrap_method_attr_index: u16,
        name_and_type_index: u16,
    },
    Module(u16),
    Package(u16),
}

struct ConstantPool {
    entries: Vec<Option<ConstantPoolEntry>>,
}

impl ConstantPool {
    fn read(reader: &mut ClassReader<'_>) -> Result<Self> {
        let cp_count = reader.read_u2()? as usize;
        let mut entries = Vec::with_capacity(cp_count);
        entries.push(None);

        let mut index = 1;
        while index < cp_count {
            let tag = reader.read_u1()?;
            match tag {
                1 => {
                    let len = reader.read_u2()? as usize;
                    let value = reader.read_bytes(len)?;
                    entries.push(Some(ConstantPoolEntry::Utf8(
                        String::from_utf8_lossy(value).into_owned(),
                    )));
                }
                3 => {
                    entries.push(Some(ConstantPoolEntry::Integer(reader.read_u4()? as i32)));
                }
                4 => {
                    entries.push(Some(ConstantPoolEntry::Float(reader.read_u4()?)));
                }
                5 => {
                    entries.push(Some(ConstantPoolEntry::Long(reader.read_u8()? as i64)));
                    entries.push(None);
                    index += 1;
                }
                6 => {
                    entries.push(Some(ConstantPoolEntry::Double(reader.read_u8()?)));
                    entries.push(None);
                    index += 1;
                }
                7 => {
                    entries.push(Some(ConstantPoolEntry::Class(reader.read_u2()?)));
                }
                8 => {
                    entries.push(Some(ConstantPoolEntry::String(reader.read_u2()?)));
                }
                9 => {
                    entries.push(Some(ConstantPoolEntry::Fieldref {
                        class_index: reader.read_u2()?,
                        name_and_type_index: reader.read_u2()?,
                    }));
                }
                10 => {
                    entries.push(Some(ConstantPoolEntry::Methodref {
                        class_index: reader.read_u2()?,
                        name_and_type_index: reader.read_u2()?,
                    }));
                }
                11 => {
                    entries.push(Some(ConstantPoolEntry::InterfaceMethodref {
                        class_index: reader.read_u2()?,
                        name_and_type_index: reader.read_u2()?,
                    }));
                }
                12 => {
                    entries.push(Some(ConstantPoolEntry::NameAndType {
                        name_index: reader.read_u2()?,
                        descriptor_index: reader.read_u2()?,
                    }));
                }
                15 => {
                    entries.push(Some(ConstantPoolEntry::MethodHandle {
                        reference_kind: reader.read_u1()?,
                        reference_index: reader.read_u2()?,
                    }));
                }
                16 => {
                    entries.push(Some(ConstantPoolEntry::MethodType {
                        descriptor_index: reader.read_u2()?,
                    }));
                }
                17 => {
                    entries.push(Some(ConstantPoolEntry::Dynamic {
                        bootstrap_method_attr_index: reader.read_u2()?,
                        name_and_type_index: reader.read_u2()?,
                    }));
                }
                18 => {
                    entries.push(Some(ConstantPoolEntry::InvokeDynamic {
                        bootstrap_method_attr_index: reader.read_u2()?,
                        name_and_type_index: reader.read_u2()?,
                    }));
                }
                19 => {
                    entries.push(Some(ConstantPoolEntry::Module(reader.read_u2()?)));
                }
                20 => {
                    entries.push(Some(ConstantPoolEntry::Package(reader.read_u2()?)));
                }
                _ => bail!("unsupported constant pool tag {tag}"),
            }
            index += 1;
        }

        Ok(Self { entries })
    }

    fn utf8(&self, index: u16) -> Result<&str> {
        match self.entries.get(index as usize).and_then(Option::as_ref) {
            Some(ConstantPoolEntry::Utf8(value)) if !value.is_empty() => Ok(value),
            _ => bail!("constant pool index {index} does not point to a CONSTANT_Utf8 entry"),
        }
    }

    fn entry(&self, index: u16) -> Result<&ConstantPoolEntry> {
        self.entries
            .get(index as usize)
            .and_then(Option::as_ref)
            .with_context(|| format!("constant pool index {index} is not a valid entry"))
    }

    fn class_name(&self, index: u16) -> Result<String> {
        let name_index = match self.entries.get(index as usize).and_then(Option::as_ref) {
            Some(ConstantPoolEntry::Class(name_index)) => *name_index,
            _ => bail!("constant pool index {index} does not point to a CONSTANT_Class entry"),
        };
        Ok(self.utf8(name_index)?.to_string())
    }

    fn class_reference(&self, index: u16) -> Result<ClassReference> {
        let internal_name = self.class_name(index)?;
        Ok(ClassReference {
            fully_qualified_name: internal_to_fqn(&internal_name),
            internal_name,
        })
    }

    fn name_and_type(&self, index: u16) -> Result<(String, String)> {
        match self.entry(index)? {
            ConstantPoolEntry::NameAndType {
                name_index,
                descriptor_index,
            } => Ok((
                self.utf8(*name_index)?.to_string(),
                self.utf8(*descriptor_index)?.to_string(),
            )),
            _ => {
                bail!("constant pool index {index} does not point to a CONSTANT_NameAndType entry")
            }
        }
    }

    fn resolve(&self, index: u16) -> Result<ResolvedConstantPoolEntry> {
        match self.entry(index)? {
            ConstantPoolEntry::Utf8(value) => {
                let mut resolved = resolved_constant_pool_entry("Utf8");
                resolved.value = Some(value.clone());
                Ok(resolved)
            }
            ConstantPoolEntry::Integer(value) => {
                let mut resolved = resolved_constant_pool_entry("Integer");
                resolved.value = Some(value.to_string());
                Ok(resolved)
            }
            ConstantPoolEntry::Float(bits) => {
                let mut resolved = resolved_constant_pool_entry("Float");
                resolved.value = Some(f32::from_bits(*bits).to_string());
                Ok(resolved)
            }
            ConstantPoolEntry::Long(value) => {
                let mut resolved = resolved_constant_pool_entry("Long");
                resolved.value = Some(value.to_string());
                Ok(resolved)
            }
            ConstantPoolEntry::Double(bits) => {
                let mut resolved = resolved_constant_pool_entry("Double");
                resolved.value = Some(f64::from_bits(*bits).to_string());
                Ok(resolved)
            }
            ConstantPoolEntry::Class(_) => {
                let mut resolved = resolved_constant_pool_entry("Class");
                resolved.class_reference = Some(self.class_reference(index)?);
                Ok(resolved)
            }
            ConstantPoolEntry::String(string_index) => {
                let mut resolved = resolved_constant_pool_entry("String");
                resolved.value = Some(self.utf8(*string_index)?.to_string());
                Ok(resolved)
            }
            ConstantPoolEntry::Fieldref {
                class_index,
                name_and_type_index,
            } => self.resolve_member_reference("Fieldref", *class_index, *name_and_type_index),
            ConstantPoolEntry::Methodref {
                class_index,
                name_and_type_index,
            } => self.resolve_member_reference("Methodref", *class_index, *name_and_type_index),
            ConstantPoolEntry::InterfaceMethodref {
                class_index,
                name_and_type_index,
            } => self.resolve_member_reference(
                "InterfaceMethodref",
                *class_index,
                *name_and_type_index,
            ),
            ConstantPoolEntry::NameAndType { .. } => {
                let (name, descriptor) = self.name_and_type(index)?;
                let mut resolved = resolved_constant_pool_entry("NameAndType");
                apply_descriptor(&mut resolved, &descriptor);
                resolved.name = Some(name);
                resolved.descriptor = Some(descriptor);
                Ok(resolved)
            }
            ConstantPoolEntry::MethodHandle {
                reference_kind,
                reference_index,
            } => {
                let mut resolved = resolved_constant_pool_entry("MethodHandle");
                resolved.reference_kind = Some(*reference_kind);
                resolved.reference_kind_text = Some(method_handle_kind_text(*reference_kind));
                resolved.reference_index = Some(*reference_index);
                if let Ok(reference) = self.resolve(*reference_index) {
                    resolved.class_reference = reference.class_reference;
                    resolved.name = reference.name;
                    resolved.descriptor = reference.descriptor;
                    resolved.field_type = reference.field_type;
                    resolved.parameter_types = reference.parameter_types;
                    resolved.return_type = reference.return_type;
                }
                Ok(resolved)
            }
            ConstantPoolEntry::MethodType { descriptor_index } => {
                let descriptor = self.utf8(*descriptor_index)?.to_string();
                let mut resolved = resolved_constant_pool_entry("MethodType");
                apply_descriptor(&mut resolved, &descriptor);
                resolved.descriptor = Some(descriptor);
                Ok(resolved)
            }
            ConstantPoolEntry::Dynamic {
                bootstrap_method_attr_index,
                name_and_type_index,
            } => self.resolve_dynamic_reference(
                "Dynamic",
                *bootstrap_method_attr_index,
                *name_and_type_index,
            ),
            ConstantPoolEntry::InvokeDynamic {
                bootstrap_method_attr_index,
                name_and_type_index,
            } => self.resolve_dynamic_reference(
                "InvokeDynamic",
                *bootstrap_method_attr_index,
                *name_and_type_index,
            ),
            ConstantPoolEntry::Module(name_index) => {
                let mut resolved = resolved_constant_pool_entry("Module");
                resolved.value = Some(self.utf8(*name_index)?.to_string());
                Ok(resolved)
            }
            ConstantPoolEntry::Package(name_index) => {
                let mut resolved = resolved_constant_pool_entry("Package");
                resolved.value = Some(self.utf8(*name_index)?.to_string());
                Ok(resolved)
            }
        }
    }

    fn resolve_member_reference(
        &self,
        tag: &'static str,
        class_index: u16,
        name_and_type_index: u16,
    ) -> Result<ResolvedConstantPoolEntry> {
        let (name, descriptor) = self.name_and_type(name_and_type_index)?;
        let mut resolved = resolved_constant_pool_entry(tag);
        apply_descriptor(&mut resolved, &descriptor);
        resolved.class_reference = Some(self.class_reference(class_index)?);
        resolved.name = Some(name);
        resolved.descriptor = Some(descriptor);
        Ok(resolved)
    }

    fn resolve_dynamic_reference(
        &self,
        tag: &'static str,
        bootstrap_method_attr_index: u16,
        name_and_type_index: u16,
    ) -> Result<ResolvedConstantPoolEntry> {
        let (name, descriptor) = self.name_and_type(name_and_type_index)?;
        let mut resolved = resolved_constant_pool_entry(tag);
        apply_descriptor(&mut resolved, &descriptor);
        resolved.name = Some(name);
        resolved.descriptor = Some(descriptor);
        resolved.bootstrap_method_attr_index = Some(bootstrap_method_attr_index);
        Ok(resolved)
    }
}

fn resolved_constant_pool_entry(tag: &'static str) -> ResolvedConstantPoolEntry {
    ResolvedConstantPoolEntry {
        tag,
        class_reference: None,
        name: None,
        descriptor: None,
        field_type: None,
        parameter_types: Vec::new(),
        return_type: None,
        value: None,
        reference_kind: None,
        reference_kind_text: None,
        reference_index: None,
        bootstrap_method_attr_index: None,
    }
}

fn apply_descriptor(resolved: &mut ResolvedConstantPoolEntry, descriptor: &str) {
    if descriptor.starts_with('(') {
        if let Ok((parameter_types, return_type)) = parse_method_descriptor(descriptor) {
            resolved.parameter_types = parameter_types;
            resolved.return_type = Some(return_type);
        }
    } else if let Ok(field_type) = parse_field_descriptor(descriptor) {
        resolved.field_type = Some(field_type);
    }
}

fn method_handle_kind_text(kind: u8) -> &'static str {
    match kind {
        1 => "getField",
        2 => "getStatic",
        3 => "putField",
        4 => "putStatic",
        5 => "invokeVirtual",
        6 => "invokeStatic",
        7 => "invokeSpecial",
        8 => "newInvokeSpecial",
        9 => "invokeInterface",
        _ => "unknown",
    }
}

#[derive(Default)]
struct ParsedAttributes {
    source_file: Option<String>,
    signature: Option<String>,
    constant_value: Option<String>,
    exceptions: Vec<ClassReference>,
    code: Option<MethodCodeEntry>,
    bootstrap_methods: Vec<BootstrapMethodEntry>,
}

#[derive(Debug, Clone, Default)]
struct BootstrapMethodEntry {
    arguments: Vec<String>,
}

pub fn parse_class_internal_name(bytes: &[u8]) -> Result<String> {
    Ok(parse_class_file(bytes)?.internal_name)
}

pub fn parse_class_file(bytes: &[u8]) -> Result<ParsedClass> {
    let mut reader = ClassReader { bytes, offset: 0 };
    let magic = reader.read_u4()?;
    if magic != 0xCAFEBABE {
        bail!("not a JVM class file")
    }

    let minor_version = reader.read_u2()?;
    let major_version = reader.read_u2()?;
    let constant_pool = ConstantPool::read(&mut reader)?;

    let access_flags = reader.read_u2()?;
    let this_class = reader.read_u2()?;
    let super_class = reader.read_u2()?;
    let internal_name = constant_pool.class_name(this_class)?;
    let fully_qualified_name = internal_to_fqn(&internal_name);
    let super_internal_name = if super_class == 0 {
        None
    } else {
        Some(constant_pool.class_name(super_class)?)
    };
    let super_fully_qualified_name = super_internal_name.as_deref().map(internal_to_fqn);

    let interfaces_count = reader.read_u2()?;
    let mut interfaces = Vec::with_capacity(interfaces_count as usize);
    for _ in 0..interfaces_count {
        interfaces.push(constant_pool.class_reference(reader.read_u2()?)?);
    }

    let fields = parse_fields(&mut reader, &constant_pool)?;
    let mut methods = parse_methods(&mut reader, &constant_pool)?;
    let attributes = parse_attributes(&mut reader, &constant_pool)?;
    enrich_methods_with_bootstrap_arguments(&mut methods, &attributes.bootstrap_methods);
    if !reader.is_done() {
        bail!("class file has trailing bytes after attributes")
    }

    Ok(ParsedClass {
        internal_name,
        fully_qualified_name,
        super_internal_name,
        super_fully_qualified_name,
        interfaces,
        minor_version,
        major_version,
        access_flags,
        access_flags_text: class_access_flags(access_flags),
        source_file: attributes.source_file,
        signature: attributes.signature,
        fields,
        methods,
    })
}

fn parse_fields(
    reader: &mut ClassReader<'_>,
    constant_pool: &ConstantPool,
) -> Result<Vec<FieldEntry>> {
    let fields_count = reader.read_u2()?;
    let mut fields = Vec::with_capacity(fields_count as usize);
    for _ in 0..fields_count {
        let access_flags = reader.read_u2()?;
        let name = constant_pool.utf8(reader.read_u2()?)?.to_string();
        let descriptor = constant_pool.utf8(reader.read_u2()?)?.to_string();
        let attributes = parse_attributes(reader, constant_pool)?;
        fields.push(FieldEntry {
            name,
            type_name: parse_field_descriptor(&descriptor).ok(),
            descriptor,
            access_flags,
            access_flags_text: field_access_flags(access_flags),
            signature: attributes.signature,
            constant_value: attributes.constant_value,
        });
    }
    Ok(fields)
}

fn parse_methods(
    reader: &mut ClassReader<'_>,
    constant_pool: &ConstantPool,
) -> Result<Vec<MethodEntry>> {
    let methods_count = reader.read_u2()?;
    let mut methods = Vec::with_capacity(methods_count as usize);
    for _ in 0..methods_count {
        let access_flags = reader.read_u2()?;
        let name = constant_pool.utf8(reader.read_u2()?)?.to_string();
        let descriptor = constant_pool.utf8(reader.read_u2()?)?.to_string();
        let attributes = parse_attributes(reader, constant_pool)?;
        let (parameter_types, return_type) = parse_method_descriptor(&descriptor)
            .map(|(parameters, return_type)| (parameters, Some(return_type)))
            .unwrap_or_else(|_| (Vec::new(), None));
        methods.push(MethodEntry {
            name,
            descriptor,
            parameter_types,
            return_type,
            access_flags,
            access_flags_text: method_access_flags(access_flags),
            signature: attributes.signature,
            exceptions: attributes.exceptions,
            code: attributes.code,
        });
    }
    Ok(methods)
}

fn parse_attributes(
    reader: &mut ClassReader<'_>,
    constant_pool: &ConstantPool,
) -> Result<ParsedAttributes> {
    let attributes_count = reader.read_u2()?;
    let mut attributes = ParsedAttributes::default();
    for _ in 0..attributes_count {
        let name = constant_pool.utf8(reader.read_u2()?)?.to_string();
        let len = usize::try_from(reader.read_u4()?).context("attribute length is too large")?;
        let bytes = reader.read_bytes(len)?;
        match name.as_str() {
            "SourceFile" if len == 2 => {
                attributes.source_file = Some(constant_pool.utf8(attribute_u2(bytes))?.to_string());
            }
            "Signature" if len == 2 => {
                attributes.signature = Some(constant_pool.utf8(attribute_u2(bytes))?.to_string());
            }
            "ConstantValue" if len == 2 => {
                let resolved = constant_pool.resolve(attribute_u2(bytes))?;
                attributes.constant_value = resolved_constant_expr(&resolved);
            }
            "Exceptions" => {
                attributes.exceptions = parse_exceptions_attribute(bytes, constant_pool)?;
            }
            "Code" => {
                attributes.code = Some(parse_code_attribute(bytes, constant_pool)?);
            }
            "BootstrapMethods" => {
                attributes.bootstrap_methods = parse_bootstrap_methods(bytes, constant_pool)?;
            }
            _ => {}
        }
    }
    Ok(attributes)
}

fn parse_bootstrap_methods(
    bytes: &[u8],
    constant_pool: &ConstantPool,
) -> Result<Vec<BootstrapMethodEntry>> {
    let mut reader = ClassReader { bytes, offset: 0 };
    let method_count = reader.read_u2()?;
    let mut methods = Vec::with_capacity(method_count as usize);
    for _ in 0..method_count {
        let _bootstrap_method_ref = reader.read_u2()?;
        let argument_count = reader.read_u2()?;
        let mut arguments = Vec::with_capacity(argument_count as usize);
        for _ in 0..argument_count {
            let argument_index = reader.read_u2()?;
            if let Ok(resolved) = constant_pool.resolve(argument_index) {
                if let Some(argument) = bootstrap_argument_expr(&resolved) {
                    arguments.push(argument);
                }
            }
        }
        methods.push(BootstrapMethodEntry { arguments });
    }
    if !reader.is_done() {
        bail!("BootstrapMethods attribute has trailing bytes")
    }
    Ok(methods)
}

fn parse_exceptions_attribute(
    bytes: &[u8],
    constant_pool: &ConstantPool,
) -> Result<Vec<ClassReference>> {
    let mut reader = ClassReader { bytes, offset: 0 };
    let exception_count = reader.read_u2()?;
    let mut exceptions = Vec::with_capacity(exception_count as usize);
    for _ in 0..exception_count {
        exceptions.push(constant_pool.class_reference(reader.read_u2()?)?);
    }
    if !reader.is_done() {
        bail!("Exceptions attribute has trailing bytes")
    }
    Ok(exceptions)
}

fn enrich_methods_with_bootstrap_arguments(
    methods: &mut [MethodEntry],
    bootstrap_methods: &[BootstrapMethodEntry],
) {
    if bootstrap_methods.is_empty() {
        return;
    }
    for method in methods {
        let Some(code) = method.code.as_mut() else {
            continue;
        };
        let dynamic_calls = code
            .instructions
            .iter()
            .filter(|instruction| instruction.opcode == 0xba)
            .filter_map(|instruction| {
                let bootstrap_index = instruction
                    .operands
                    .first()
                    .and_then(|operand| operand.resolved.as_ref())
                    .and_then(|resolved| resolved.bootstrap_method_attr_index)?;
                let arguments = bootstrap_methods
                    .get(bootstrap_index as usize)
                    .map(|entry| entry.arguments.clone())?;
                Some((instruction.offset, arguments))
            })
            .collect::<Vec<_>>();
        for (offset, arguments) in dynamic_calls {
            if let Some(entry) = code
                .body_ir
                .iter_mut()
                .find(|entry| entry.offset == offset && entry.operation == "call")
            {
                entry.bootstrap_arguments = arguments;
            }
        }
    }
}

fn attribute_u2(bytes: &[u8]) -> u16 {
    u16::from_be_bytes([bytes[0], bytes[1]])
}

#[derive(Default)]
struct ParsedCodeAttributes {
    line_numbers: Vec<LineNumberEntry>,
    local_variables: Vec<LocalVariableEntry>,
    local_variable_types: Vec<LocalVariableTypeEntry>,
}

struct LocalVariableTypeEntry {
    start_pc: u16,
    length: u16,
    name: String,
    signature: String,
    index: u16,
}

fn parse_code_attribute(bytes: &[u8], constant_pool: &ConstantPool) -> Result<MethodCodeEntry> {
    let mut reader = ClassReader { bytes, offset: 0 };
    let max_stack = reader.read_u2()?;
    let max_locals = reader.read_u2()?;
    let bytecode_length =
        usize::try_from(reader.read_u4()?).context("bytecode length is too large")?;
    let bytecode = reader.read_bytes(bytecode_length)?;
    let instructions = decode_bytecode(bytecode, constant_pool)?;

    let exception_table_count = reader.read_u2()?;
    let mut exception_table = Vec::with_capacity(exception_table_count as usize);
    for _ in 0..exception_table_count {
        let start_pc = reader.read_u2()?;
        let end_pc = reader.read_u2()?;
        let handler_pc = reader.read_u2()?;
        let catch_type_index = reader.read_u2()?;
        exception_table.push(ExceptionHandlerEntry {
            start_pc,
            end_pc,
            handler_pc,
            catch_type: if catch_type_index == 0 {
                None
            } else {
                Some(constant_pool.class_reference(catch_type_index)?)
            },
        });
    }

    let code_attributes = parse_code_attributes(&mut reader, constant_pool)?;
    if !reader.is_done() {
        bail!("Code attribute has trailing bytes")
    }
    let body_ir = build_method_body_ir(
        &instructions,
        &code_attributes.local_variables,
        &exception_table,
    );

    Ok(MethodCodeEntry {
        max_stack,
        max_locals,
        bytecode_length,
        instructions,
        body_ir,
        exception_table,
        line_numbers: code_attributes.line_numbers,
        local_variables: code_attributes.local_variables,
    })
}

fn parse_code_attributes(
    reader: &mut ClassReader<'_>,
    constant_pool: &ConstantPool,
) -> Result<ParsedCodeAttributes> {
    let attributes_count = reader.read_u2()?;
    let mut attributes = ParsedCodeAttributes::default();
    for _ in 0..attributes_count {
        let name = constant_pool.utf8(reader.read_u2()?)?.to_string();
        let len = usize::try_from(reader.read_u4()?).context("attribute length is too large")?;
        let bytes = reader.read_bytes(len)?;
        match name.as_str() {
            "LineNumberTable" => {
                attributes.line_numbers = parse_line_number_table(bytes)?;
            }
            "LocalVariableTable" => {
                attributes.local_variables = parse_local_variable_table(bytes, constant_pool)?;
            }
            "LocalVariableTypeTable" => {
                attributes.local_variable_types =
                    parse_local_variable_type_table(bytes, constant_pool)?;
            }
            _ => {}
        }
    }
    merge_local_variable_signatures(
        &mut attributes.local_variables,
        &attributes.local_variable_types,
    );
    Ok(attributes)
}

fn parse_line_number_table(bytes: &[u8]) -> Result<Vec<LineNumberEntry>> {
    let mut reader = ClassReader { bytes, offset: 0 };
    let line_number_table_length = reader.read_u2()?;
    let mut entries = Vec::with_capacity(line_number_table_length as usize);
    for _ in 0..line_number_table_length {
        entries.push(LineNumberEntry {
            start_pc: reader.read_u2()?,
            line_number: reader.read_u2()?,
        });
    }
    if !reader.is_done() {
        bail!("LineNumberTable attribute has trailing bytes")
    }
    Ok(entries)
}

fn parse_local_variable_table(
    bytes: &[u8],
    constant_pool: &ConstantPool,
) -> Result<Vec<LocalVariableEntry>> {
    let mut reader = ClassReader { bytes, offset: 0 };
    let local_variable_table_length = reader.read_u2()?;
    let mut entries = Vec::with_capacity(local_variable_table_length as usize);
    for _ in 0..local_variable_table_length {
        let start_pc = reader.read_u2()?;
        let length = reader.read_u2()?;
        let name = constant_pool.utf8(reader.read_u2()?)?.to_string();
        let descriptor = constant_pool.utf8(reader.read_u2()?)?.to_string();
        let index = reader.read_u2()?;
        entries.push(LocalVariableEntry {
            start_pc,
            length,
            name,
            type_name: parse_field_descriptor(&descriptor).ok(),
            descriptor,
            signature: None,
            index,
        });
    }
    if !reader.is_done() {
        bail!("LocalVariableTable attribute has trailing bytes")
    }
    Ok(entries)
}

fn parse_local_variable_type_table(
    bytes: &[u8],
    constant_pool: &ConstantPool,
) -> Result<Vec<LocalVariableTypeEntry>> {
    let mut reader = ClassReader { bytes, offset: 0 };
    let local_variable_type_table_length = reader.read_u2()?;
    let mut entries = Vec::with_capacity(local_variable_type_table_length as usize);
    for _ in 0..local_variable_type_table_length {
        let start_pc = reader.read_u2()?;
        let length = reader.read_u2()?;
        let name = constant_pool.utf8(reader.read_u2()?)?.to_string();
        let signature = constant_pool.utf8(reader.read_u2()?)?.to_string();
        let index = reader.read_u2()?;
        entries.push(LocalVariableTypeEntry {
            start_pc,
            length,
            name,
            signature,
            index,
        });
    }
    if !reader.is_done() {
        bail!("LocalVariableTypeTable attribute has trailing bytes")
    }
    Ok(entries)
}

fn merge_local_variable_signatures(
    local_variables: &mut [LocalVariableEntry],
    local_variable_types: &[LocalVariableTypeEntry],
) {
    for local in local_variables {
        if let Some(local_type) = local_variable_types.iter().find(|local_type| {
            local_type.start_pc == local.start_pc
                && local_type.length == local.length
                && local_type.index == local.index
                && local_type.name == local.name
        }) {
            local.signature = Some(local_type.signature.clone());
        }
    }
}

fn build_method_body_ir(
    instructions: &[BytecodeInstructionEntry],
    local_variables: &[LocalVariableEntry],
    exception_handlers: &[ExceptionHandlerEntry],
) -> Vec<MethodBodyIrEntry> {
    let mut simulator = BodyIrBuilder {
        stack: Vec::new(),
        local_variables,
        exception_handlers,
        entries: Vec::new(),
        next_stack_temp: 1,
    };
    for instruction in instructions {
        simulator.apply(instruction);
    }
    simulator.entries
}

struct BodyIrBuilder<'a> {
    stack: Vec<String>,
    local_variables: &'a [LocalVariableEntry],
    exception_handlers: &'a [ExceptionHandlerEntry],
    entries: Vec<MethodBodyIrEntry>,
    next_stack_temp: usize,
}

struct CallIrMetadata {
    method_full_name: Option<String>,
    signature: Option<String>,
    dispatch_type: Option<&'static str>,
    receiver: Option<String>,
}

impl BodyIrBuilder<'_> {
    fn apply(&mut self, instruction: &BytecodeInstructionEntry) {
        if self
            .exception_handlers
            .iter()
            .any(|handler| handler.handler_pc as u32 == instruction.offset)
        {
            self.stack.clear();
            self.push(CAUGHT_EXCEPTION_REF.to_string());
        }
        match instruction.opcode {
            0x00 => {}
            0x01 => self.push("null".to_string()),
            0x02..=0x08 => self.push((instruction.opcode as i32 - 3).to_string()),
            0x09 | 0x0b | 0x0e => self.push("0".to_string()),
            0x0a | 0x0c | 0x0f => self.push("1".to_string()),
            0x0d => self.push("2".to_string()),
            0x10 | 0x11 => self.push(
                first_operand_value(instruction)
                    .unwrap_or_default()
                    .to_string(),
            ),
            0x12..=0x14 => self.apply_constant_load(instruction),
            0x15..=0x19 => self.apply_indexed_load(instruction),
            0x1a..=0x2d => self.apply_implicit_load(instruction),
            0x2e..=0x35 => self.apply_array_load(instruction),
            0x36..=0x3a => self.apply_indexed_store(instruction),
            0x3b..=0x4e => self.apply_implicit_store(instruction),
            0x4f..=0x56 => self.apply_array_store(instruction),
            0x57 => {
                self.pop();
            }
            0x58 => {
                self.pop();
                self.pop();
            }
            0x59..=0x5f => self.apply_stack_manipulation(instruction),
            0x60..=0x83 => self.apply_binary_or_unary(instruction),
            0x84 => self.apply_increment(instruction),
            0x85..=0x93 => self.apply_cast(instruction),
            0x94..=0x98 => self.apply_compare(instruction),
            0x99..=0xa7 | 0xc6 | 0xc7 | 0xc8 => self.apply_branch(instruction),
            0xa8 | 0xc9 => self.apply_jsr(instruction),
            0xa9 => self.apply_ret(instruction),
            0xaa | 0xab => self.apply_switch(instruction),
            0xac..=0xb1 => self.apply_return(instruction),
            0xb2..=0xb5 => self.apply_field_access(instruction),
            0xb6..=0xba => self.apply_call(instruction),
            0xbb => self.apply_new(instruction),
            0xbc | 0xbd | 0xc5 => self.apply_new_array(instruction),
            0xbe => self.apply_array_length(instruction),
            0xbf => {
                let value = self.pop_expr();
                self.emit_with_args(instruction, "throw", vec![value], None, None);
            }
            0xc0 | 0xc1 => self.apply_type_operation(instruction),
            0xc2 | 0xc3 => {
                let value = self.pop_expr();
                self.emit_with_args(instruction, instruction.mnemonic, vec![value], None, None);
            }
            0xc4 => self.apply_wide(instruction),
            _ => self.emit(
                instruction,
                "unsupported",
                instruction.mnemonic.to_string(),
                None,
                None,
                Vec::new(),
            ),
        }
    }

    fn apply_constant_load(&mut self, instruction: &BytecodeInstructionEntry) {
        let resolved = instruction
            .operands
            .first()
            .and_then(|operand| operand.resolved.as_ref())
            .cloned();
        let value = resolved
            .as_ref()
            .and_then(resolved_constant_expr)
            .unwrap_or_else(|| {
                format!(
                    "cp#{}",
                    first_operand_value(instruction).unwrap_or_default()
                )
            });
        let target = resolved
            .as_ref()
            .and_then(resolved_constant_type)
            .map(str::to_string);
        self.emit(
            instruction,
            "constant",
            value.clone(),
            Some(value.clone()),
            target,
            Vec::new(),
        );
        self.push(value);
    }

    fn apply_indexed_load(&mut self, instruction: &BytecodeInstructionEntry) {
        let index = operand_value(instruction, "index").unwrap_or_default() as u16;
        let local = self.local_expr(index, instruction.offset);
        self.emit(
            instruction,
            "load",
            local.clone(),
            Some(local.clone()),
            None,
            vec![local.clone()],
        );
        self.push(local);
    }

    fn apply_implicit_load(&mut self, instruction: &BytecodeInstructionEntry) {
        let index = implicit_local_index(instruction.opcode);
        let local = self.local_expr(index, instruction.offset);
        self.emit(
            instruction,
            "load",
            local.clone(),
            Some(local.clone()),
            None,
            vec![local.clone()],
        );
        self.push(local);
    }

    fn apply_indexed_store(&mut self, instruction: &BytecodeInstructionEntry) {
        let index = operand_value(instruction, "index").unwrap_or_default() as u16;
        self.store_local(instruction, index);
    }

    fn apply_implicit_store(&mut self, instruction: &BytecodeInstructionEntry) {
        self.store_local(instruction, implicit_local_index(instruction.opcode));
    }

    fn store_local(&mut self, instruction: &BytecodeInstructionEntry, index: u16) {
        let value = self.pop_expr();
        let local = self.local_expr(index, instruction.offset);
        let target = self.local_type(index, instruction.offset).or_else(|| {
            (value == CAUGHT_EXCEPTION_REF)
                .then(|| self.caught_exception_type_at_handler(instruction.offset))
                .flatten()
        });
        self.materialize_stack_occurrences(instruction, &local, target.clone());
        self.emit(
            instruction,
            "assignment",
            format!("{local} = {value}"),
            Some(local),
            target,
            vec![value],
        );
    }

    fn apply_array_load(&mut self, instruction: &BytecodeInstructionEntry) {
        let index = self.pop_expr();
        let array = self.pop_expr();
        let expr = format!("{array}[{index}]");
        let target = self.array_element_type(&array, instruction.offset);
        self.emit(
            instruction,
            "array_load",
            expr.clone(),
            Some(expr.clone()),
            target,
            vec![array, index],
        );
        self.push(expr);
    }

    fn apply_array_store(&mut self, instruction: &BytecodeInstructionEntry) {
        let value = self.pop_expr();
        let index = self.pop_expr();
        let array = self.pop_expr();
        let target = format!("{array}[{index}]");
        let target_type = self.array_element_type(&array, instruction.offset);
        self.materialize_stack_occurrences(instruction, &target, target_type.clone());
        self.materialize_stack_occurrences(instruction, &value, target_type.clone());
        self.emit(
            instruction,
            "array_store",
            format!("{target} = {value}"),
            Some(target),
            target_type,
            vec![array, index, value],
        );
    }

    fn apply_binary_or_unary(&mut self, instruction: &BytecodeInstructionEntry) {
        if let Some(operator) = arithmetic_operator(instruction.opcode) {
            let right = self.pop_expr();
            let left = self.pop_expr();
            let expr = format!("({left} {operator} {right})");
            let target = numeric_result_type(instruction.opcode).map(str::to_string);
            self.emit(
                instruction,
                "binary",
                expr.clone(),
                Some(expr.clone()),
                target,
                vec![left, right],
            );
            self.push(expr);
        } else if let Some(operator) = unary_operator(instruction.opcode) {
            let value = self.pop_expr();
            let expr = format!("({operator}{value})");
            let target = numeric_result_type(instruction.opcode).map(str::to_string);
            self.emit(
                instruction,
                "unary",
                expr.clone(),
                Some(expr.clone()),
                target,
                vec![value],
            );
            self.push(expr);
        } else {
            self.emit(
                instruction,
                "unsupported",
                instruction.mnemonic.to_string(),
                None,
                None,
                Vec::new(),
            );
        }
    }

    fn apply_increment(&mut self, instruction: &BytecodeInstructionEntry) {
        let index = operand_value(instruction, "index").unwrap_or_default() as u16;
        let value = operand_value(instruction, "const").unwrap_or_default();
        let local = self.local_expr(index, instruction.offset);
        let target = self.local_type(index, instruction.offset);
        self.materialize_stack_occurrences(instruction, &local, target.clone());
        let (operator, operand) = if value < 0 {
            ("-", (-value).to_string())
        } else {
            ("+", value.to_string())
        };
        let expr = format!("({local} {operator} {operand})");
        self.emit(
            instruction,
            "binary",
            expr.clone(),
            Some(expr.clone()),
            target.clone(),
            vec![local.clone(), operand],
        );
        self.emit(
            instruction,
            "assignment",
            format!("{local} = {expr}"),
            Some(local),
            target,
            vec![expr],
        );
    }

    fn apply_stack_manipulation(&mut self, instruction: &BytecodeInstructionEntry) {
        match instruction.opcode {
            0x59 => {
                let len = self.stack.len();
                if len >= 1 {
                    self.materialize_duplicated_stack_values(instruction, &[len - 1], false);
                    if let Some(value) = self.stack.last().cloned() {
                        self.push(value);
                    }
                }
            }
            0x5a => {
                let len = self.stack.len();
                if len >= 2 {
                    self.materialize_duplicated_stack_values(instruction, &[len - 1], false);
                    let value1 = self.pop_expr();
                    let value2 = self.pop_expr();
                    self.push(value1.clone());
                    self.push(value2);
                    self.push(value1);
                }
            }
            0x5b => {
                let len = self.stack.len();
                if len >= 3 {
                    self.materialize_duplicated_stack_values(instruction, &[len - 1], false);
                    let value1 = self.pop_expr();
                    let value2 = self.pop_expr();
                    let value3 = self.pop_expr();
                    self.push(value1.clone());
                    self.push(value3);
                    self.push(value2);
                    self.push(value1);
                } else if len >= 2 {
                    self.materialize_duplicated_stack_values(instruction, &[len - 1], false);
                    let value1 = self.pop_expr();
                    let value2 = self.pop_expr();
                    self.push(value1.clone());
                    self.push(value2);
                    self.push(value1);
                }
            }
            0x5c => {
                let len = self.stack.len();
                if len >= 2 {
                    self.materialize_duplicated_stack_values(
                        instruction,
                        &[len - 2, len - 1],
                        false,
                    );
                    let value2 = self.stack[len - 2].clone();
                    let value1 = self.stack[len - 1].clone();
                    self.push(value2);
                    self.push(value1);
                } else if len == 1 {
                    self.materialize_duplicated_stack_values(instruction, &[0], true);
                    if let Some(value) = self.stack.last().cloned() {
                        self.push(value);
                    }
                }
            }
            0x5d => {
                let len = self.stack.len();
                if len >= 3 {
                    self.materialize_duplicated_stack_values(
                        instruction,
                        &[len - 2, len - 1],
                        false,
                    );
                    let value1 = self.pop_expr();
                    let value2 = self.pop_expr();
                    let value3 = self.pop_expr();
                    self.push(value2.clone());
                    self.push(value1.clone());
                    self.push(value3);
                    self.push(value2);
                    self.push(value1);
                }
            }
            0x5e => {
                let len = self.stack.len();
                if len >= 4 {
                    self.materialize_duplicated_stack_values(
                        instruction,
                        &[len - 2, len - 1],
                        false,
                    );
                    let value1 = self.pop_expr();
                    let value2 = self.pop_expr();
                    let value3 = self.pop_expr();
                    let value4 = self.pop_expr();
                    self.push(value2.clone());
                    self.push(value1.clone());
                    self.push(value4);
                    self.push(value3);
                    self.push(value2);
                    self.push(value1);
                }
            }
            0x5f => {
                let len = self.stack.len();
                if len >= 2 {
                    self.stack.swap(len - 1, len - 2);
                }
            }
            _ => {}
        }
    }

    fn apply_cast(&mut self, instruction: &BytecodeInstructionEntry) {
        let value = self.pop_expr();
        let target = primitive_conversion_result_type(instruction.opcode).map(str::to_string);
        let expr = target
            .as_ref()
            .map(|target| format!("({target}) {value}"))
            .unwrap_or_else(|| format!("{}({value})", instruction.mnemonic));
        self.emit(
            instruction,
            "cast",
            expr.clone(),
            Some(expr.clone()),
            target,
            vec![value],
        );
        self.push(expr);
    }

    fn apply_compare(&mut self, instruction: &BytecodeInstructionEntry) {
        let right = self.pop_expr();
        let left = self.pop_expr();
        let expr = format!("{}({left}, {right})", instruction.mnemonic);
        self.emit(
            instruction,
            "compare",
            expr.clone(),
            Some(expr.clone()),
            None,
            vec![left, right],
        );
        self.push(expr);
    }

    fn apply_branch(&mut self, instruction: &BytecodeInstructionEntry) {
        let targets = branch_targets(instruction);
        let target_label = targets
            .first()
            .map(u32::to_string)
            .unwrap_or_else(|| "?".to_string());
        let args: Vec<String> = branch_argument_count(instruction.opcode)
            .map(|count| (0..count).map(|_| self.pop_expr()).collect())
            .unwrap_or_default();
        let code = if args.is_empty() {
            format!("goto {target_label}")
        } else {
            format!(
                "{}({}) -> {}",
                instruction.mnemonic,
                args.iter().rev().cloned().collect::<Vec<_>>().join(", "),
                target_label
            )
        };
        self.emit_control(instruction, "branch", code, targets, args);
    }

    fn apply_jsr(&mut self, instruction: &BytecodeInstructionEntry) {
        let targets = branch_targets(instruction);
        let target_label = targets
            .first()
            .map(u32::to_string)
            .unwrap_or_else(|| "?".to_string());
        let return_address = return_address_expr(jsr_return_offset(instruction));
        self.emit_control(
            instruction,
            "jsr",
            format!("{} {target_label}", instruction.mnemonic),
            targets,
            vec![return_address.clone()],
        );
        self.push(return_address);
    }

    fn apply_ret(&mut self, instruction: &BytecodeInstructionEntry) {
        let index = operand_value(instruction, "index").unwrap_or_default() as u16;
        let local = self.local_expr(index, instruction.offset);
        let targets = self.ret_targets_for_local(&local);
        self.emit_control(
            instruction,
            "ret",
            format!("ret {local}"),
            targets,
            vec![local],
        );
    }

    fn apply_switch(&mut self, instruction: &BytecodeInstructionEntry) {
        let selector = self.pop_expr();
        self.emit_control(
            instruction,
            "switch",
            format!("{}({selector})", instruction.mnemonic),
            branch_targets(instruction),
            vec![selector],
        );
    }

    fn apply_return(&mut self, instruction: &BytecodeInstructionEntry) {
        let args = if instruction.opcode == 0xb1 {
            Vec::new()
        } else {
            vec![self.pop_expr()]
        };
        let code = if args.is_empty() {
            "return".to_string()
        } else {
            format!("return {}", args[0])
        };
        self.emit(instruction, "return", code, None, None, args);
    }

    fn apply_field_access(&mut self, instruction: &BytecodeInstructionEntry) {
        let resolved = instruction
            .operands
            .first()
            .and_then(|operand| operand.resolved.as_ref());
        let field_name = resolved_field_name(resolved);
        match instruction.opcode {
            0xb2 => {
                if let Some(class_literal) =
                    resolved.and_then(primitive_class_literal_from_type_field)
                {
                    self.emit(
                        instruction,
                        "constant",
                        class_literal.clone(),
                        Some(class_literal.clone()),
                        Some("java.lang.Class".to_string()),
                        Vec::new(),
                    );
                    self.push(class_literal);
                    return;
                }
                let target_type = resolved.and_then(|entry| entry.field_type.clone());
                self.emit(
                    instruction,
                    "field_load",
                    field_name.clone(),
                    Some(field_name.clone()),
                    target_type,
                    Vec::new(),
                );
                self.push(field_name);
            }
            0xb3 => {
                let value = self.pop_expr();
                let target_type = resolved.and_then(|entry| entry.field_type.clone());
                self.materialize_stack_occurrences(instruction, &field_name, target_type.clone());
                self.materialize_stack_occurrences(instruction, &value, target_type.clone());
                self.emit(
                    instruction,
                    "field_store",
                    format!("{field_name} = {value}"),
                    Some(field_name),
                    target_type,
                    vec![value],
                );
            }
            0xb4 => {
                let receiver = self.pop_expr();
                let target = resolved
                    .and_then(|entry| entry.name.as_deref())
                    .map(|name| format!("{receiver}.{name}"))
                    .unwrap_or_else(|| format!("{receiver}.{field_name}"));
                let target_type = resolved.and_then(|entry| entry.field_type.clone());
                self.emit(
                    instruction,
                    "field_load",
                    target.clone(),
                    Some(target.clone()),
                    target_type,
                    vec![receiver],
                );
                self.push(target);
            }
            0xb5 => {
                let value = self.pop_expr();
                let receiver = self.pop_expr();
                let target = resolved
                    .and_then(|entry| entry.name.as_deref())
                    .map(|name| format!("{receiver}.{name}"))
                    .unwrap_or_else(|| format!("{receiver}.{field_name}"));
                let target_type = resolved.and_then(|entry| entry.field_type.clone());
                self.materialize_stack_occurrences(instruction, &target, target_type.clone());
                self.materialize_stack_occurrences(instruction, &value, target_type.clone());
                self.emit(
                    instruction,
                    "field_store",
                    format!("{target} = {value}"),
                    Some(target),
                    target_type,
                    vec![receiver, value],
                );
            }
            _ => {}
        }
    }

    fn apply_call(&mut self, instruction: &BytecodeInstructionEntry) {
        let resolved = instruction
            .operands
            .first()
            .and_then(|operand| operand.resolved.as_ref());
        let parameter_count = resolved.map_or(0, |entry| entry.parameter_types.len());
        let mut args = (0..parameter_count)
            .map(|_| self.pop_expr())
            .collect::<Vec<_>>();
        args.reverse();
        let receiver = if matches!(instruction.opcode, 0xb6 | 0xb7 | 0xb9) {
            Some(self.pop_expr())
        } else {
            None
        };
        let target = resolved_call_name(resolved, receiver.as_deref(), instruction.mnemonic);
        let metadata = CallIrMetadata {
            method_full_name: resolved.and_then(call_method_full_name),
            signature: resolved.and_then(call_signature),
            dispatch_type: Some(call_dispatch_type(instruction.opcode)),
            receiver,
        };
        let mut display_args = Vec::new();
        if let Some(receiver) = metadata.receiver.as_ref() {
            display_args.push(receiver.clone());
        }
        display_args.extend(args);
        let code = format!("{target}({})", display_args.join(", "));
        let returns_value = resolved
            .and_then(|entry| entry.return_type.as_deref())
            .is_some_and(|return_type| return_type != "void");
        let result = returns_value.then(|| code.clone());
        self.emit_call(
            instruction,
            code.clone(),
            result.clone(),
            Some(target),
            display_args,
            metadata,
        );
        if let Some(result) = result {
            self.push(result);
        }
    }

    fn apply_new(&mut self, instruction: &BytecodeInstructionEntry) {
        let class_name = instruction
            .operands
            .first()
            .and_then(|operand| operand.resolved.as_ref())
            .and_then(|entry| entry.class_reference.as_ref())
            .map(|class_reference| class_reference.fully_qualified_name.clone())
            .unwrap_or_else(|| "unknown".to_string());
        let expr = format!("new {class_name}");
        let result = self.next_stack_temp();
        self.emit(
            instruction,
            "alloc",
            expr.clone(),
            Some(result.clone()),
            Some(class_name),
            Vec::new(),
        );
        self.push(result);
    }

    fn apply_new_array(&mut self, instruction: &BytecodeInstructionEntry) {
        let dimensions = if instruction.opcode == 0xc5 {
            operand_value(instruction, "dimensions").unwrap_or(1).max(1) as usize
        } else {
            1
        };
        let mut args = (0..dimensions).map(|_| self.pop_expr()).collect::<Vec<_>>();
        args.reverse();
        let array_type = array_type_for_new_array(instruction, dimensions)
            .unwrap_or_else(|| format!("{}[]", instruction.mnemonic));
        let expr = array_allocation_code(&array_type, &args);
        self.emit(
            instruction,
            "alloc_array",
            expr.clone(),
            Some(expr.clone()),
            Some(array_type),
            args,
        );
        self.push(expr);
    }

    fn apply_array_length(&mut self, instruction: &BytecodeInstructionEntry) {
        let array = self.pop_expr();
        let expr = format!("{array}.length");
        self.emit(
            instruction,
            "array_length",
            expr.clone(),
            Some(expr.clone()),
            Some("int".to_string()),
            vec![array],
        );
        self.push(expr);
    }

    fn apply_type_operation(&mut self, instruction: &BytecodeInstructionEntry) {
        let value = self.pop_expr();
        let target = instruction
            .operands
            .first()
            .and_then(|operand| operand.resolved.as_ref())
            .and_then(|entry| entry.class_reference.as_ref())
            .map(|class_reference| class_reference.fully_qualified_name.clone());
        let code = format!(
            "{}({value}, {})",
            instruction.mnemonic,
            target.as_deref().unwrap_or("?")
        );
        if instruction.opcode == 0xc0 {
            self.emit(
                instruction,
                "cast",
                code.clone(),
                Some(code.clone()),
                target,
                vec![value],
            );
            self.push(code);
        } else {
            self.emit(
                instruction,
                "type_check",
                code.clone(),
                Some(code.clone()),
                target,
                vec![value],
            );
            self.push(code);
        }
    }

    fn apply_wide(&mut self, instruction: &BytecodeInstructionEntry) {
        let modified_opcode =
            operand_value(instruction, "modified_opcode").unwrap_or_default() as u8;
        if modified_opcode == 0x84 {
            self.apply_increment(instruction);
        } else if matches!(modified_opcode, 0x15..=0x19) {
            self.apply_indexed_load(instruction);
        } else if matches!(modified_opcode, 0x36..=0x3a) {
            self.apply_indexed_store(instruction);
        } else if modified_opcode == 0xa9 {
            self.apply_ret(instruction);
        } else {
            self.emit(
                instruction,
                "unsupported",
                instruction.mnemonic.to_string(),
                None,
                None,
                Vec::new(),
            );
        }
    }

    fn local_expr(&self, index: u16, offset: u32) -> String {
        self.local_variables
            .iter()
            .find(|local| {
                local.index == index
                    && offset >= local.start_pc as u32
                    && offset < (local.start_pc as u32 + local.length as u32)
            })
            .or_else(|| {
                self.local_variables
                    .iter()
                    .find(|local| local.index == index)
            })
            .map(|local| local.name.clone())
            .unwrap_or_else(|| format!("l{index}"))
    }

    fn local_type(&self, index: u16, offset: u32) -> Option<String> {
        self.local_variables
            .iter()
            .find(|local| {
                local.index == index
                    && offset >= local.start_pc as u32
                    && offset < (local.start_pc as u32 + local.length as u32)
            })
            .or_else(|| {
                self.local_variables
                    .iter()
                    .find(|local| local.index == index)
            })
            .and_then(|local| local.type_name.clone())
    }

    fn caught_exception_type_at_handler(&self, offset: u32) -> Option<String> {
        self.exception_handlers
            .iter()
            .find(|handler| handler.handler_pc as u32 == offset)
            .map(|handler| {
                handler
                    .catch_type
                    .as_ref()
                    .map(|catch_type| catch_type.fully_qualified_name.clone())
                    .unwrap_or_else(|| "java.lang.Throwable".to_string())
            })
    }

    fn ret_targets_for_local(&self, local: &str) -> Vec<u32> {
        self.entries
            .iter()
            .rev()
            .find(|entry| {
                entry.operation == "assignment"
                    && entry.result.as_deref() == Some(local)
                    && entry
                        .arguments
                        .first()
                        .is_some_and(|argument| argument.starts_with("@retaddr"))
            })
            .and_then(|entry| entry.arguments.first())
            .and_then(|argument| parse_return_address_expr(argument))
            .into_iter()
            .collect()
    }

    fn next_stack_temp(&mut self) -> String {
        let temp = format!("$stack{}", self.next_stack_temp);
        self.next_stack_temp += 1;
        temp
    }

    fn materialize_duplicated_stack_values(
        &mut self,
        instruction: &BytecodeInstructionEntry,
        indexes: &[usize],
        materialize_simple_values: bool,
    ) {
        let mut values = Vec::new();
        for index in indexes {
            let Some(value) = self.stack.get(*index) else {
                continue;
            };
            if materialize_simple_values || should_materialize_duplicated_value(value) {
                values.push(value.clone());
            }
        }
        values.sort();
        values.dedup();
        for value in values {
            let target = self
                .type_for_stack_value(&value, instruction.offset)
                .or_else(|| Some("ANY".to_string()));
            self.materialize_stack_value(instruction, &value, target);
        }
    }

    fn materialize_stack_occurrences(
        &mut self,
        instruction: &BytecodeInstructionEntry,
        value: &str,
        target: Option<String>,
    ) {
        if !should_materialize_stack_occurrence(value)
            || !self.stack.iter().any(|item| item == value)
        {
            return;
        }
        let target = target
            .or_else(|| self.type_for_stack_value(value, instruction.offset))
            .or_else(|| Some("ANY".to_string()));
        self.materialize_stack_value(instruction, value, target);
    }

    fn materialize_stack_value(
        &mut self,
        instruction: &BytecodeInstructionEntry,
        value: &str,
        target: Option<String>,
    ) -> Option<String> {
        if value.starts_with("$stack") {
            return Some(value.to_string());
        }
        if value.starts_with("new ") {
            return self.materialize_stack_allocation(value);
        }
        if !self.stack.iter().any(|item| item == value) {
            return None;
        }
        let temp = self.next_stack_temp();
        for item in &mut self.stack {
            if item == value {
                *item = temp.clone();
            }
        }
        self.emit(
            instruction,
            "assignment",
            format!("{temp} = {value}"),
            Some(temp.clone()),
            target,
            vec![value.to_string()],
        );
        Some(temp)
    }

    fn materialize_stack_allocation(&mut self, value: &str) -> Option<String> {
        if !value.starts_with("new ") {
            return None;
        }
        let entry_index = self.entries.iter().rposition(|entry| {
            matches!(entry.operation, "alloc" | "alloc_array")
                && entry.result.as_deref() == Some(value)
        })?;
        let temp = self.next_stack_temp();
        self.entries[entry_index].result = Some(temp.clone());
        for item in &mut self.stack {
            if item == value {
                *item = temp.clone();
            }
        }
        Some(temp)
    }

    fn type_for_stack_value(&self, value: &str, offset: u32) -> Option<String> {
        self.local_variables
            .iter()
            .find(|local| {
                local.name == value
                    && offset >= local.start_pc as u32
                    && offset < (local.start_pc as u32 + local.length as u32)
            })
            .or_else(|| {
                self.local_variables
                    .iter()
                    .find(|local| local.name == value)
            })
            .and_then(|local| local.type_name.clone())
            .or_else(|| {
                self.entries
                    .iter()
                    .rev()
                    .find(|entry| entry.result.as_deref() == Some(value))
                    .and_then(|entry| entry.target.clone())
            })
    }

    fn array_element_type(&self, array: &str, offset: u32) -> Option<String> {
        self.type_for_stack_value(array, offset)
            .and_then(|type_name| array_element_type_name(&type_name))
    }

    fn push(&mut self, value: String) {
        self.stack.push(value);
    }

    fn pop(&mut self) -> Option<String> {
        self.stack.pop()
    }

    fn pop_expr(&mut self) -> String {
        self.pop().unwrap_or_else(|| "<stack>".to_string())
    }

    fn emit_with_args(
        &mut self,
        instruction: &BytecodeInstructionEntry,
        operation: &'static str,
        args: Vec<String>,
        result: Option<String>,
        target: Option<String>,
    ) {
        self.emit(
            instruction,
            operation,
            format!("{}({})", instruction.mnemonic, args.join(", ")),
            result,
            target,
            args,
        );
    }

    fn emit(
        &mut self,
        instruction: &BytecodeInstructionEntry,
        operation: &'static str,
        code: String,
        result: Option<String>,
        target: Option<String>,
        arguments: Vec<String>,
    ) {
        self.entries.push(MethodBodyIrEntry {
            offset: instruction.offset,
            operation,
            code,
            result,
            target,
            method_full_name: None,
            signature: None,
            dispatch_type: None,
            receiver: None,
            targets: Vec::new(),
            arguments,
            bootstrap_arguments: Vec::new(),
        });
    }

    fn emit_call(
        &mut self,
        instruction: &BytecodeInstructionEntry,
        code: String,
        result: Option<String>,
        target: Option<String>,
        arguments: Vec<String>,
        metadata: CallIrMetadata,
    ) {
        self.entries.push(MethodBodyIrEntry {
            offset: instruction.offset,
            operation: "call",
            code,
            result,
            target,
            method_full_name: metadata.method_full_name,
            signature: metadata.signature,
            dispatch_type: metadata.dispatch_type,
            receiver: metadata.receiver,
            targets: Vec::new(),
            arguments,
            bootstrap_arguments: Vec::new(),
        });
    }

    fn emit_control(
        &mut self,
        instruction: &BytecodeInstructionEntry,
        operation: &'static str,
        code: String,
        targets: Vec<u32>,
        arguments: Vec<String>,
    ) {
        let target = targets.first().map(|target| target.to_string());
        self.entries.push(MethodBodyIrEntry {
            offset: instruction.offset,
            operation,
            code,
            result: None,
            target,
            method_full_name: None,
            signature: None,
            dispatch_type: None,
            receiver: None,
            targets,
            arguments,
            bootstrap_arguments: Vec::new(),
        });
    }
}

fn should_materialize_duplicated_value(value: &str) -> bool {
    value.starts_with("new ") || (!is_literal_ir_value(value) && !is_simple_stack_identifier(value))
}

fn should_materialize_stack_occurrence(value: &str) -> bool {
    !value.starts_with("$stack") && !is_literal_ir_value(value)
}

fn is_simple_stack_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_' || first == '$')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

fn is_literal_ir_value(value: &str) -> bool {
    value == "null"
        || value.parse::<i64>().is_ok()
        || value.parse::<f64>().is_ok()
        || (value.starts_with('"') && value.ends_with('"'))
}

fn array_element_type_name(type_name: &str) -> Option<String> {
    type_name
        .strip_suffix("[]")
        .filter(|element_type| !element_type.is_empty())
        .map(str::to_string)
}

fn branch_targets(instruction: &BytecodeInstructionEntry) -> Vec<u32> {
    let mut targets = Vec::new();
    for operand in &instruction.operands {
        if operand.kind == "branch" && operand.value >= 0 {
            let target = operand.value as u32;
            if !targets.contains(&target) {
                targets.push(target);
            }
        }
    }
    targets
}

fn jsr_return_offset(instruction: &BytecodeInstructionEntry) -> u32 {
    let width = match instruction.opcode {
        0xa8 => 3,
        0xc9 => 5,
        _ => 1,
    };
    instruction.offset + width
}

fn return_address_expr(offset: u32) -> String {
    format!("@retaddr{offset}")
}

fn parse_return_address_expr(value: &str) -> Option<u32> {
    value.strip_prefix("@retaddr")?.parse().ok()
}

fn first_operand_value(instruction: &BytecodeInstructionEntry) -> Option<i32> {
    instruction.operands.first().map(|operand| operand.value)
}

fn operand_value(instruction: &BytecodeInstructionEntry, name: &str) -> Option<i32> {
    instruction
        .operands
        .iter()
        .find(|operand| operand.name == name)
        .map(|operand| operand.value)
}

fn implicit_local_index(opcode: u8) -> u16 {
    match opcode {
        0x1a..=0x1d => (opcode - 0x1a) as u16,
        0x1e..=0x21 => (opcode - 0x1e) as u16,
        0x22..=0x25 => (opcode - 0x22) as u16,
        0x26..=0x29 => (opcode - 0x26) as u16,
        0x2a..=0x2d => (opcode - 0x2a) as u16,
        0x3b..=0x3e => (opcode - 0x3b) as u16,
        0x3f..=0x42 => (opcode - 0x3f) as u16,
        0x43..=0x46 => (opcode - 0x43) as u16,
        0x47..=0x4a => (opcode - 0x47) as u16,
        0x4b..=0x4e => (opcode - 0x4b) as u16,
        _ => 0,
    }
}

fn arithmetic_operator(opcode: u8) -> Option<&'static str> {
    match opcode {
        0x60..=0x63 => Some("+"),
        0x64..=0x67 => Some("-"),
        0x68..=0x6b => Some("*"),
        0x6c..=0x6f => Some("/"),
        0x70..=0x73 => Some("%"),
        0x78 | 0x79 => Some("<<"),
        0x7a | 0x7b => Some(">>"),
        0x7c | 0x7d => Some(">>>"),
        0x7e | 0x7f => Some("&"),
        0x80 | 0x81 => Some("|"),
        0x82 | 0x83 => Some("^"),
        _ => None,
    }
}

fn unary_operator(opcode: u8) -> Option<&'static str> {
    match opcode {
        0x74..=0x77 => Some("-"),
        _ => None,
    }
}

fn numeric_result_type(opcode: u8) -> Option<&'static str> {
    match opcode {
        0x60 | 0x64 | 0x68 | 0x6c | 0x70 | 0x74 | 0x78 | 0x7a | 0x7c | 0x7e | 0x80 | 0x82 => {
            Some("int")
        }
        0x61 | 0x65 | 0x69 | 0x6d | 0x71 | 0x75 | 0x79 | 0x7b | 0x7d | 0x7f | 0x81 | 0x83 => {
            Some("long")
        }
        0x62 | 0x66 | 0x6a | 0x6e | 0x72 | 0x76 => Some("float"),
        0x63 | 0x67 | 0x6b | 0x6f | 0x73 | 0x77 => Some("double"),
        _ => None,
    }
}

fn primitive_conversion_result_type(opcode: u8) -> Option<&'static str> {
    match opcode {
        0x85 | 0x8c | 0x8f => Some("long"),
        0x86 | 0x89 | 0x90 => Some("float"),
        0x87 | 0x8a | 0x8d => Some("double"),
        0x88 | 0x8b | 0x8e => Some("int"),
        0x91 => Some("byte"),
        0x92 => Some("char"),
        0x93 => Some("short"),
        _ => None,
    }
}

fn branch_argument_count(opcode: u8) -> Option<usize> {
    match opcode {
        0x99..=0x9e | 0xc6 | 0xc7 => Some(1),
        0x9f..=0xa6 => Some(2),
        0xa7 | 0xa8 | 0xc8 | 0xc9 => Some(0),
        _ => None,
    }
}

fn resolved_constant_expr(resolved: &ResolvedConstantPoolEntry) -> Option<String> {
    resolved
        .value
        .as_ref()
        .map(|value| match resolved.tag {
            "String" | "Utf8" => format!("{value:?}"),
            _ => value.clone(),
        })
        .or_else(|| resolved.class_reference.as_ref().map(class_literal_code))
}

fn resolved_constant_type(resolved: &ResolvedConstantPoolEntry) -> Option<&'static str> {
    match resolved.tag {
        "Integer" => Some("int"),
        "Float" => Some("float"),
        "Long" => Some("long"),
        "Double" => Some("double"),
        "String" | "Utf8" => Some("java.lang.String"),
        "Class" => Some("java.lang.Class"),
        _ => None,
    }
}

fn class_literal_code(class_reference: &ClassReference) -> String {
    format!(
        "{}.class",
        parse_field_descriptor(&class_reference.internal_name)
            .unwrap_or_else(|_| class_reference.fully_qualified_name.clone())
    )
}

fn primitive_class_literal_from_type_field(resolved: &ResolvedConstantPoolEntry) -> Option<String> {
    if resolved.name.as_deref() != Some("TYPE")
        || resolved.field_type.as_deref() != Some("java.lang.Class")
    {
        return None;
    }
    let primitive = match resolved
        .class_reference
        .as_ref()
        .map(|class_reference| class_reference.fully_qualified_name.as_str())
    {
        Some("java.lang.Boolean") => "boolean",
        Some("java.lang.Byte") => "byte",
        Some("java.lang.Character") => "char",
        Some("java.lang.Short") => "short",
        Some("java.lang.Integer") => "int",
        Some("java.lang.Long") => "long",
        Some("java.lang.Float") => "float",
        Some("java.lang.Double") => "double",
        Some("java.lang.Void") => "void",
        _ => return None,
    };
    Some(format!("{primitive}.class"))
}

fn bootstrap_argument_expr(resolved: &ResolvedConstantPoolEntry) -> Option<String> {
    if resolved.tag == "MethodHandle" {
        return call_method_full_name(resolved)
            .or_else(|| resolved.name.clone())
            .or_else(|| resolved_constant_expr(resolved));
    }
    resolved_constant_expr(resolved)
        .or_else(|| call_signature(resolved))
        .or_else(|| resolved.descriptor.clone())
        .or_else(|| resolved.name.clone())
}

fn resolved_field_name(resolved: Option<&ResolvedConstantPoolEntry>) -> String {
    resolved
        .and_then(|entry| {
            let class_name = entry
                .class_reference
                .as_ref()
                .map(|class_reference| class_reference.fully_qualified_name.as_str());
            match (class_name, entry.name.as_deref()) {
                (Some(class_name), Some(name)) => Some(format!("{class_name}.{name}")),
                (_, Some(name)) => Some(name.to_string()),
                _ => None,
            }
        })
        .unwrap_or_else(|| "<field>".to_string())
}

fn resolved_call_name(
    resolved: Option<&ResolvedConstantPoolEntry>,
    receiver: Option<&str>,
    fallback: &str,
) -> String {
    resolved
        .and_then(|entry| {
            match (
                receiver,
                entry.class_reference.as_ref(),
                entry.name.as_deref(),
            ) {
                (Some(receiver), _, Some(name)) => Some(format!("{receiver}.{name}")),
                (None, Some(class_reference), Some(name)) => {
                    Some(format!("{}.{}", class_reference.fully_qualified_name, name))
                }
                (None, None, Some(name)) => Some(name.to_string()),
                _ => None,
            }
        })
        .unwrap_or_else(|| fallback.to_string())
}

fn call_method_full_name(resolved: &ResolvedConstantPoolEntry) -> Option<String> {
    let class_name = resolved
        .class_reference
        .as_ref()?
        .fully_qualified_name
        .as_str();
    let method_name = resolved.name.as_deref()?;
    let signature = call_signature(resolved)?;
    Some(format!("{class_name}.{method_name}:{signature}"))
}

fn call_signature(resolved: &ResolvedConstantPoolEntry) -> Option<String> {
    let return_type = resolved.return_type.as_deref()?;
    Some(format!(
        "{return_type}({})",
        resolved.parameter_types.join(",")
    ))
}

fn call_dispatch_type(opcode: u8) -> &'static str {
    match opcode {
        0xb6 | 0xb9 | 0xba => "DYNAMIC_DISPATCH",
        _ => "STATIC_DISPATCH",
    }
}

fn array_type_for_new_array(
    instruction: &BytecodeInstructionEntry,
    dimensions: usize,
) -> Option<String> {
    match instruction.opcode {
        0xbc => {
            let base = operand_value(instruction, "atype").and_then(primitive_array_type)?;
            Some(add_array_dimensions(base, dimensions))
        }
        0xbd => instruction
            .operands
            .first()
            .and_then(|operand| operand.resolved.as_ref())
            .and_then(|entry| entry.class_reference.as_ref())
            .map(|class_reference| add_array_dimensions(&class_reference.fully_qualified_name, 1)),
        0xc5 => instruction
            .operands
            .first()
            .and_then(|operand| operand.resolved.as_ref())
            .and_then(|entry| entry.class_reference.as_ref())
            .map(|class_reference| class_reference.fully_qualified_name.clone()),
        _ => None,
    }
}

fn primitive_array_type(atype: i32) -> Option<&'static str> {
    match atype {
        4 => Some("boolean"),
        5 => Some("char"),
        6 => Some("float"),
        7 => Some("double"),
        8 => Some("byte"),
        9 => Some("short"),
        10 => Some("int"),
        11 => Some("long"),
        _ => None,
    }
}

fn add_array_dimensions(base: &str, dimensions: usize) -> String {
    let mut array_type = base.to_string();
    for _ in 0..dimensions {
        array_type.push_str("[]");
    }
    array_type
}

fn array_allocation_code(array_type: &str, sizes: &[String]) -> String {
    let (base_type, total_dimensions) = split_array_type(array_type);
    let mut code = format!("new {base_type}");
    for size in sizes {
        code.push('[');
        code.push_str(size);
        code.push(']');
    }
    for _ in sizes.len()..total_dimensions {
        code.push_str("[]");
    }
    code
}

fn split_array_type(array_type: &str) -> (&str, usize) {
    let mut base_end = array_type.len();
    let mut dimensions = 0;
    while base_end >= 2 && &array_type[base_end - 2..base_end] == "[]" {
        dimensions += 1;
        base_end -= 2;
    }
    (&array_type[..base_end], dimensions)
}

fn decode_bytecode(
    bytes: &[u8],
    constant_pool: &ConstantPool,
) -> Result<Vec<BytecodeInstructionEntry>> {
    let mut reader = BytecodeReader { bytes, offset: 0 };
    let mut instructions = Vec::new();

    while !reader.is_done() {
        let offset = reader.offset;
        let opcode = reader.read_u1()?;
        let mnemonic = opcode_mnemonic(opcode);
        if mnemonic == "unknown" {
            bail!("unknown JVM opcode 0x{opcode:02x} at {offset}")
        }
        let operands = decode_instruction_operands(opcode, offset, &mut reader, constant_pool)
            .with_context(|| {
                format!("failed to decode bytecode instruction {mnemonic} at {offset}")
            })?;
        instructions.push(BytecodeInstructionEntry {
            offset: u32::try_from(offset).context("bytecode offset is too large")?,
            opcode,
            mnemonic,
            operands,
        });
    }

    Ok(instructions)
}

fn decode_instruction_operands(
    opcode: u8,
    offset: usize,
    reader: &mut BytecodeReader<'_>,
    constant_pool: &ConstantPool,
) -> Result<Vec<BytecodeOperandEntry>> {
    let mut operands = Vec::new();
    match opcode {
        0x10 => operands.push(operand("value", "i1", reader.read_i1()? as i32)),
        0x11 => operands.push(operand("value", "i2", reader.read_i2()? as i32)),
        0x12 => operands.push(constant_pool_operand(
            "index",
            reader.read_u1()? as u16,
            constant_pool,
        )?),
        0x13 | 0x14 | 0xb2 | 0xb3 | 0xb4 | 0xb5 | 0xb6 | 0xb7 | 0xb8 | 0xbb | 0xbd | 0xc0
        | 0xc1 => operands.push(constant_pool_operand(
            "index",
            reader.read_u2()?,
            constant_pool,
        )?),
        0x15 | 0x16 | 0x17 | 0x18 | 0x19 | 0x36 | 0x37 | 0x38 | 0x39 | 0x3a | 0xa9 => {
            operands.push(operand("index", "local", reader.read_u1()? as i32));
        }
        0x84 => {
            operands.push(operand("index", "local", reader.read_u1()? as i32));
            operands.push(operand("const", "i1", reader.read_i1()? as i32));
        }
        0x99..=0xa8 | 0xc6 | 0xc7 => {
            let relative = reader.read_i2()? as i32;
            operands.push(operand(
                "target",
                "branch",
                branch_target(offset, relative)?,
            ));
        }
        0xaa => decode_table_switch(offset, reader, &mut operands)?,
        0xab => decode_lookup_switch(offset, reader, &mut operands)?,
        0xb9 => {
            operands.push(constant_pool_operand(
                "index",
                reader.read_u2()?,
                constant_pool,
            )?);
            operands.push(operand("count", "u1", reader.read_u1()? as i32));
            operands.push(operand("zero", "u1", reader.read_u1()? as i32));
        }
        0xba => {
            operands.push(constant_pool_operand(
                "index",
                reader.read_u2()?,
                constant_pool,
            )?);
            operands.push(operand("zero1", "u1", reader.read_u1()? as i32));
            operands.push(operand("zero2", "u1", reader.read_u1()? as i32));
        }
        0xbc => operands.push(operand("atype", "atype", reader.read_u1()? as i32)),
        0xc4 => decode_wide(reader, &mut operands)?,
        0xc5 => {
            operands.push(constant_pool_operand(
                "index",
                reader.read_u2()?,
                constant_pool,
            )?);
            operands.push(operand("dimensions", "u1", reader.read_u1()? as i32));
        }
        0xc8 | 0xc9 => {
            let relative = reader.read_i4()?;
            operands.push(operand(
                "target",
                "branch",
                branch_target(offset, relative)?,
            ));
        }
        _ => {}
    }
    Ok(operands)
}

fn decode_table_switch(
    offset: usize,
    reader: &mut BytecodeReader<'_>,
    operands: &mut Vec<BytecodeOperandEntry>,
) -> Result<()> {
    let padding = reader.align_to_four()?;
    operands.push(operand("padding", "u1_count", padding as i32));
    let default = reader.read_i4()?;
    let low = reader.read_i4()?;
    let high = reader.read_i4()?;
    if high < low {
        bail!("tableswitch high value is lower than low value")
    }
    let count = usize::try_from((high as i64) - (low as i64) + 1)
        .context("tableswitch case count overflow")?;
    if count > reader.remaining() / 4 {
        bail!("tableswitch case count exceeds remaining bytecode")
    }
    operands.push(operand(
        "default_target",
        "branch",
        branch_target(offset, default)?,
    ));
    operands.push(operand("low", "i4", low));
    operands.push(operand("high", "i4", high));
    for _ in 0..count {
        let relative = reader.read_i4()?;
        operands.push(operand(
            "case_target",
            "branch",
            branch_target(offset, relative)?,
        ));
    }
    Ok(())
}

fn decode_lookup_switch(
    offset: usize,
    reader: &mut BytecodeReader<'_>,
    operands: &mut Vec<BytecodeOperandEntry>,
) -> Result<()> {
    let padding = reader.align_to_four()?;
    operands.push(operand("padding", "u1_count", padding as i32));
    let default = reader.read_i4()?;
    let pairs = reader.read_i4()?;
    if pairs < 0 {
        bail!("lookupswitch pair count is negative")
    }
    let pairs = usize::try_from(pairs).context("lookupswitch pair count overflow")?;
    if pairs > reader.remaining() / 8 {
        bail!("lookupswitch pair count exceeds remaining bytecode")
    }
    operands.push(operand(
        "default_target",
        "branch",
        branch_target(offset, default)?,
    ));
    operands.push(operand("pairs", "u4_count", pairs as i32));
    for _ in 0..pairs {
        operands.push(operand("match", "i4", reader.read_i4()?));
        let relative = reader.read_i4()?;
        operands.push(operand(
            "target",
            "branch",
            branch_target(offset, relative)?,
        ));
    }
    Ok(())
}

fn decode_wide(
    reader: &mut BytecodeReader<'_>,
    operands: &mut Vec<BytecodeOperandEntry>,
) -> Result<()> {
    let modified_opcode = reader.read_u1()?;
    operands.push(operand("modified_opcode", "opcode", modified_opcode as i32));
    match modified_opcode {
        0x15 | 0x16 | 0x17 | 0x18 | 0x19 | 0x36 | 0x37 | 0x38 | 0x39 | 0x3a | 0xa9 => {
            operands.push(operand("index", "local", reader.read_u2()? as i32));
        }
        0x84 => {
            operands.push(operand("index", "local", reader.read_u2()? as i32));
            operands.push(operand("const", "i2", reader.read_i2()? as i32));
        }
        _ => bail!("invalid wide-modified opcode 0x{modified_opcode:02x}"),
    }
    Ok(())
}

fn branch_target(offset: usize, relative: i32) -> Result<i32> {
    let offset = i32::try_from(offset).context("branch source offset is too large")?;
    offset
        .checked_add(relative)
        .context("branch target overflow")
}

fn operand(name: &'static str, kind: &'static str, value: i32) -> BytecodeOperandEntry {
    BytecodeOperandEntry {
        name,
        kind,
        value,
        resolved: None,
    }
}

fn constant_pool_operand(
    name: &'static str,
    index: u16,
    constant_pool: &ConstantPool,
) -> Result<BytecodeOperandEntry> {
    Ok(BytecodeOperandEntry {
        name,
        kind: "constant_pool",
        value: index as i32,
        resolved: Some(constant_pool.resolve(index)?),
    })
}

struct BytecodeReader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> BytecodeReader<'a> {
    fn read_u1(&mut self) -> Result<u8> {
        Ok(*self.read_bytes(1)?.first().expect("read one byte"))
    }

    fn read_i1(&mut self) -> Result<i8> {
        Ok(self.read_u1()? as i8)
    }

    fn read_u2(&mut self) -> Result<u16> {
        let bytes = self.read_bytes(2)?;
        Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
    }

    fn read_i2(&mut self) -> Result<i16> {
        Ok(self.read_u2()? as i16)
    }

    fn read_i4(&mut self) -> Result<i32> {
        let bytes = self.read_bytes(4)?;
        Ok(i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn read_bytes(&mut self, len: usize) -> Result<&'a [u8]> {
        let end = self
            .offset
            .checked_add(len)
            .context("bytecode offset overflow")?;
        if end > self.bytes.len() {
            bail!("unexpected end of bytecode")
        }
        let slice = &self.bytes[self.offset..end];
        self.offset = end;
        Ok(slice)
    }

    fn align_to_four(&mut self) -> Result<usize> {
        let padding = (4 - (self.offset % 4)) % 4;
        self.read_bytes(padding)?;
        Ok(padding)
    }

    fn remaining(&self) -> usize {
        self.bytes.len() - self.offset
    }

    fn is_done(&self) -> bool {
        self.offset == self.bytes.len()
    }
}

fn opcode_mnemonic(opcode: u8) -> &'static str {
    match opcode {
        0x00 => "nop",
        0x01 => "aconst_null",
        0x02 => "iconst_m1",
        0x03 => "iconst_0",
        0x04 => "iconst_1",
        0x05 => "iconst_2",
        0x06 => "iconst_3",
        0x07 => "iconst_4",
        0x08 => "iconst_5",
        0x09 => "lconst_0",
        0x0a => "lconst_1",
        0x0b => "fconst_0",
        0x0c => "fconst_1",
        0x0d => "fconst_2",
        0x0e => "dconst_0",
        0x0f => "dconst_1",
        0x10 => "bipush",
        0x11 => "sipush",
        0x12 => "ldc",
        0x13 => "ldc_w",
        0x14 => "ldc2_w",
        0x15 => "iload",
        0x16 => "lload",
        0x17 => "fload",
        0x18 => "dload",
        0x19 => "aload",
        0x1a => "iload_0",
        0x1b => "iload_1",
        0x1c => "iload_2",
        0x1d => "iload_3",
        0x1e => "lload_0",
        0x1f => "lload_1",
        0x20 => "lload_2",
        0x21 => "lload_3",
        0x22 => "fload_0",
        0x23 => "fload_1",
        0x24 => "fload_2",
        0x25 => "fload_3",
        0x26 => "dload_0",
        0x27 => "dload_1",
        0x28 => "dload_2",
        0x29 => "dload_3",
        0x2a => "aload_0",
        0x2b => "aload_1",
        0x2c => "aload_2",
        0x2d => "aload_3",
        0x2e => "iaload",
        0x2f => "laload",
        0x30 => "faload",
        0x31 => "daload",
        0x32 => "aaload",
        0x33 => "baload",
        0x34 => "caload",
        0x35 => "saload",
        0x36 => "istore",
        0x37 => "lstore",
        0x38 => "fstore",
        0x39 => "dstore",
        0x3a => "astore",
        0x3b => "istore_0",
        0x3c => "istore_1",
        0x3d => "istore_2",
        0x3e => "istore_3",
        0x3f => "lstore_0",
        0x40 => "lstore_1",
        0x41 => "lstore_2",
        0x42 => "lstore_3",
        0x43 => "fstore_0",
        0x44 => "fstore_1",
        0x45 => "fstore_2",
        0x46 => "fstore_3",
        0x47 => "dstore_0",
        0x48 => "dstore_1",
        0x49 => "dstore_2",
        0x4a => "dstore_3",
        0x4b => "astore_0",
        0x4c => "astore_1",
        0x4d => "astore_2",
        0x4e => "astore_3",
        0x4f => "iastore",
        0x50 => "lastore",
        0x51 => "fastore",
        0x52 => "dastore",
        0x53 => "aastore",
        0x54 => "bastore",
        0x55 => "castore",
        0x56 => "sastore",
        0x57 => "pop",
        0x58 => "pop2",
        0x59 => "dup",
        0x5a => "dup_x1",
        0x5b => "dup_x2",
        0x5c => "dup2",
        0x5d => "dup2_x1",
        0x5e => "dup2_x2",
        0x5f => "swap",
        0x60 => "iadd",
        0x61 => "ladd",
        0x62 => "fadd",
        0x63 => "dadd",
        0x64 => "isub",
        0x65 => "lsub",
        0x66 => "fsub",
        0x67 => "dsub",
        0x68 => "imul",
        0x69 => "lmul",
        0x6a => "fmul",
        0x6b => "dmul",
        0x6c => "idiv",
        0x6d => "ldiv",
        0x6e => "fdiv",
        0x6f => "ddiv",
        0x70 => "irem",
        0x71 => "lrem",
        0x72 => "frem",
        0x73 => "drem",
        0x74 => "ineg",
        0x75 => "lneg",
        0x76 => "fneg",
        0x77 => "dneg",
        0x78 => "ishl",
        0x79 => "lshl",
        0x7a => "ishr",
        0x7b => "lshr",
        0x7c => "iushr",
        0x7d => "lushr",
        0x7e => "iand",
        0x7f => "land",
        0x80 => "ior",
        0x81 => "lor",
        0x82 => "ixor",
        0x83 => "lxor",
        0x84 => "iinc",
        0x85 => "i2l",
        0x86 => "i2f",
        0x87 => "i2d",
        0x88 => "l2i",
        0x89 => "l2f",
        0x8a => "l2d",
        0x8b => "f2i",
        0x8c => "f2l",
        0x8d => "f2d",
        0x8e => "d2i",
        0x8f => "d2l",
        0x90 => "d2f",
        0x91 => "i2b",
        0x92 => "i2c",
        0x93 => "i2s",
        0x94 => "lcmp",
        0x95 => "fcmpl",
        0x96 => "fcmpg",
        0x97 => "dcmpl",
        0x98 => "dcmpg",
        0x99 => "ifeq",
        0x9a => "ifne",
        0x9b => "iflt",
        0x9c => "ifge",
        0x9d => "ifgt",
        0x9e => "ifle",
        0x9f => "if_icmpeq",
        0xa0 => "if_icmpne",
        0xa1 => "if_icmplt",
        0xa2 => "if_icmpge",
        0xa3 => "if_icmpgt",
        0xa4 => "if_icmple",
        0xa5 => "if_acmpeq",
        0xa6 => "if_acmpne",
        0xa7 => "goto",
        0xa8 => "jsr",
        0xa9 => "ret",
        0xaa => "tableswitch",
        0xab => "lookupswitch",
        0xac => "ireturn",
        0xad => "lreturn",
        0xae => "freturn",
        0xaf => "dreturn",
        0xb0 => "areturn",
        0xb1 => "return",
        0xb2 => "getstatic",
        0xb3 => "putstatic",
        0xb4 => "getfield",
        0xb5 => "putfield",
        0xb6 => "invokevirtual",
        0xb7 => "invokespecial",
        0xb8 => "invokestatic",
        0xb9 => "invokeinterface",
        0xba => "invokedynamic",
        0xbb => "new",
        0xbc => "newarray",
        0xbd => "anewarray",
        0xbe => "arraylength",
        0xbf => "athrow",
        0xc0 => "checkcast",
        0xc1 => "instanceof",
        0xc2 => "monitorenter",
        0xc3 => "monitorexit",
        0xc4 => "wide",
        0xc5 => "multianewarray",
        0xc6 => "ifnull",
        0xc7 => "ifnonnull",
        0xc8 => "goto_w",
        0xc9 => "jsr_w",
        0xca => "breakpoint",
        0xfe => "impdep1",
        0xff => "impdep2",
        _ => "unknown",
    }
}

fn internal_to_fqn(internal_name: &str) -> String {
    if internal_name.starts_with('[') {
        parse_field_descriptor(internal_name).unwrap_or_else(|_| internal_name.replace('/', "."))
    } else {
        internal_name.replace('/', ".")
    }
}

fn parse_field_descriptor(descriptor: &str) -> Result<String> {
    let mut offset = 0;
    let ty = parse_type_descriptor(descriptor.as_bytes(), &mut offset, false)?;
    if offset != descriptor.len() {
        bail!("unexpected trailing field descriptor bytes")
    }
    Ok(ty)
}

fn parse_method_descriptor(descriptor: &str) -> Result<(Vec<String>, String)> {
    let bytes = descriptor.as_bytes();
    if bytes.first() != Some(&b'(') {
        bail!("method descriptor does not start with '('")
    }
    let mut offset = 1;
    let mut parameters = Vec::new();
    while offset < bytes.len() && bytes[offset] != b')' {
        parameters.push(parse_type_descriptor(bytes, &mut offset, false)?);
    }
    if offset >= bytes.len() || bytes[offset] != b')' {
        bail!("method descriptor is missing ')'")
    }
    offset += 1;
    let return_type = parse_type_descriptor(bytes, &mut offset, true)?;
    if offset != bytes.len() {
        bail!("unexpected trailing method descriptor bytes")
    }
    Ok((parameters, return_type))
}

fn parse_type_descriptor(bytes: &[u8], offset: &mut usize, allow_void: bool) -> Result<String> {
    let mut dimensions = 0;
    while *offset < bytes.len() && bytes[*offset] == b'[' {
        dimensions += 1;
        *offset += 1;
    }

    if *offset >= bytes.len() {
        bail!("unexpected end of type descriptor")
    }

    let base = match bytes[*offset] {
        b'B' => {
            *offset += 1;
            "byte".to_string()
        }
        b'C' => {
            *offset += 1;
            "char".to_string()
        }
        b'D' => {
            *offset += 1;
            "double".to_string()
        }
        b'F' => {
            *offset += 1;
            "float".to_string()
        }
        b'I' => {
            *offset += 1;
            "int".to_string()
        }
        b'J' => {
            *offset += 1;
            "long".to_string()
        }
        b'S' => {
            *offset += 1;
            "short".to_string()
        }
        b'Z' => {
            *offset += 1;
            "boolean".to_string()
        }
        b'V' if allow_void && dimensions == 0 => {
            *offset += 1;
            "void".to_string()
        }
        b'L' => {
            *offset += 1;
            let start = *offset;
            while *offset < bytes.len() && bytes[*offset] != b';' {
                *offset += 1;
            }
            if *offset >= bytes.len() || bytes[*offset] != b';' {
                bail!("object descriptor is missing ';'")
            }
            let internal_name = std::str::from_utf8(&bytes[start..*offset])
                .context("object descriptor is not valid utf-8")?;
            *offset += 1;
            internal_to_fqn(internal_name)
        }
        other => bail!("unsupported descriptor tag '{}'", other as char),
    };

    let mut ty = base;
    for _ in 0..dimensions {
        ty.push_str("[]");
    }
    Ok(ty)
}

fn class_access_flags(flags: u16) -> Vec<&'static str> {
    flag_names(
        flags,
        &[
            (0x0001, "public"),
            (0x0010, "final"),
            (0x0020, "super"),
            (0x0200, "interface"),
            (0x0400, "abstract"),
            (0x1000, "synthetic"),
            (0x2000, "annotation"),
            (0x4000, "enum"),
            (0x8000, "module"),
        ],
    )
}

fn field_access_flags(flags: u16) -> Vec<&'static str> {
    flag_names(
        flags,
        &[
            (0x0001, "public"),
            (0x0002, "private"),
            (0x0004, "protected"),
            (0x0008, "static"),
            (0x0010, "final"),
            (0x0040, "volatile"),
            (0x0080, "transient"),
            (0x1000, "synthetic"),
            (0x4000, "enum"),
        ],
    )
}

fn method_access_flags(flags: u16) -> Vec<&'static str> {
    flag_names(
        flags,
        &[
            (0x0001, "public"),
            (0x0002, "private"),
            (0x0004, "protected"),
            (0x0008, "static"),
            (0x0010, "final"),
            (0x0020, "synchronized"),
            (0x0040, "bridge"),
            (0x0080, "varargs"),
            (0x0100, "native"),
            (0x0400, "abstract"),
            (0x0800, "strict"),
            (0x1000, "synthetic"),
        ],
    )
}

fn flag_names(flags: u16, known: &[(u16, &'static str)]) -> Vec<&'static str> {
    known
        .iter()
        .filter_map(|(bit, name)| ((flags & bit) != 0).then_some(*name))
        .collect()
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

    fn read_u8(&mut self) -> Result<u64> {
        let bytes = self.read_bytes(8)?;
        Ok(u64::from_be_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
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

    fn is_done(&self) -> bool {
        self.offset == self.bytes.len()
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

    #[test]
    fn parses_declaration_metadata_from_class_file() {
        let info = parse_class_file(&class_with_members()).unwrap();

        assert_eq!(info.internal_name, "pkg/Foo");
        assert_eq!(info.fully_qualified_name, "pkg.Foo");
        assert_eq!(
            info.super_internal_name.as_deref(),
            Some("java/lang/Object")
        );
        assert_eq!(
            info.super_fully_qualified_name.as_deref(),
            Some("java.lang.Object")
        );
        assert_eq!(
            info.interfaces,
            vec![ClassReference {
                internal_name: "java/io/Serializable".to_string(),
                fully_qualified_name: "java.io.Serializable".to_string(),
            }]
        );
        assert_eq!(info.major_version, 52);
        assert!(info.access_flags_text.contains(&"public"));
        assert!(info.access_flags_text.contains(&"super"));
        assert_eq!(info.source_file.as_deref(), Some("Foo.java"));

        let value_field = info
            .fields
            .iter()
            .find(|field| field.name == "value")
            .expect("value field");
        assert_eq!(value_field.descriptor, "I");
        assert_eq!(value_field.type_name.as_deref(), Some("int"));
        assert_eq!(value_field.constant_value.as_deref(), Some("7"));
        assert!(value_field.access_flags_text.contains(&"public"));

        let names_field = info
            .fields
            .iter()
            .find(|field| field.name == "names")
            .expect("names field");
        assert_eq!(names_field.type_name.as_deref(), Some("java.lang.String[]"));
        assert_eq!(names_field.constant_value, None);
        assert!(names_field.access_flags_text.contains(&"private"));

        let answer_method = info
            .methods
            .iter()
            .find(|method| method.name == "answer")
            .expect("answer method");
        assert_eq!(
            answer_method.descriptor,
            "(ILjava/lang/String;[I)Ljava/lang/String;"
        );
        assert_eq!(
            answer_method.parameter_types,
            vec![
                "int".to_string(),
                "java.lang.String".to_string(),
                "int[]".to_string(),
            ]
        );
        assert_eq!(
            answer_method.return_type.as_deref(),
            Some("java.lang.String")
        );
        assert_eq!(
            answer_method.exceptions,
            vec![ClassReference {
                internal_name: "java/lang/Exception".to_string(),
                fully_qualified_name: "java.lang.Exception".to_string(),
            }]
        );
        assert!(answer_method.access_flags_text.contains(&"public"));
        let code = answer_method.code.as_ref().expect("answer code");
        assert_eq!(code.max_stack, 1);
        assert_eq!(code.max_locals, 4);
        assert_eq!(code.bytecode_length, 1);
        assert_eq!(
            code.line_numbers,
            vec![LineNumberEntry {
                start_pc: 0,
                line_number: 42,
            }]
        );
        assert_eq!(code.instructions.len(), 1);
        assert_eq!(code.instructions[0].offset, 0);
        assert_eq!(code.instructions[0].opcode, 0xb0);
        assert_eq!(code.instructions[0].mnemonic, "areturn");
        assert!(code.instructions[0].operands.is_empty());
    }

    #[test]
    fn decodes_bytecode_instruction_operands() {
        let constant_pool = empty_constant_pool();
        let instructions = decode_bytecode(
            &[
                0x10, 0x7f, // bipush 127
                0x84, 0x02, 0xfe, // iinc 2 by -2
                0xa7, 0xff, 0xfb, // goto 0
                0xc4, 0x84, 0x01, 0x00, 0xff, 0xff, // wide iinc 256 by -1
            ],
            &constant_pool,
        )
        .unwrap();

        assert_eq!(instructions.len(), 4);
        assert_eq!(instructions[0].mnemonic, "bipush");
        assert_eq!(instructions[0].operands, vec![operand("value", "i1", 127)]);
        assert_eq!(instructions[1].mnemonic, "iinc");
        assert_eq!(
            instructions[1].operands,
            vec![operand("index", "local", 2), operand("const", "i1", -2)]
        );
        assert_eq!(instructions[2].mnemonic, "goto");
        assert_eq!(
            instructions[2].operands,
            vec![operand("target", "branch", 0)]
        );
        assert_eq!(instructions[3].mnemonic, "wide");
        assert_eq!(
            instructions[3].operands,
            vec![
                operand("modified_opcode", "opcode", 0x84),
                operand("index", "local", 256),
                operand("const", "i2", -1),
            ]
        );
    }

    #[test]
    fn decodes_tableswitch_alignment_and_targets() {
        let constant_pool = empty_constant_pool();
        let instructions = decode_bytecode(
            &[
                0xaa, // tableswitch at offset 0
                0x00, 0x00, 0x00, // padding
                0x00, 0x00, 0x00, 0x0c, // default
                0x00, 0x00, 0x00, 0x01, // low
                0x00, 0x00, 0x00, 0x02, // high
                0x00, 0x00, 0x00, 0x14, // case 1
                0x00, 0x00, 0x00, 0x18, // case 2
            ],
            &constant_pool,
        )
        .unwrap();

        assert_eq!(instructions.len(), 1);
        assert_eq!(instructions[0].mnemonic, "tableswitch");
        assert_eq!(
            instructions[0].operands,
            vec![
                operand("padding", "u1_count", 3),
                operand("default_target", "branch", 12),
                operand("low", "i4", 1),
                operand("high", "i4", 2),
                operand("case_target", "branch", 20),
                operand("case_target", "branch", 24),
            ]
        );
    }

    #[test]
    fn decodes_lookupswitch_matches_and_targets() {
        let constant_pool = empty_constant_pool();
        let instructions = decode_bytecode(
            &[
                0xab, // lookupswitch at offset 0
                0x00, 0x00, 0x00, // padding
                0x00, 0x00, 0x00, 0x0c, // default
                0x00, 0x00, 0x00, 0x02, // npairs
                0x00, 0x00, 0x00, 0x07, // match 7
                0x00, 0x00, 0x00, 0x14, // target
                0x00, 0x00, 0x03, 0xe8, // match 1000
                0x00, 0x00, 0x00, 0x18, // target
            ],
            &constant_pool,
        )
        .unwrap();

        assert_eq!(instructions.len(), 1);
        assert_eq!(instructions[0].mnemonic, "lookupswitch");
        assert_eq!(
            instructions[0].operands,
            vec![
                operand("padding", "u1_count", 3),
                operand("default_target", "branch", 12),
                operand("pairs", "u4_count", 2),
                operand("match", "i4", 7),
                operand("target", "branch", 20),
                operand("match", "i4", 1000),
                operand("target", "branch", 24),
            ]
        );
    }

    #[test]
    fn resolves_constant_pool_operands() {
        let constant_pool = field_reference_constant_pool();
        let instructions = decode_bytecode(&[0xb4, 0x00, 0x06], &constant_pool).unwrap();

        assert_eq!(instructions.len(), 1);
        assert_eq!(instructions[0].mnemonic, "getfield");
        let operand = instructions[0]
            .operands
            .first()
            .expect("constant pool operand");
        assert_eq!(operand.kind, "constant_pool");
        assert_eq!(operand.value, 6);
        let resolved = operand.resolved.as_ref().expect("resolved operand");
        assert_eq!(resolved.tag, "Fieldref");
        assert_eq!(
            resolved.class_reference,
            Some(ClassReference {
                internal_name: "pkg/Foo".to_string(),
                fully_qualified_name: "pkg.Foo".to_string(),
            })
        );
        assert_eq!(resolved.name.as_deref(), Some("name"));
        assert_eq!(resolved.descriptor.as_deref(), Some("Ljava/lang/String;"));
        assert_eq!(resolved.field_type.as_deref(), Some("java.lang.String"));
    }

    #[test]
    fn builds_stack_simulated_body_ir() {
        let constant_pool = empty_constant_pool();
        let instructions = decode_bytecode(&[0x1a, 0x04, 0x60, 0xac], &constant_pool).unwrap();
        let local_variables = vec![LocalVariableEntry {
            start_pc: 0,
            length: 4,
            name: "x".to_string(),
            descriptor: "I".to_string(),
            type_name: Some("int".to_string()),
            signature: None,
            index: 0,
        }];
        let ir = build_method_body_ir(&instructions, &local_variables, &[]);

        assert_eq!(
            ir.iter().map(|entry| entry.operation).collect::<Vec<_>>(),
            vec!["load", "binary", "return"]
        );
        assert_eq!(ir[0].result.as_deref(), Some("x"));
        assert_eq!(ir[1].code, "(x + 1)");
        assert_eq!(ir[2].code, "return (x + 1)");
    }

    #[test]
    fn records_bitwise_and_shift_binary_ir() {
        let constant_pool = empty_constant_pool();
        let instructions = decode_bytecode(
            &[
                0x1a, 0x04, 0x78, // x << 1
                0x1b, 0x06, 0x7e, // y & 3
                0x82, // xor
                0x1a, 0x05, 0x7a, // x >> 2
                0x80, // or
                0x1b, 0x04, 0x7c, // y >>> 1
                0x80, // or
                0xac, // ireturn
            ],
            &constant_pool,
        )
        .expect("decode bytecode");
        let local_variables = vec![
            LocalVariableEntry {
                start_pc: 0,
                length: 16,
                name: "x".to_string(),
                descriptor: "I".to_string(),
                type_name: Some("int".to_string()),
                signature: None,
                index: 0,
            },
            LocalVariableEntry {
                start_pc: 0,
                length: 16,
                name: "y".to_string(),
                descriptor: "I".to_string(),
                type_name: Some("int".to_string()),
                signature: None,
                index: 1,
            },
        ];
        let ir = build_method_body_ir(&instructions, &local_variables, &[]);
        let binary_codes = ir
            .iter()
            .filter(|entry| entry.operation == "binary")
            .map(|entry| entry.code.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            binary_codes,
            vec![
                "(x << 1)",
                "(y & 3)",
                "((x << 1) ^ (y & 3))",
                "(x >> 2)",
                "(((x << 1) ^ (y & 3)) | (x >> 2))",
                "(y >>> 1)",
                "((((x << 1) ^ (y & 3)) | (x >> 2)) | (y >>> 1))"
            ]
        );
        assert_eq!(
            ir.last().map(|entry| entry.code.as_str()),
            Some("return ((((x << 1) ^ (y & 3)) | (x >> 2)) | (y >>> 1))")
        );
        assert!(!ir.iter().any(|entry| entry.operation == "unsupported"));
    }

    #[test]
    fn records_primitive_conversion_cast_target_type() {
        let constant_pool = empty_constant_pool();
        let instructions = decode_bytecode(&[0x1a, 0x85, 0xad], &constant_pool).unwrap();
        let local_variables = vec![LocalVariableEntry {
            start_pc: 0,
            length: 3,
            name: "x".to_string(),
            descriptor: "I".to_string(),
            type_name: Some("int".to_string()),
            signature: None,
            index: 0,
        }];
        let ir = build_method_body_ir(&instructions, &local_variables, &[]);
        let cast = ir
            .iter()
            .find(|entry| entry.operation == "cast")
            .expect("cast entry");

        assert_eq!(cast.code, "(long) x");
        assert_eq!(cast.result.as_deref(), Some("(long) x"));
        assert_eq!(cast.target.as_deref(), Some("long"));
        assert_eq!(cast.arguments, vec!["x"]);
        assert_eq!(
            ir.last().map(|entry| entry.code.as_str()),
            Some("return (long) x")
        );
    }

    #[test]
    fn records_array_length_result_type() {
        let constant_pool = empty_constant_pool();
        let instructions = decode_bytecode(&[0x2a, 0xbe, 0xac], &constant_pool).unwrap();
        let local_variables = vec![LocalVariableEntry {
            start_pc: 0,
            length: 3,
            name: "values".to_string(),
            descriptor: "[I".to_string(),
            type_name: Some("int[]".to_string()),
            signature: None,
            index: 0,
        }];
        let ir = build_method_body_ir(&instructions, &local_variables, &[]);
        let length = ir
            .iter()
            .find(|entry| entry.operation == "array_length")
            .expect("array length entry");

        assert_eq!(length.code, "values.length");
        assert_eq!(length.result.as_deref(), Some("values.length"));
        assert_eq!(length.target.as_deref(), Some("int"));
        assert_eq!(length.arguments, vec!["values"]);
        assert_eq!(
            ir.last().map(|entry| entry.code.as_str()),
            Some("return values.length")
        );
    }

    #[test]
    fn records_class_literal_constant_types() {
        let object_constant_pool = class_constant_pool("pkg/Foo");
        let object_instructions =
            decode_bytecode(&[0x12, 0x02, 0xb0], &object_constant_pool).unwrap();
        let object_ir = build_method_body_ir(&object_instructions, &[], &[]);
        let object_constant = object_ir
            .iter()
            .find(|entry| entry.operation == "constant")
            .expect("object class constant entry");

        assert_eq!(object_constant.code, "pkg.Foo.class");
        assert_eq!(object_constant.result.as_deref(), Some("pkg.Foo.class"));
        assert_eq!(object_constant.target.as_deref(), Some("java.lang.Class"));
        assert_eq!(
            object_ir.last().map(|entry| entry.code.as_str()),
            Some("return pkg.Foo.class")
        );

        let primitive_constant_pool = class_constant_pool("I");
        let primitive_instructions =
            decode_bytecode(&[0x12, 0x02, 0xb0], &primitive_constant_pool).unwrap();
        let primitive_ir = build_method_body_ir(&primitive_instructions, &[], &[]);
        let primitive_constant = primitive_ir
            .iter()
            .find(|entry| entry.operation == "constant")
            .expect("primitive class constant entry");

        assert_eq!(primitive_constant.code, "int.class");
        assert_eq!(primitive_constant.result.as_deref(), Some("int.class"));
        assert_eq!(
            primitive_constant.target.as_deref(),
            Some("java.lang.Class")
        );
        assert_eq!(
            primitive_ir.last().map(|entry| entry.code.as_str()),
            Some("return int.class")
        );

        let primitive_type_field_pool = primitive_type_field_constant_pool();
        let primitive_type_field_instructions =
            decode_bytecode(&[0xb2, 0x00, 0x06, 0xb0], &primitive_type_field_pool).unwrap();
        let primitive_type_field_ir =
            build_method_body_ir(&primitive_type_field_instructions, &[], &[]);
        let primitive_type_field_constant = primitive_type_field_ir
            .iter()
            .find(|entry| entry.operation == "constant")
            .expect("primitive TYPE field class constant entry");

        assert_eq!(primitive_type_field_constant.code, "int.class");
        assert_eq!(
            primitive_type_field_constant.target.as_deref(),
            Some("java.lang.Class")
        );
        assert_eq!(
            primitive_type_field_ir
                .last()
                .map(|entry| entry.code.as_str()),
            Some("return int.class")
        );
    }

    #[test]
    fn maps_all_primitive_conversion_result_types() {
        let mappings = [
            (0x85, "long"),
            (0x86, "float"),
            (0x87, "double"),
            (0x88, "int"),
            (0x89, "float"),
            (0x8a, "double"),
            (0x8b, "int"),
            (0x8c, "long"),
            (0x8d, "double"),
            (0x8e, "int"),
            (0x8f, "long"),
            (0x90, "float"),
            (0x91, "byte"),
            (0x92, "char"),
            (0x93, "short"),
        ];

        for (opcode, expected_type) in mappings {
            assert_eq!(
                primitive_conversion_result_type(opcode),
                Some(expected_type)
            );
        }
    }

    #[test]
    fn records_body_ir_branch_targets() {
        let constant_pool = empty_constant_pool();
        let instructions = decode_bytecode(
            &[0x1a, 0x9e, 0x00, 0x05, 0x04, 0xac, 0x02, 0xac],
            &constant_pool,
        )
        .unwrap();
        let local_variables = vec![LocalVariableEntry {
            start_pc: 0,
            length: 8,
            name: "x".to_string(),
            descriptor: "I".to_string(),
            type_name: Some("int".to_string()),
            signature: None,
            index: 0,
        }];
        let ir = build_method_body_ir(&instructions, &local_variables, &[]);
        let branch = ir
            .iter()
            .find(|entry| entry.operation == "branch")
            .expect("branch entry");

        assert_eq!(branch.code, "ifle(x) -> 6");
        assert_eq!(branch.targets, vec![6]);
    }

    #[test]
    fn records_legacy_subroutine_control_flow() {
        let constant_pool = empty_constant_pool();
        let instructions = decode_bytecode(
            &[
                0xa8, 0x00, 0x05, // jsr 5
                0x04, // iconst_1
                0xac, // ireturn
                0x4c, // astore_1
                0xa9, 0x01, // ret 1
            ],
            &constant_pool,
        )
        .expect("decode bytecode");
        let ir = build_method_body_ir(&instructions, &[], &[]);

        assert!(!ir.iter().any(|entry| entry.operation == "unsupported"));
        let jsr = ir
            .iter()
            .find(|entry| entry.operation == "jsr")
            .expect("jsr entry");
        assert_eq!(jsr.code, "jsr 5");
        assert_eq!(jsr.targets, vec![5]);
        assert_eq!(jsr.arguments, vec!["@retaddr3"]);
        let ret_assignment = ir
            .iter()
            .find(|entry| entry.code == "l1 = @retaddr3")
            .expect("return address assignment");
        assert_eq!(ret_assignment.result.as_deref(), Some("l1"));
        let ret = ir
            .iter()
            .find(|entry| entry.operation == "ret")
            .expect("ret entry");
        assert_eq!(ret.code, "ret l1");
        assert_eq!(ret.targets, vec![3]);
        assert_eq!(ret.arguments, vec!["l1"]);
    }

    #[test]
    fn uses_wide_local_index_for_load_store_and_ret() {
        let constant_pool = empty_constant_pool();
        let instructions = decode_bytecode(
            &[
                0xa8, 0x00, 0x05, // jsr 5
                0x04, // iconst_1
                0xac, // ireturn
                0xc4, 0x3a, 0x01, 0x00, // wide astore 256
                0xc4, 0x19, 0x01, 0x00, // wide aload 256
                0x57, // pop
                0xc4, 0xa9, 0x01, 0x00, // wide ret 256
            ],
            &constant_pool,
        )
        .expect("decode bytecode");
        let ir = build_method_body_ir(&instructions, &[], &[]);

        assert!(!ir.iter().any(|entry| entry.operation == "unsupported"));
        assert!(ir.iter().any(|entry| entry.code == "l256 = @retaddr3"));
        assert!(ir
            .iter()
            .any(|entry| entry.operation == "load" && entry.result.as_deref() == Some("l256")));
        let ret = ir
            .iter()
            .find(|entry| entry.operation == "ret")
            .expect("wide ret entry");
        assert_eq!(ret.code, "ret l256");
        assert_eq!(ret.targets, vec![3]);
    }

    #[test]
    fn records_iinc_with_binary_ir_and_preserves_post_increment_value() {
        let constant_pool = empty_constant_pool();
        let instructions = decode_bytecode(&[0x1b, 0x84, 0x01, 0x01, 0xac], &constant_pool)
            .expect("decode bytecode");
        let local_variables = vec![LocalVariableEntry {
            start_pc: 0,
            length: 5,
            name: "x".to_string(),
            descriptor: "I".to_string(),
            type_name: Some("int".to_string()),
            signature: None,
            index: 1,
        }];
        let ir = build_method_body_ir(&instructions, &local_variables, &[]);

        assert_eq!(
            ir.iter().map(|entry| entry.operation).collect::<Vec<_>>(),
            vec!["load", "assignment", "binary", "assignment", "return"]
        );
        assert_eq!(ir[1].code, "$stack1 = x");
        assert_eq!(ir[1].target.as_deref(), Some("int"));
        assert_eq!(ir[2].code, "(x + 1)");
        assert_eq!(ir[3].code, "x = (x + 1)");
        assert_eq!(ir[4].code, "return $stack1");
    }

    #[test]
    fn records_field_post_increment_stack_permutation() {
        let constant_pool = int_field_reference_constant_pool();
        let instructions = decode_bytecode(
            &[
                0x2b, 0x59, 0xb4, 0x00, 0x06, 0x5a, 0x04, 0x60, 0xb5, 0x00, 0x06, 0xac,
            ],
            &constant_pool,
        )
        .expect("decode bytecode");
        let local_variables = vec![LocalVariableEntry {
            start_pc: 0,
            length: 12,
            name: "f".to_string(),
            descriptor: "Lpkg/Foo;".to_string(),
            type_name: Some("pkg.Foo".to_string()),
            signature: None,
            index: 1,
        }];
        let ir = build_method_body_ir(&instructions, &local_variables, &[]);

        assert_eq!(
            ir.iter().map(|entry| entry.operation).collect::<Vec<_>>(),
            vec![
                "load",
                "field_load",
                "assignment",
                "binary",
                "field_store",
                "return"
            ]
        );
        assert_eq!(ir[1].code, "f.field");
        assert_eq!(ir[2].code, "$stack1 = f.field");
        assert_eq!(ir[3].code, "($stack1 + 1)");
        assert_eq!(ir[4].code, "f.field = ($stack1 + 1)");
        assert_eq!(ir[5].code, "return $stack1");
        assert!(!ir.iter().any(|entry| entry.operation == "unsupported"));
    }

    #[test]
    fn records_array_post_increment_stack_permutation() {
        let constant_pool = empty_constant_pool();
        let instructions = decode_bytecode(
            &[0x2b, 0x1c, 0x5c, 0x2e, 0x5b, 0x04, 0x60, 0x4f, 0xac],
            &constant_pool,
        )
        .expect("decode bytecode");
        let local_variables = vec![
            LocalVariableEntry {
                start_pc: 0,
                length: 9,
                name: "values".to_string(),
                descriptor: "[I".to_string(),
                type_name: Some("int[]".to_string()),
                signature: None,
                index: 1,
            },
            LocalVariableEntry {
                start_pc: 0,
                length: 9,
                name: "i".to_string(),
                descriptor: "I".to_string(),
                type_name: Some("int".to_string()),
                signature: None,
                index: 2,
            },
        ];
        let ir = build_method_body_ir(&instructions, &local_variables, &[]);

        assert_eq!(
            ir.iter().map(|entry| entry.operation).collect::<Vec<_>>(),
            vec![
                "load",
                "load",
                "array_load",
                "assignment",
                "binary",
                "array_store",
                "return"
            ]
        );
        assert_eq!(ir[2].code, "values[i]");
        assert_eq!(ir[2].target.as_deref(), Some("int"));
        assert_eq!(ir[3].code, "$stack1 = values[i]");
        assert_eq!(ir[3].target.as_deref(), Some("int"));
        assert_eq!(ir[4].code, "($stack1 + 1)");
        assert_eq!(ir[4].target.as_deref(), Some("int"));
        assert_eq!(ir[5].code, "values[i] = ($stack1 + 1)");
        assert_eq!(ir[5].target.as_deref(), Some("int"));
        assert_eq!(ir[6].code, "return $stack1");
        assert!(!ir.iter().any(|entry| entry.operation == "unsupported"));
    }

    #[test]
    fn records_body_ir_call_metadata() {
        let constant_pool = method_reference_constant_pool();
        let instructions = decode_bytecode(&[0x04, 0x05, 0xb8, 0x00, 0x06, 0xac], &constant_pool)
            .expect("decode bytecode");
        let ir = build_method_body_ir(&instructions, &[], &[]);
        let call = ir
            .iter()
            .find(|entry| entry.operation == "call")
            .expect("call entry");

        assert_eq!(call.code, "pkg.Foo.add(1, 2)");
        assert_eq!(call.target.as_deref(), Some("pkg.Foo.add"));
        assert_eq!(
            call.method_full_name.as_deref(),
            Some("pkg.Foo.add:int(int,int)")
        );
        assert_eq!(call.signature.as_deref(), Some("int(int,int)"));
        assert_eq!(call.dispatch_type, Some("STATIC_DISPATCH"));
        assert_eq!(call.receiver, None);
        assert_eq!(call.arguments, vec!["1", "2"]);
    }

    #[test]
    fn records_invokedynamic_bootstrap_arguments() {
        let constant_pool = invokedynamic_constant_pool();
        let bootstrap_methods = parse_bootstrap_methods(
            &[
                0x00, 0x01, // bootstrap_methods_count
                0x00, 0x0f, // bootstrap_method_ref
                0x00, 0x03, // bootstrap_argument_count
                0x00, 0x06, // String "A\u{1}"
                0x00, 0x08, // MethodType ()V
                0x00, 0x0f, // MethodHandle Foo.lambda$lambda$0
            ],
            &constant_pool,
        )
        .expect("parse bootstrap methods");
        let instructions =
            decode_bytecode(&[0x2b, 0xba, 0x00, 0x04, 0x00, 0x00, 0xb0], &constant_pool)
                .expect("decode bytecode");
        let local_variables = vec![LocalVariableEntry {
            start_pc: 0,
            length: 7,
            name: "x".to_string(),
            descriptor: "Ljava/lang/String;".to_string(),
            type_name: Some("java.lang.String".to_string()),
            signature: None,
            index: 1,
        }];
        let code = MethodCodeEntry {
            max_stack: 1,
            max_locals: 2,
            bytecode_length: 7,
            body_ir: build_method_body_ir(&instructions, &local_variables, &[]),
            instructions,
            exception_table: Vec::new(),
            line_numbers: Vec::new(),
            local_variables,
        };
        let mut methods = vec![MethodEntry {
            name: "concat".to_string(),
            descriptor: "(Ljava/lang/String;)Ljava/lang/String;".to_string(),
            parameter_types: vec!["java.lang.String".to_string()],
            return_type: Some("java.lang.String".to_string()),
            access_flags: 0,
            access_flags_text: Vec::new(),
            signature: None,
            exceptions: Vec::new(),
            code: Some(code),
        }];

        enrich_methods_with_bootstrap_arguments(&mut methods, &bootstrap_methods);

        let call = methods[0]
            .code
            .as_ref()
            .expect("code")
            .body_ir
            .iter()
            .find(|entry| entry.operation == "call")
            .expect("call");
        assert_eq!(call.code, "makeConcatWithConstants(x)");
        assert_eq!(call.arguments, vec!["x"]);
        assert_eq!(call.bootstrap_arguments.len(), 3);
        assert!(call
            .bootstrap_arguments
            .iter()
            .any(|argument| argument.contains('A')));
        assert!(call.bootstrap_arguments.contains(&"void()".to_string()));
        assert!(call
            .bootstrap_arguments
            .iter()
            .any(|argument| argument.contains("lambda$lambda$0")));
    }

    #[test]
    fn records_constructor_alloc_with_stack_temp_receiver() {
        let constant_pool = constructor_reference_constant_pool();
        let instructions = decode_bytecode(
            &[0xbb, 0x00, 0x02, 0x59, 0x04, 0xb7, 0x00, 0x06, 0xb0],
            &constant_pool,
        )
        .expect("decode bytecode");
        let ir = build_method_body_ir(&instructions, &[], &[]);

        assert_eq!(
            ir.iter().map(|entry| entry.operation).collect::<Vec<_>>(),
            vec!["alloc", "call", "return"]
        );
        let alloc = &ir[0];
        assert_eq!(alloc.code, "new pkg.Foo");
        assert_eq!(alloc.result.as_deref(), Some("$stack1"));
        assert_eq!(alloc.target.as_deref(), Some("pkg.Foo"));

        let init = &ir[1];
        assert_eq!(init.receiver.as_deref(), Some("$stack1"));
        assert_eq!(init.arguments, vec!["$stack1", "1"]);
        assert_eq!(
            init.method_full_name.as_deref(),
            Some("pkg.Foo.<init>:void(int)")
        );
        assert_eq!(init.signature.as_deref(), Some("void(int)"));
        assert_eq!(init.dispatch_type, Some("STATIC_DISPATCH"));
        assert_eq!(ir[2].code, "return $stack1");
    }

    #[test]
    fn records_primitive_array_allocation_types() {
        let constant_pool = empty_constant_pool();
        let instructions =
            decode_bytecode(&[0x05, 0xbc, 0x0a, 0x4c], &constant_pool).expect("decode bytecode");
        let local_variables = vec![LocalVariableEntry {
            start_pc: 0,
            length: 4,
            name: "values".to_string(),
            descriptor: "[I".to_string(),
            type_name: Some("int[]".to_string()),
            signature: None,
            index: 1,
        }];
        let ir = build_method_body_ir(&instructions, &local_variables, &[]);

        assert_eq!(
            ir.iter().map(|entry| entry.operation).collect::<Vec<_>>(),
            vec!["alloc_array", "assignment"]
        );
        assert_eq!(ir[0].code, "new int[2]");
        assert_eq!(ir[0].result.as_deref(), Some("new int[2]"));
        assert_eq!(ir[0].target.as_deref(), Some("int[]"));
        assert_eq!(ir[0].arguments, vec!["2"]);
        assert_eq!(ir[1].code, "values = new int[2]");
    }

    #[test]
    fn materializes_duplicated_array_allocations_as_stack_temps() {
        let constant_pool = empty_constant_pool();
        let instructions = decode_bytecode(
            &[0x06, 0xbc, 0x0a, 0x59, 0x03, 0x04, 0x4f, 0x4c],
            &constant_pool,
        )
        .expect("decode bytecode");
        let local_variables = vec![LocalVariableEntry {
            start_pc: 0,
            length: 8,
            name: "values".to_string(),
            descriptor: "[I".to_string(),
            type_name: Some("int[]".to_string()),
            signature: None,
            index: 1,
        }];
        let ir = build_method_body_ir(&instructions, &local_variables, &[]);

        assert_eq!(
            ir.iter().map(|entry| entry.operation).collect::<Vec<_>>(),
            vec!["alloc_array", "array_store", "assignment"]
        );
        assert_eq!(ir[0].code, "new int[3]");
        assert_eq!(ir[0].result.as_deref(), Some("$stack1"));
        assert_eq!(ir[0].target.as_deref(), Some("int[]"));
        assert_eq!(ir[1].code, "$stack1[0] = 1");
        assert_eq!(ir[2].code, "values = $stack1");
    }

    #[test]
    fn records_object_array_allocation_types() {
        let constant_pool = object_array_constant_pool();
        let instructions = decode_bytecode(&[0x05, 0xbd, 0x00, 0x02, 0x4c], &constant_pool)
            .expect("decode bytecode");
        let local_variables = vec![LocalVariableEntry {
            start_pc: 0,
            length: 5,
            name: "names".to_string(),
            descriptor: "[Ljava/lang/String;".to_string(),
            type_name: Some("java.lang.String[]".to_string()),
            signature: None,
            index: 1,
        }];
        let ir = build_method_body_ir(&instructions, &local_variables, &[]);

        assert_eq!(
            ir.iter().map(|entry| entry.operation).collect::<Vec<_>>(),
            vec!["alloc_array", "assignment"]
        );
        assert_eq!(ir[0].code, "new java.lang.String[2]");
        assert_eq!(ir[0].target.as_deref(), Some("java.lang.String[]"));
        assert_eq!(ir[0].arguments, vec!["2"]);
        assert_eq!(ir[1].code, "names = new java.lang.String[2]");
    }

    #[test]
    fn records_multidimensional_array_allocation_types() {
        let constant_pool = int_matrix_constant_pool();
        let instructions =
            decode_bytecode(&[0x05, 0x06, 0xc5, 0x00, 0x02, 0x02, 0xb0], &constant_pool)
                .expect("decode bytecode");
        let ir = build_method_body_ir(&instructions, &[], &[]);

        assert_eq!(
            ir.iter().map(|entry| entry.operation).collect::<Vec<_>>(),
            vec!["alloc_array", "return"]
        );
        assert_eq!(ir[0].code, "new int[2][3]");
        assert_eq!(ir[0].result.as_deref(), Some("new int[2][3]"));
        assert_eq!(ir[0].target.as_deref(), Some("int[][]"));
        assert_eq!(ir[0].arguments, vec!["2", "3"]);
        assert_eq!(ir[1].code, "return new int[2][3]");
    }

    #[test]
    fn records_monitor_operations_with_jimple_style_temp_locals() {
        let constant_pool = empty_constant_pool();
        let instructions =
            decode_bytecode(&[0x2a, 0x59, 0x4c, 0xc2, 0x2b, 0xc3, 0xb1], &constant_pool)
                .expect("decode bytecode");
        let local_variables = vec![LocalVariableEntry {
            start_pc: 0,
            length: 7,
            name: "this".to_string(),
            descriptor: "Lpkg/Foo;".to_string(),
            type_name: Some("pkg.Foo".to_string()),
            signature: None,
            index: 0,
        }];
        let ir = build_method_body_ir(&instructions, &local_variables, &[]);

        assert_eq!(
            ir.iter().map(|entry| entry.operation).collect::<Vec<_>>(),
            vec![
                "load",
                "assignment",
                "monitorenter",
                "load",
                "monitorexit",
                "return"
            ]
        );
        assert_eq!(ir[1].code, "l1 = this");
        assert_eq!(ir[2].code, "monitorenter(this)");
        assert_eq!(ir[2].arguments, vec!["this"]);
        assert_eq!(ir[4].code, "monitorexit(l1)");
        assert_eq!(ir[4].arguments, vec!["l1"]);
    }

    #[test]
    fn seeds_exception_handler_stack_with_caught_exception_ref() {
        let constant_pool = empty_constant_pool();
        let instructions =
            decode_bytecode(&[0x03, 0xac, 0x4c, 0xb1], &constant_pool).expect("decode bytecode");
        let exception_handlers = vec![ExceptionHandlerEntry {
            start_pc: 0,
            end_pc: 2,
            handler_pc: 2,
            catch_type: None,
        }];
        let ir = build_method_body_ir(&instructions, &[], &exception_handlers);

        let handler_assignment = ir
            .iter()
            .find(|entry| entry.offset == 2 && entry.operation == "assignment")
            .expect("handler assignment");
        assert_eq!(handler_assignment.code, "l1 = @caughtexception");
        assert_eq!(handler_assignment.result.as_deref(), Some("l1"));
        assert_eq!(
            handler_assignment.target.as_deref(),
            Some("java.lang.Throwable")
        );
        assert_eq!(handler_assignment.arguments, vec!["@caughtexception"]);
    }

    #[test]
    fn records_local_variable_type_table_signatures() {
        let constant_pool = local_variable_type_table_constant_pool();
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0u16.to_be_bytes()); // max_stack
        bytes.extend_from_slice(&1u16.to_be_bytes()); // max_locals
        bytes.extend_from_slice(&1u32.to_be_bytes()); // code_length
        bytes.push(0xb1); // return
        bytes.extend_from_slice(&0u16.to_be_bytes()); // exception_table_length
        bytes.extend_from_slice(&2u16.to_be_bytes()); // attributes_count
        bytes.extend_from_slice(&1u16.to_be_bytes()); // LocalVariableTable
        bytes.extend_from_slice(&12u32.to_be_bytes());
        bytes.extend_from_slice(&1u16.to_be_bytes());
        bytes.extend_from_slice(&0u16.to_be_bytes()); // start_pc
        bytes.extend_from_slice(&1u16.to_be_bytes()); // length
        bytes.extend_from_slice(&3u16.to_be_bytes()); // name_index labels
        bytes.extend_from_slice(&4u16.to_be_bytes()); // descriptor_index List
        bytes.extend_from_slice(&0u16.to_be_bytes()); // index
        bytes.extend_from_slice(&2u16.to_be_bytes()); // LocalVariableTypeTable
        bytes.extend_from_slice(&12u32.to_be_bytes());
        bytes.extend_from_slice(&1u16.to_be_bytes());
        bytes.extend_from_slice(&0u16.to_be_bytes()); // start_pc
        bytes.extend_from_slice(&1u16.to_be_bytes()); // length
        bytes.extend_from_slice(&3u16.to_be_bytes()); // name_index labels
        bytes.extend_from_slice(&5u16.to_be_bytes()); // signature_index List<String>
        bytes.extend_from_slice(&0u16.to_be_bytes()); // index

        let code = parse_code_attribute(&bytes, &constant_pool).expect("parse code attribute");
        assert_eq!(code.local_variables.len(), 1);
        let local = &code.local_variables[0];
        assert_eq!(local.name, "labels");
        assert_eq!(local.descriptor, "Ljava/util/List;");
        assert_eq!(local.type_name.as_deref(), Some("java.util.List"));
        assert_eq!(
            local.signature.as_deref(),
            Some("Ljava/util/List<Ljava/lang/String;>;")
        );
    }

    fn empty_constant_pool() -> ConstantPool {
        ConstantPool {
            entries: vec![None],
        }
    }

    fn local_variable_type_table_constant_pool() -> ConstantPool {
        ConstantPool {
            entries: vec![
                None,
                Some(ConstantPoolEntry::Utf8("LocalVariableTable".to_string())),
                Some(ConstantPoolEntry::Utf8(
                    "LocalVariableTypeTable".to_string(),
                )),
                Some(ConstantPoolEntry::Utf8("labels".to_string())),
                Some(ConstantPoolEntry::Utf8("Ljava/util/List;".to_string())),
                Some(ConstantPoolEntry::Utf8(
                    "Ljava/util/List<Ljava/lang/String;>;".to_string(),
                )),
            ],
        }
    }

    fn field_reference_constant_pool() -> ConstantPool {
        ConstantPool {
            entries: vec![
                None,
                Some(ConstantPoolEntry::Utf8("pkg/Foo".to_string())),
                Some(ConstantPoolEntry::Class(1)),
                Some(ConstantPoolEntry::Utf8("name".to_string())),
                Some(ConstantPoolEntry::Utf8("Ljava/lang/String;".to_string())),
                Some(ConstantPoolEntry::NameAndType {
                    name_index: 3,
                    descriptor_index: 4,
                }),
                Some(ConstantPoolEntry::Fieldref {
                    class_index: 2,
                    name_and_type_index: 5,
                }),
            ],
        }
    }

    fn int_field_reference_constant_pool() -> ConstantPool {
        ConstantPool {
            entries: vec![
                None,
                Some(ConstantPoolEntry::Utf8("pkg/Foo".to_string())),
                Some(ConstantPoolEntry::Class(1)),
                Some(ConstantPoolEntry::Utf8("field".to_string())),
                Some(ConstantPoolEntry::Utf8("I".to_string())),
                Some(ConstantPoolEntry::NameAndType {
                    name_index: 3,
                    descriptor_index: 4,
                }),
                Some(ConstantPoolEntry::Fieldref {
                    class_index: 2,
                    name_and_type_index: 5,
                }),
            ],
        }
    }

    fn method_reference_constant_pool() -> ConstantPool {
        ConstantPool {
            entries: vec![
                None,
                Some(ConstantPoolEntry::Utf8("pkg/Foo".to_string())),
                Some(ConstantPoolEntry::Class(1)),
                Some(ConstantPoolEntry::Utf8("add".to_string())),
                Some(ConstantPoolEntry::Utf8("(II)I".to_string())),
                Some(ConstantPoolEntry::NameAndType {
                    name_index: 3,
                    descriptor_index: 4,
                }),
                Some(ConstantPoolEntry::Methodref {
                    class_index: 2,
                    name_and_type_index: 5,
                }),
            ],
        }
    }

    fn constructor_reference_constant_pool() -> ConstantPool {
        ConstantPool {
            entries: vec![
                None,
                Some(ConstantPoolEntry::Utf8("pkg/Foo".to_string())),
                Some(ConstantPoolEntry::Class(1)),
                Some(ConstantPoolEntry::Utf8("<init>".to_string())),
                Some(ConstantPoolEntry::Utf8("(I)V".to_string())),
                Some(ConstantPoolEntry::NameAndType {
                    name_index: 3,
                    descriptor_index: 4,
                }),
                Some(ConstantPoolEntry::Methodref {
                    class_index: 2,
                    name_and_type_index: 5,
                }),
            ],
        }
    }

    fn invokedynamic_constant_pool() -> ConstantPool {
        ConstantPool {
            entries: vec![
                None,
                Some(ConstantPoolEntry::Utf8(
                    "makeConcatWithConstants".to_string(),
                )),
                Some(ConstantPoolEntry::Utf8(
                    "(Ljava/lang/String;)Ljava/lang/String;".to_string(),
                )),
                Some(ConstantPoolEntry::NameAndType {
                    name_index: 1,
                    descriptor_index: 2,
                }),
                Some(ConstantPoolEntry::InvokeDynamic {
                    bootstrap_method_attr_index: 0,
                    name_and_type_index: 3,
                }),
                Some(ConstantPoolEntry::Utf8("A\u{1}".to_string())),
                Some(ConstantPoolEntry::String(5)),
                Some(ConstantPoolEntry::Utf8("()V".to_string())),
                Some(ConstantPoolEntry::MethodType {
                    descriptor_index: 7,
                }),
                Some(ConstantPoolEntry::Utf8("pkg/Foo".to_string())),
                Some(ConstantPoolEntry::Class(9)),
                Some(ConstantPoolEntry::Utf8("lambda$lambda$0".to_string())),
                Some(ConstantPoolEntry::Utf8("(Ljava/lang/String;)V".to_string())),
                Some(ConstantPoolEntry::NameAndType {
                    name_index: 11,
                    descriptor_index: 12,
                }),
                Some(ConstantPoolEntry::Methodref {
                    class_index: 10,
                    name_and_type_index: 13,
                }),
                Some(ConstantPoolEntry::MethodHandle {
                    reference_kind: 6,
                    reference_index: 14,
                }),
            ],
        }
    }

    fn object_array_constant_pool() -> ConstantPool {
        ConstantPool {
            entries: vec![
                None,
                Some(ConstantPoolEntry::Utf8("java/lang/String".to_string())),
                Some(ConstantPoolEntry::Class(1)),
            ],
        }
    }

    fn int_matrix_constant_pool() -> ConstantPool {
        ConstantPool {
            entries: vec![
                None,
                Some(ConstantPoolEntry::Utf8("[[I".to_string())),
                Some(ConstantPoolEntry::Class(1)),
            ],
        }
    }

    fn class_constant_pool(internal_name: &str) -> ConstantPool {
        ConstantPool {
            entries: vec![
                None,
                Some(ConstantPoolEntry::Utf8(internal_name.to_string())),
                Some(ConstantPoolEntry::Class(1)),
            ],
        }
    }

    fn primitive_type_field_constant_pool() -> ConstantPool {
        ConstantPool {
            entries: vec![
                None,
                Some(ConstantPoolEntry::Utf8("java/lang/Integer".to_string())),
                Some(ConstantPoolEntry::Class(1)),
                Some(ConstantPoolEntry::Utf8("TYPE".to_string())),
                Some(ConstantPoolEntry::Utf8("Ljava/lang/Class;".to_string())),
                Some(ConstantPoolEntry::NameAndType {
                    name_index: 3,
                    descriptor_index: 4,
                }),
                Some(ConstantPoolEntry::Fieldref {
                    class_index: 2,
                    name_and_type_index: 5,
                }),
            ],
        }
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
        bytes.extend_from_slice(&0u16.to_be_bytes());
        bytes.extend_from_slice(&0u16.to_be_bytes());
        bytes.extend_from_slice(&0u16.to_be_bytes());
        bytes.extend_from_slice(&0u16.to_be_bytes());
        bytes
    }

    fn class_with_members() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0xCAFEBABEu32.to_be_bytes());
        bytes.extend_from_slice(&0u16.to_be_bytes());
        bytes.extend_from_slice(&52u16.to_be_bytes());
        bytes.extend_from_slice(&22u16.to_be_bytes());
        push_utf8(&mut bytes, "pkg/Foo"); // #1
        push_class(&mut bytes, 1); // #2
        push_utf8(&mut bytes, "java/lang/Object"); // #3
        push_class(&mut bytes, 3); // #4
        push_utf8(&mut bytes, "value"); // #5
        push_utf8(&mut bytes, "I"); // #6
        push_utf8(&mut bytes, "answer"); // #7
        push_utf8(&mut bytes, "(ILjava/lang/String;[I)Ljava/lang/String;"); // #8
        push_utf8(&mut bytes, "SourceFile"); // #9
        push_utf8(&mut bytes, "Foo.java"); // #10
        push_utf8(&mut bytes, "names"); // #11
        push_utf8(&mut bytes, "[Ljava/lang/String;"); // #12
        push_utf8(&mut bytes, "java/io/Serializable"); // #13
        push_class(&mut bytes, 13); // #14
        push_utf8(&mut bytes, "Code"); // #15
        push_utf8(&mut bytes, "LineNumberTable"); // #16
        push_utf8(&mut bytes, "ConstantValue"); // #17
        push_integer(&mut bytes, 7); // #18
        push_utf8(&mut bytes, "Exceptions"); // #19
        push_utf8(&mut bytes, "java/lang/Exception"); // #20
        push_class(&mut bytes, 20); // #21

        bytes.extend_from_slice(&0x0021u16.to_be_bytes());
        bytes.extend_from_slice(&2u16.to_be_bytes());
        bytes.extend_from_slice(&4u16.to_be_bytes());

        bytes.extend_from_slice(&1u16.to_be_bytes());
        bytes.extend_from_slice(&14u16.to_be_bytes());

        bytes.extend_from_slice(&2u16.to_be_bytes());
        push_member_with_constant_value(&mut bytes, 0x0019, 5, 6, 17, 18);
        push_member(&mut bytes, 0x0002, 11, 12);

        bytes.extend_from_slice(&1u16.to_be_bytes());
        push_method_with_code_and_exceptions(&mut bytes, 0x0001, 7, 8, 15, 16, 19, 21);

        bytes.extend_from_slice(&1u16.to_be_bytes());
        bytes.extend_from_slice(&9u16.to_be_bytes());
        bytes.extend_from_slice(&2u32.to_be_bytes());
        bytes.extend_from_slice(&10u16.to_be_bytes());
        bytes
    }

    fn push_utf8(bytes: &mut Vec<u8>, value: &str) {
        bytes.push(1);
        bytes.extend_from_slice(&(value.len() as u16).to_be_bytes());
        bytes.extend_from_slice(value.as_bytes());
    }

    fn push_class(bytes: &mut Vec<u8>, name_index: u16) {
        bytes.push(7);
        bytes.extend_from_slice(&name_index.to_be_bytes());
    }

    fn push_integer(bytes: &mut Vec<u8>, value: i32) {
        bytes.push(3);
        bytes.extend_from_slice(&value.to_be_bytes());
    }

    fn push_member(bytes: &mut Vec<u8>, access_flags: u16, name_index: u16, descriptor_index: u16) {
        bytes.extend_from_slice(&access_flags.to_be_bytes());
        bytes.extend_from_slice(&name_index.to_be_bytes());
        bytes.extend_from_slice(&descriptor_index.to_be_bytes());
        bytes.extend_from_slice(&0u16.to_be_bytes());
    }

    fn push_member_with_constant_value(
        bytes: &mut Vec<u8>,
        access_flags: u16,
        name_index: u16,
        descriptor_index: u16,
        constant_value_name_index: u16,
        constant_value_index: u16,
    ) {
        bytes.extend_from_slice(&access_flags.to_be_bytes());
        bytes.extend_from_slice(&name_index.to_be_bytes());
        bytes.extend_from_slice(&descriptor_index.to_be_bytes());
        bytes.extend_from_slice(&1u16.to_be_bytes());
        bytes.extend_from_slice(&constant_value_name_index.to_be_bytes());
        bytes.extend_from_slice(&2u32.to_be_bytes());
        bytes.extend_from_slice(&constant_value_index.to_be_bytes());
    }

    #[allow(clippy::too_many_arguments)]
    fn push_method_with_code_and_exceptions(
        bytes: &mut Vec<u8>,
        access_flags: u16,
        name_index: u16,
        descriptor_index: u16,
        code_name_index: u16,
        line_number_table_name_index: u16,
        exceptions_name_index: u16,
        exception_class_index: u16,
    ) {
        bytes.extend_from_slice(&access_flags.to_be_bytes());
        bytes.extend_from_slice(&name_index.to_be_bytes());
        bytes.extend_from_slice(&descriptor_index.to_be_bytes());
        bytes.extend_from_slice(&2u16.to_be_bytes());
        bytes.extend_from_slice(&code_name_index.to_be_bytes());
        bytes.extend_from_slice(&25u32.to_be_bytes());
        bytes.extend_from_slice(&1u16.to_be_bytes());
        bytes.extend_from_slice(&4u16.to_be_bytes());
        bytes.extend_from_slice(&1u32.to_be_bytes());
        bytes.push(0xb0);
        bytes.extend_from_slice(&0u16.to_be_bytes());
        bytes.extend_from_slice(&1u16.to_be_bytes());
        bytes.extend_from_slice(&line_number_table_name_index.to_be_bytes());
        bytes.extend_from_slice(&6u32.to_be_bytes());
        bytes.extend_from_slice(&1u16.to_be_bytes());
        bytes.extend_from_slice(&0u16.to_be_bytes());
        bytes.extend_from_slice(&42u16.to_be_bytes());
        bytes.extend_from_slice(&exceptions_name_index.to_be_bytes());
        bytes.extend_from_slice(&4u32.to_be_bytes());
        bytes.extend_from_slice(&1u16.to_be_bytes());
        bytes.extend_from_slice(&exception_class_index.to_be_bytes());
    }
}
