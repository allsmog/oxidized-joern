use anyhow::{bail, Context, Result};
use clap::Parser;
use ignore::WalkBuilder;
use jsastgen_core::{parse_file, write_json};
use regex::Regex;
use std::path::{Path, PathBuf};

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

    let exclude = args.exclude_regex.as_deref().map(Regex::new).transpose()?;
    let files = collect_inputs(
        &input,
        exclude.as_ref(),
        &args.exclude_files,
        &args.language_type,
    )?;
    for file in files {
        let target = output_path(&input, &out, &file);
        match parse_file(&input_root(&input), &file).and_then(|value| write_json(&target, &value)) {
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
    exclude: Option<&Regex>,
    exclude_files: &[PathBuf],
    language_type: &str,
) -> Result<Vec<PathBuf>> {
    if input.is_file() {
        if is_supported_input(input, language_type) && !is_excluded(input, exclude, exclude_files) {
            return Ok(vec![input.to_path_buf()]);
        }
        bail!(
            "input file is not a supported {language_type} source: {}",
            input.display()
        );
    }

    let mut files = Vec::new();
    for entry in WalkBuilder::new(input).hidden(false).build() {
        let entry = entry?;
        let path = entry.path();
        if path.is_file()
            && is_supported_input(path, language_type)
            && !is_excluded(path, exclude, exclude_files)
        {
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

fn is_excluded(path: &Path, exclude: Option<&Regex>, exclude_files: &[PathBuf]) -> bool {
    exclude.is_some_and(|re| re.is_match(&path.to_string_lossy()))
        || exclude_files.iter().any(|excluded| excluded == path)
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
