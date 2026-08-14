use anyhow::{anyhow, Context, Result};
use clap::Parser;
use ignore::WalkBuilder;
use regex::Regex;
use std::fs;
use std::path::{Path, PathBuf};

const VERSION: &str = "0.43.0";

#[derive(Debug, Parser)]
#[command(name = "dotnetastgen", disable_version_flag = true)]
struct Args {
    #[arg(short = 'i', long = "input")]
    input: Option<PathBuf>,
    #[arg(short = 'o', long = "out")]
    out: Option<PathBuf>,
    #[arg(short = 'e', long = "exclude")]
    exclude: Option<String>,
    #[arg(long = "version", action = clap::ArgAction::SetTrue)]
    version: bool,
    #[arg(short = 'v', action = clap::ArgAction::SetTrue)]
    short_version: bool,
    #[arg(name = "legacy-version", long = "legacy-version", hide = true, action = clap::ArgAction::SetTrue)]
    legacy_version: bool,
    #[arg(name = "rest", trailing_var_arg = true, allow_hyphen_values = true)]
    rest: Vec<String>,
}

fn main() -> Result<()> {
    let args = Args::parse();
    if args.version || args.short_version || args.rest.iter().any(|x| x == "-version") {
        println!("{VERSION}");
        return Ok(());
    }

    let input = args
        .input
        .ok_or_else(|| anyhow!("missing required -i/--input"))?;
    let out = args
        .out
        .ok_or_else(|| anyhow!("missing required -o/--out"))?;
    fs::create_dir_all(&out)?;

    let exclude = args
        .exclude
        .as_deref()
        .filter(|x| !x.is_empty())
        .map(Regex::new)
        .transpose()
        .context("invalid exclude regex")?;

    let files = source_files(&input, exclude.as_ref())?;
    for file in files {
        eprintln!(
            "info: DotNetAstGen.Program[0] Parsing file: {}",
            file.display()
        );
        let json = match dotnetastgen_core::generate_file(&file) {
            Ok(json) => json,
            Err(err) => {
                eprintln!(
                    "fail: DotNetAstGen.Program[0] Error(s) encountered while parsing: {}",
                    file.display()
                );
                eprintln!("fail: DotNetAstGen.Program[0] {err:#}");
                eprintln!(
                    "info: DotNetAstGen.Program[0] Skipping file: {}",
                    file.display()
                );
                continue;
            }
        };
        let target = output_path(&input, &out, &file);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&target, serde_json::to_vec(&json)?)?;
    }
    // One stderr summary of node kinds that fell through to `Unknown`. Kept off stdout
    // so the emitted JSON is never polluted.
    if let Some(summary) = dotnetastgen_core::take_unmapped_summary() {
        eprintln!("{summary}");
    }
    Ok(())
}

fn source_files(input: &Path, exclude: Option<&Regex>) -> Result<Vec<PathBuf>> {
    let cs_files = files_with_extension(input, exclude, "cs")?;
    if !cs_files.is_empty() {
        return Ok(cs_files);
    }
    files_with_extension(input, exclude, "xml")
}

fn files_with_extension(
    input: &Path,
    exclude: Option<&Regex>,
    extension: &str,
) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    if input.is_file() {
        if input.extension().is_some_and(|ext| ext == extension) && !is_excluded(input, exclude) {
            files.push(input.to_path_buf());
        }
        return Ok(files);
    }

    for entry in WalkBuilder::new(input).standard_filters(true).build() {
        let entry = entry?;
        let path = entry.path();
        if path.is_file()
            && path.extension().is_some_and(|ext| ext == extension)
            && !is_excluded(path, exclude)
        {
            files.push(path.to_path_buf());
        }
    }
    files.sort();
    Ok(files)
}

fn is_excluded(path: &Path, exclude: Option<&Regex>) -> bool {
    exclude.is_some_and(|regex| regex.is_match(&path.to_string_lossy()))
}

fn output_path(input: &Path, out: &Path, file: &Path) -> PathBuf {
    let input_is_file = input.is_file() || input.extension().is_some_and(|ext| ext == "cs");
    let relative = if input_is_file {
        file.file_name().map(Path::new).unwrap_or(file)
    } else {
        file.strip_prefix(input).unwrap_or(file)
    };
    out.join(relative).with_extension(format!(
        "{}.json",
        relative
            .extension()
            .and_then(|x| x.to_str())
            .unwrap_or("cs")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_output_keeps_cs_extension() {
        let out = output_path(
            Path::new("/tmp/Test.cs"),
            Path::new("/tmp/out"),
            Path::new("/tmp/Test.cs"),
        );
        assert_eq!(out, PathBuf::from("/tmp/out/Test.cs.json"));
    }

    #[test]
    fn dir_output_preserves_relative_path() {
        let out = output_path(
            Path::new("/tmp/src"),
            Path::new("/tmp/out"),
            Path::new("/tmp/src/a/b/Test.cs"),
        );
        assert_eq!(out, PathBuf::from("/tmp/out/a/b/Test.cs.json"));
    }

    #[test]
    fn source_files_prefers_cs_over_xml() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cs = dir.path().join("A.cs");
        let xml = dir.path().join("A.xml");
        fs::write(&cs, "class A {}").expect("cs");
        fs::write(&xml, "<doc />").expect("xml");

        assert_eq!(source_files(dir.path(), None).expect("files"), vec![cs]);
    }

    #[test]
    fn source_files_uses_xml_when_no_cs_exists() {
        let dir = tempfile::tempdir().expect("tempdir");
        let xml = dir.path().join("A.xml");
        fs::write(&xml, "<doc />").expect("xml");

        assert_eq!(source_files(dir.path(), None).expect("files"), vec![xml]);
    }
}
