use anyhow::{Context, Result, bail};
use clap::Parser;
use ignore::WalkBuilder;
use regex::Regex;
use rustastgen_core::{parse_file_with_sysroot, write_json};
use std::path::{Path, PathBuf};

#[derive(Parser, Debug)]
#[command(name = "rust_ast_gen", disable_version_flag = true)]
struct Args {
    #[arg(long = "version", action = clap::ArgAction::SetTrue)]
    version: bool,

    #[arg(short = 'i', long = "input", value_name = "INPUT")]
    input: Option<PathBuf>,

    #[arg(short = 'o', long = "out", value_name = "OUT")]
    out: Option<PathBuf>,

    #[arg(long = "no-sysroot", action = clap::ArgAction::SetTrue)]
    no_sysroot: bool,

    #[arg(long = "exclude-regex", value_name = "REGEX")]
    exclude_regex: Option<String>,

    #[arg(long = "exclude-file", value_name = "PATH")]
    exclude_files: Vec<PathBuf>,
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

    let input = args.input.context("missing -i <input>")?;
    let out = args.out.context("missing -o <dir>")?;
    let exclude_regex = args
        .exclude_regex
        .as_deref()
        .map(compile_exclude_regex)
        .transpose()?;
    let exclude = ExcludeMatcher::new(&input, exclude_regex, &args.exclude_files);
    let files = collect_inputs(&input, &exclude)?;
    let root = input_root(&input);

    for file in files {
        let target = output_path(&input, &out, &file);
        match parse_file_with_sysroot(&root, &file, !args.no_sysroot)
            .and_then(|value| write_json(&target, &value))
        {
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
            "-i" => "--input".into(),
            "-o" => "--out".into(),
            "-version" => "--version".into(),
            _ => arg,
        })
        .collect()
}

fn collect_inputs(input: &Path, exclude: &ExcludeMatcher) -> Result<Vec<PathBuf>> {
    if input.is_file() {
        if is_rust_input(input) && !exclude.is_excluded(input) {
            return Ok(vec![input.to_path_buf()]);
        }
        bail!("input file is not a .rs file: {}", input.display());
    }

    let mut files = Vec::new();
    for entry in WalkBuilder::new(input).hidden(false).build() {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() && is_rust_input(path) && !exclude.is_excluded(path) {
            files.push(path.to_path_buf());
        }
    }
    files.sort();
    Ok(files)
}

fn is_rust_input(path: &Path) -> bool {
    path.extension().and_then(|x| x.to_str()) == Some("rs")
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
            .any(|excluded| path == excluded || path.starts_with(excluded))
    }
}

fn input_root(input: &Path) -> PathBuf {
    if is_directory_input(input) {
        input.to_path_buf()
    } else {
        input.parent().unwrap_or(input).to_path_buf()
    }
}

fn output_path(input: &Path, out: &Path, file: &Path) -> PathBuf {
    let relative = if is_directory_input(input) {
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

fn normalize_absolute_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    }
}

fn is_directory_input(input: &Path) -> bool {
    input.is_dir() || (!input.exists() && input.extension().is_none())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_version_alias() {
        let args = Args::parse_from(["rust_ast_gen", "--version"]);
        assert!(args.version);
    }

    #[test]
    fn preserves_relative_output_path() {
        let input = Path::new("/tmp/project");
        let file = Path::new("/tmp/project/src/main.rs");
        let out = Path::new("/tmp/out");
        assert_eq!(
            output_path(input, out, file),
            Path::new("/tmp/out/src/main.rs.json")
        );
    }
}
