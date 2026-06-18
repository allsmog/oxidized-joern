use anyhow::{bail, Context, Result};
use clap::Parser;
use ignore::WalkBuilder;
use regex::Regex;
use std::path::{Path, PathBuf};
use swiftastgen_core::{parse_file, write_json};

#[derive(Parser, Debug)]
#[command(name = "SwiftAstGen", disable_version_flag = true)]
struct Args {
    #[arg(long = "version", action = clap::ArgAction::SetTrue)]
    version: bool,

    #[arg(long = "scalaAstOnly", action = clap::ArgAction::SetTrue)]
    scala_ast_only: bool,

    #[arg(short = 'o', long = "out", value_name = "OUT")]
    out: Option<PathBuf>,

    #[arg(long = "exclude-regex", value_name = "REGEX")]
    exclude_regex: Option<String>,

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
    if args.scala_ast_only {
        bail!("--scalaAstOnly is generated from upstream SwiftSyntax metadata and is not implemented by oxidized SwiftAstGen");
    }

    let out = args.out.context("missing -o <dir>")?;
    let input = args.input.unwrap_or(std::env::current_dir()?);
    let exclude = args.exclude_regex.as_deref().map(Regex::new).transpose()?;
    let files = collect_inputs(&input, exclude.as_ref())?;
    for file in files {
        let target = output_path(&input, &out, &file);
        match parse_file(&input, &file).and_then(|value| write_json(&target, &value)) {
            Ok(()) => println!("Generated AST for file: `{}`", file.display()),
            Err(err) => println!("{} {}", file.display(), err),
        }
    }
    Ok(())
}

fn normalized_args() -> Vec<String> {
    std::env::args()
        .map(|arg| match arg.as_str() {
            "-o" => "--out".into(),
            "-version" => "--version".into(),
            _ => arg,
        })
        .collect()
}

fn collect_inputs(input: &Path, exclude: Option<&Regex>) -> Result<Vec<PathBuf>> {
    if input.is_file() {
        if is_swift_input(input) && !is_excluded(input, exclude) {
            return Ok(vec![input.to_path_buf()]);
        }
        bail!("input file is not a .swift file: {}", input.display());
    }

    let mut files = Vec::new();
    for entry in WalkBuilder::new(input).hidden(false).build() {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() && is_swift_input(path) && !is_excluded(path, exclude) {
            files.push(path.to_path_buf());
        }
    }
    files.sort();
    Ok(files)
}

fn is_swift_input(path: &Path) -> bool {
    path.extension().and_then(|x| x.to_str()) == Some("swift")
}

fn is_excluded(path: &Path, exclude: Option<&Regex>) -> bool {
    exclude.is_some_and(|regex| regex.is_match(&path.to_string_lossy()))
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
