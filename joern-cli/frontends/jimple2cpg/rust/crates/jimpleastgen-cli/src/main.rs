use anyhow::{Context, Result};
use clap::Parser;
use jimpleastgen_core::{generate_manifest, write_manifest, GenerateOptions};
use regex::Regex;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "jimpleastgen", disable_version_flag = true)]
struct Args {
    #[arg(long = "version", action = clap::ArgAction::SetTrue)]
    version: bool,

    #[arg(short = 'o', long = "out", value_name = "OUT")]
    out: Option<PathBuf>,

    #[arg(long = "recurse", action = clap::ArgAction::SetTrue)]
    recurse: bool,

    #[arg(long = "depth", default_value_t = 1)]
    depth: usize,

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
        println!("{}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    let input = args.input.context("missing input path")?;
    let out = args.out.context("missing -out <dir>")?;
    let exclude = args.exclude.as_deref().map(Regex::new).transpose()?;
    let manifest = generate_manifest(GenerateOptions {
        input: &input,
        out: &out,
        recurse: args.recurse,
        depth: args.depth,
        exclude: exclude.as_ref(),
    })?;
    let manifest_path = out.join("manifest.json");
    write_manifest(&manifest_path, &manifest)?;
    println!(
        "Extracted {} class files to {}",
        manifest.classes.len(),
        out.display()
    );
    for skipped in &manifest.skipped {
        println!("Skipped {} {}", skipped.path, skipped.reason);
    }
    Ok(())
}

fn normalized_args() -> Vec<String> {
    std::env::args()
        .map(|arg| match arg.as_str() {
            "-out" => "--out".into(),
            "-recurse" => "--recurse".into(),
            "-depth" => "--depth".into(),
            "-exclude" => "--exclude".into(),
            "-version" => "--version".into(),
            _ => arg,
        })
        .collect()
}
