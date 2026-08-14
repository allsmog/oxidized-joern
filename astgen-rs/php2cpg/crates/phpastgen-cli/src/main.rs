use anyhow::{anyhow, Context, Result};
use clap::Parser;
use ignore::WalkBuilder;
use std::fs;
use std::path::{Path, PathBuf};

const VERSION: &str = "0.1.0";

#[derive(Debug, Parser)]
#[command(name = "phpastgen", disable_version_flag = true)]
struct Args {
    #[arg(long = "version", action = clap::ArgAction::SetTrue)]
    version: bool,
    #[arg(short = 'v', action = clap::ArgAction::SetTrue)]
    short_version: bool,
    #[arg(name = "args", trailing_var_arg = true, allow_hyphen_values = true)]
    args: Vec<String>,
}

fn main() -> Result<()> {
    let args = Args::parse();
    if args.version || args.short_version || args.args.iter().any(|x| x == "-version") {
        println!("{VERSION}");
        return Ok(());
    }

    let inputs = source_files(&args.args)?;
    let (result, summary) = phpastgen_core::with_unmapped_summary(|| {
        for file in &inputs {
            emit_file(file).with_context(|| format!("generating {}", file.display()))?;
        }
        Ok(())
    });
    if let Some(summary) = summary {
        eprintln!("{summary}");
    }
    result
}

fn emit_file(file: &Path) -> Result<()> {
    let canonical = fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    println!("====> File {}:", canonical.display());
    println!("==> Resolved names.");
    println!("==> JSON dump:");
    match phpastgen_core::generate_file(file) {
        Ok(json) => println!("{}", serde_json::to_string_pretty(&json)?),
        Err(err) => {
            eprintln!("Failed to parse {}: {err:#}", file.display());
            println!("[]");
        }
    }
    Ok(())
}

fn source_files(args: &[String]) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for arg in args.iter().filter(|arg| !arg.starts_with("--")) {
        let path = PathBuf::from(arg);
        if path.is_file() {
            if is_php_file(&path) {
                files.push(path);
            }
        } else if path.is_dir() {
            for entry in WalkBuilder::new(&path).standard_filters(true).build() {
                let entry = entry?;
                let entry_path = entry.path();
                if entry_path.is_file() && is_php_file(entry_path) {
                    files.push(entry_path.to_path_buf());
                }
            }
        } else if arg.ends_with(".php") {
            return Err(anyhow!("input file does not exist: {arg}"));
        }
    }
    files.sort();
    Ok(files)
}

fn is_php_file(path: &Path) -> bool {
    path.extension()
        .and_then(|x| x.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("php"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_files_ignores_flags() {
        let dir = tempfile::tempdir().expect("tempdir");
        let php = dir.path().join("a.php");
        fs::write(&php, "<?php echo 1;").expect("php");

        let args = vec![
            "--with-recovery".to_string(),
            "--json-dump".to_string(),
            php.to_string_lossy().to_string(),
        ];
        assert_eq!(source_files(&args).expect("files"), vec![php]);
    }
}
