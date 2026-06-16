use anyhow::{bail, Context, Result};
use clap::Parser;
use cxxastgen_core::{is_cxx_input, parse_file, write_json, ParseOptions};
use regex::Regex;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Parser, Debug)]
#[command(name = "cxxastgen", disable_version_flag = true)]
struct Args {
    #[arg(long = "version", action = clap::ArgAction::SetTrue)]
    version: bool,

    #[arg(short = 'o', long = "out", value_name = "OUT")]
    out: Option<PathBuf>,

    #[arg(long = "include", value_name = "PATH")]
    include_paths: Vec<PathBuf>,

    #[arg(long = "define", value_name = "NAME[=VALUE]")]
    defines: Vec<String>,

    #[arg(long = "compilation-database", value_name = "COMPILE_COMMANDS_JSON")]
    compilation_database: Option<PathBuf>,

    #[arg(long = "skip-function-bodies", action = clap::ArgAction::SetTrue)]
    skip_function_bodies: bool,

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
    let options = ParseOptions {
        include_paths: args
            .include_paths
            .iter()
            .map(|path| display_path(path))
            .collect(),
        defines: args.defines,
        compilation_database: args
            .compilation_database
            .as_ref()
            .map(|path| display_path(path)),
        skip_function_bodies: args.skip_function_bodies,
    };

    let files = collect_inputs(&input, exclude.as_ref())?;
    for file in files {
        let target = output_path(&input, &out, &file);
        match parse_file(&file, &options).and_then(|document| write_json(&target, &document)) {
            Ok(()) => println!(
                "Converted AST scaffold for {} to {}",
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
            "-include" => "--include".into(),
            "-define" => "--define".into(),
            "-compilation-database" => "--compilation-database".into(),
            "-skip-function-bodies" => "--skip-function-bodies".into(),
            "-exclude" => "--exclude".into(),
            "-version" => "--version".into(),
            _ => arg,
        })
        .collect()
}

fn collect_inputs(input: &Path, exclude: Option<&Regex>) -> Result<Vec<PathBuf>> {
    if input.is_file() {
        if is_cxx_input(input) && !is_excluded(input, exclude) {
            return Ok(vec![input.to_path_buf()]);
        }
        bail!(
            "input file is not a supported C/C++ source: {}",
            input.display()
        );
    }

    let mut files = Vec::new();
    collect_inputs_from_dir(input, exclude, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_inputs_from_dir(
    dir: &Path,
    exclude: Option<&Regex>,
    files: &mut Vec<PathBuf>,
) -> Result<()> {
    for entry in fs::read_dir(dir)
        .with_context(|| format!("failed to read directory '{}'", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_inputs_from_dir(&path, exclude, files)?;
        } else if path.is_file() && is_cxx_input(&path) && !is_excluded(&path, exclude) {
            files.push(path);
        }
    }
    Ok(())
}

fn is_excluded(path: &Path, exclude: Option<&Regex>) -> bool {
    exclude.is_some_and(|regex| regex.is_match(&display_path(path)))
}

fn output_path(input: &Path, out: &Path, file: &Path) -> PathBuf {
    let relative = if input.is_dir() {
        file.strip_prefix(input).unwrap_or(file).to_path_buf()
    } else {
        file.file_name()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("out"))
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

fn display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
