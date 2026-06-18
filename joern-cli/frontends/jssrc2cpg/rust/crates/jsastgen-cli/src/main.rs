use anyhow::{bail, Context, Result};
use clap::Parser;
use ignore::WalkBuilder;
use jsastgen_core::{parse_file_with_source, write_json, write_type_map, TypeMapProject};
use regex::Regex;
use std::path::{Component, Path, PathBuf};

#[derive(Parser, Debug)]
#[command(name = "astgen", disable_version_flag = true)]
struct Args {
    #[arg(long = "version", action = clap::ArgAction::SetTrue)]
    version: bool,

    #[arg(short = 'o', long = "out", value_name = "OUT")]
    out: Option<PathBuf>,

    #[arg(short = 't', value_name = "TYPE", default_value = "ts")]
    language_type: String,

    #[arg(long = "no-tsTypes", action = clap::ArgAction::SetTrue)]
    no_ts_types: bool,

    #[arg(long = "exclude-regex", value_name = "REGEX")]
    exclude_regex: Option<String>,

    #[arg(long = "exclude-file", value_name = "PATH")]
    exclude_files: Vec<PathBuf>,

    #[arg(value_name = "INPUT")]
    input: Option<PathBuf>,
}

fn main() {
    if let Err(err) = run() {
        eprintln!("{err:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args = Args::parse_from(normalized_args());
    if args.version {
        println!("{}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    let out = args.out.context("missing -o <dir>")?;
    let input = args.input.unwrap_or(std::env::current_dir()?);
    if args.language_type != "ts" && args.language_type != "js" && args.language_type != "vue" {
        bail!("unsupported astgen type '{}'", args.language_type);
    }

    let exclude_regex = args
        .exclude_regex
        .as_deref()
        .map(compile_exclude_regex)
        .transpose()?;
    let exclude = ExcludeMatcher::new(&input, exclude_regex, &args.exclude_files);
    let files = collect_inputs(&input, &exclude, &args.language_type)?;
    let root = input_root(&input);
    let mut parsed_files = Vec::new();
    for file in files {
        match parse_file_with_source(&root, &file) {
            Ok((value, source)) => parsed_files.push((file, value, source)),
            Err(err) => println!("{} {}", file.display(), err),
        }
    }

    let type_project =
        (!args.no_ts_types).then(|| TypeMapProject::from_parsed_files(&parsed_files));
    for (file, value, source) in parsed_files {
        let target = output_path(&input, &out, &file);
        let write_result = (|| -> Result<()> {
            write_json(&target, &value)?;
            if let Some(project) = &type_project {
                let type_map = project.infer_type_map(&value, &source);
                write_type_map(&type_map_output_path(&target), &type_map)?;
            }
            Ok(())
        })();
        match write_result {
            Ok(()) => println!(
                "Converted AST for {} to {}",
                file.display(),
                target.display()
            ),
            Err(err) => println!("{} {}", file.display(), err),
        }
    }

    Ok(())
}

fn normalized_args() -> Vec<String> {
    std::env::args()
        .map(|arg| match arg.as_str() {
            "-o" => "--out".into(),
            "-out" => "--out".into(),
            "-version" => "--version".into(),
            _ => arg,
        })
        .collect()
}

fn collect_inputs(
    input: &Path,
    exclude: &ExcludeMatcher,
    language_type: &str,
) -> Result<Vec<PathBuf>> {
    if input.is_file() {
        if is_supported_input(input, language_type) && !exclude.is_excluded(input) {
            return Ok(vec![input.to_path_buf()]);
        }
        bail!(
            "input file is not a supported {language_type} source: {}",
            input.display()
        );
    }

    let mut files = Vec::new();
    for entry in WalkBuilder::new(input).hidden(true).build() {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() && is_supported_input(path, language_type) && !exclude.is_excluded(path) {
            files.push(path.to_path_buf());
        }
    }
    files.sort();
    Ok(files)
}

fn is_js_input(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|x| x.to_str()),
        Some("js" | "jsx" | "cjs" | "mjs" | "xsjs" | "xsjslib" | "ts" | "tsx")
    )
}

fn is_supported_input(path: &Path, language_type: &str) -> bool {
    if language_type == "vue" {
        path.extension().and_then(|x| x.to_str()) == Some("vue")
    } else {
        is_js_input(path)
    }
}

struct ExcludeMatcher {
    input_root: PathBuf,
    regex: Option<Regex>,
    paths: Vec<PathBuf>,
}

impl ExcludeMatcher {
    fn new(input: &Path, regex: Option<Regex>, exclude_files: &[PathBuf]) -> Self {
        let input_root = normalize_absolute_path(&input_root(input));
        let paths = exclude_files
            .iter()
            .map(|path| {
                if path.is_absolute() {
                    normalize_absolute_path(path)
                } else {
                    normalize_absolute_path(&input_root.join(path))
                }
            })
            .collect();
        Self {
            input_root,
            regex,
            paths,
        }
    }

    fn is_excluded(&self, path: &Path) -> bool {
        let normalized_path = normalize_absolute_path(path);
        self.is_excluded_by_regex(&normalized_path) || self.is_excluded_by_path(&normalized_path)
    }

    fn is_excluded_by_regex(&self, path: &Path) -> bool {
        self.regex.as_ref().is_some_and(|regex| {
            regex.is_match(&path.to_string_lossy())
                || path
                    .strip_prefix(&self.input_root)
                    .ok()
                    .is_some_and(|relative| regex.is_match(&relative.to_string_lossy()))
        })
    }

    fn is_excluded_by_path(&self, path: &Path) -> bool {
        self.paths
            .iter()
            .any(|excluded| path == excluded || (excluded.is_dir() && path.starts_with(excluded)))
    }
}

fn compile_exclude_regex(raw: &str) -> Result<Regex> {
    let pattern = normalize_java_quoted_regex(raw);
    Regex::new(&pattern).with_context(|| format!("invalid exclude regex '{raw}'"))
}

fn normalize_java_quoted_regex(raw: &str) -> String {
    let mut normalized = String::new();
    let mut rest = raw;
    while let Some(start) = rest.find(r"\Q") {
        normalized.push_str(&rest[..start]);
        let quoted = &rest[start + 2..];
        if let Some(end) = quoted.find(r"\E") {
            normalized.push_str(&regex::escape(&quoted[..end]));
            rest = &quoted[end + 2..];
        } else {
            normalized.push_str(&regex::escape(quoted));
            rest = "";
        }
    }
    normalized.push_str(rest);
    normalized
}

fn normalize_absolute_path(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };
    normalize_path_components(&absolute)
}

fn normalize_path_components(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
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

fn output_path(input: &Path, out: &Path, file: &Path) -> PathBuf {
    let relative = if input.is_dir() {
        file.strip_prefix(input).unwrap_or(file)
    } else {
        file.file_name().map(Path::new).unwrap_or(file)
    };
    let mut target = out.join(relative);
    let file_name = target
        .file_name()
        .and_then(|x| x.to_str())
        .map(|x| format!("{x}.json"))
        .unwrap_or_else(|| "out.json".into());
    target.set_file_name(file_name);
    target
}

fn type_map_output_path(json_path: &Path) -> PathBuf {
    let mut path = json_path.to_path_buf();
    path.set_extension("typemap");
    path
}
