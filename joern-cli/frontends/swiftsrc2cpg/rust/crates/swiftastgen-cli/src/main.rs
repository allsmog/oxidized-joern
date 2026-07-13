use anyhow::{bail, Context, Result};
use clap::Parser;
use ignore::WalkBuilder;
use regex::Regex;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use swiftastgen_core::{parse_file, take_unsupported_node_tally, write_json};

const SCALA_AST_SOURCE: &str =
    include_str!("../../../../src/main/scala/io/joern/swiftsrc2cpg/parser/SwiftNodeSyntax.scala");

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
        write_stdout(SCALA_AST_SOURCE)?;
        return Ok(());
    }

    let out = args.out.context("missing -o <dir>")?;
    let input = args.input.unwrap_or(std::env::current_dir()?);
    let exclude = args
        .exclude_regex
        .as_deref()
        .map(compile_exclude_regex)
        .transpose()?;
    let files = collect_inputs(&input, exclude.as_ref())?;
    for file in files {
        let target = output_path(&input, &out, &file);
        match parse_file(&input, &file).and_then(|value| write_json(&target, &value)) {
            Ok(()) => println!("Generated AST for file: `{}`", file.display()),
            Err(err) => println!("{} {}", file.display(), err),
        }
    }
    report_unsupported_nodes();
    Ok(())
}

/// Emits a single stderr summary of node kinds that could not be mapped to a
/// precise SwiftSyntax node and were degraded to a best-effort placeholder.
/// Stays silent (and off stdout/JSON) when everything mapped cleanly so the
/// `--scalaAstOnly` golden contract keeps an empty stderr.
fn report_unsupported_nodes() {
    let tally = take_unsupported_node_tally();
    if tally.is_empty() {
        return;
    }
    let total: usize = tally.iter().map(|(_, count)| count).sum();
    let breakdown = tally
        .iter()
        .map(|(kind, count)| format!("{kind}(x{count})"))
        .collect::<Vec<_>>()
        .join(", ");
    eprintln!("swiftastgen: {total} unsupported node(s) degraded to placeholders: {breakdown}");
}

fn write_stdout(value: &str) -> Result<()> {
    let mut stdout = io::stdout().lock();
    match stdout
        .write_all(value.as_bytes())
        .and_then(|_| stdout.flush())
    {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        Err(err) => Err(err).context("failed to write stdout"),
    }
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
    let input_root = input_root(input);
    if input.is_file() {
        if is_swift_input(input) && !is_excluded(input, &input_root, exclude) {
            return Ok(vec![input.to_path_buf()]);
        }
        bail!("input file is not a .swift file: {}", input.display());
    }

    let mut files = Vec::new();
    let walk_input_root = input_root.clone();
    for entry in WalkBuilder::new(input)
        .hidden(false)
        .filter_entry(move |entry| !is_default_ignored_entry(&walk_input_root, entry.path()))
        .build()
    {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() && is_swift_input(path) && !is_excluded(path, &input_root, exclude) {
            files.push(path.to_path_buf());
        }
    }
    files.sort();
    Ok(files)
}

fn is_swift_input(path: &Path) -> bool {
    path.extension().and_then(|x| x.to_str()) == Some("swift")
}

fn is_excluded(path: &Path, input_root: &Path, exclude: Option<&Regex>) -> bool {
    exclude.is_some_and(|regex| {
        regex.is_match(&path.to_string_lossy())
            || path
                .strip_prefix(input_root)
                .ok()
                .is_some_and(|relative| regex.is_match(&relative.to_string_lossy()))
    })
}

fn is_default_ignored_entry(input: &Path, path: &Path) -> bool {
    if path == input {
        return false;
    }
    let Ok(relative) = path.strip_prefix(input) else {
        return false;
    };
    let Some(first_component) = relative.components().next() else {
        return false;
    };
    let name = first_component.as_os_str().to_string_lossy();
    name.starts_with('.')
        || name.starts_with("__")
        || matches!(name.as_ref(), "tests" | "specs" | "test" | "spec")
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

fn input_root(input: &Path) -> PathBuf {
    if input.is_dir() {
        input.to_path_buf()
    } else {
        input.parent().unwrap_or(input).to_path_buf()
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
