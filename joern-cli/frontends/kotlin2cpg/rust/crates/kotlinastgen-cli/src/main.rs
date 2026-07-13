use anyhow::{bail, Context, Result};
use clap::Parser;
use ignore::WalkBuilder;
use kotlinastgen_core::{output_path, parse_file, write_json};
use regex::Regex;
use std::path::{Path, PathBuf};

#[derive(Parser, Debug)]
#[command(name = "kotlinastgen", disable_version_flag = true)]
struct Args {
    #[arg(long = "version", action = clap::ArgAction::SetTrue)]
    version: bool,

    #[arg(short = 'o', long = "out", value_name = "OUT")]
    out: Option<PathBuf>,

    #[arg(long = "exclude", value_name = "REGEX")]
    exclude: Option<String>,

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
        println!("v{}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    let input = args.input.context("missing input path")?;
    let out = args.out.context("missing -out <dir>")?;
    let exclude = args.exclude.as_deref().map(Regex::new).transpose()?;
    let files = collect_inputs(&input, exclude.as_ref())?;
    let root = input_root(&input);

    for file in files {
        let target = output_path(&input, &out, &file);
        match parse_file(&root, &file).and_then(|document| write_json(&target, &document)) {
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
            "-out" => "--out".into(),
            "-exclude" => "--exclude".into(),
            "-version" => "--version".into(),
            _ => arg,
        })
        .collect()
}

fn collect_inputs(input: &Path, exclude: Option<&Regex>) -> Result<Vec<PathBuf>> {
    if input.is_file() {
        if is_kotlin_input(input) && !is_excluded(input, exclude) {
            return Ok(vec![input.to_path_buf()]);
        }
        bail!("input file is not a .kt or .kts file: {}", input.display());
    }

    let mut files = Vec::new();
    for entry in WalkBuilder::new(input).hidden(false).build() {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() && is_kotlin_input(path) && !is_excluded(path, exclude) {
            files.push(path.to_path_buf());
        }
    }
    files.sort();
    Ok(files)
}

fn is_kotlin_input(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|value| value.to_str()),
        Some("kt" | "kts")
    )
}

fn is_excluded(path: &Path, exclude: Option<&Regex>) -> bool {
    exclude.is_some_and(|regex| regex.is_match(&path.to_string_lossy()))
}

fn input_root(input: &Path) -> PathBuf {
    if input.is_dir() {
        input.to_path_buf()
    } else {
        input
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
    }
}
