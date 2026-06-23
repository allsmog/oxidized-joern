use anyhow::{anyhow, Context, Result};
use clap::Parser;
use ignore::WalkBuilder;
use std::fs;
use std::path::{Path, PathBuf};

const VERSION: &str = "0.1.0";

#[derive(Debug, Parser)]
#[command(name = "abapgen", disable_version_flag = true)]
struct Args {
    #[arg(long = "version", action = clap::ArgAction::SetTrue)]
    version: bool,
    #[arg(short = 'v', action = clap::ArgAction::SetTrue)]
    short_version: bool,
    #[arg(name = "input")]
    input: Option<PathBuf>,
    #[arg(name = "out")]
    out: Option<PathBuf>,
    #[arg(name = "rest", trailing_var_arg = true, allow_hyphen_values = true)]
    rest: Vec<String>,
}

fn main() -> Result<()> {
    let args = Args::parse();
    if args.version || args.short_version || args.rest.iter().any(|x| x == "-version") {
        println!("{VERSION}");
        return Ok(());
    }

    let input = args.input.ok_or_else(|| anyhow!("missing input path"))?;
    let out = args.out.ok_or_else(|| anyhow!("missing output path"))?;
    fs::create_dir_all(&out)?;

    for file in source_files(&input)? {
        match write_file(&input, &out, &file) {
            Ok(target) => println!("OK {} -> {}", file.display(), target.display()),
            Err(err) => println!("ERR {}: {err:#}", file.display()),
        }
    }

    // Surface classifier blind spots on stderr without polluting stdout/JSON.
    let unclassified = abapastgen_core::unclassified_count();
    if unclassified > 0 {
        eprintln!("abapastgen: {unclassified} unclassified statement(s)");
    }

    Ok(())
}

fn write_file(input: &Path, out: &Path, file: &Path) -> Result<PathBuf> {
    let relative = relative_path(input, file);
    let display_file = relative.to_string_lossy().replace('\\', "/");
    let program = abapastgen_core::generate_file(file, &display_file)?;
    let target = out.join(relative).with_extension(format!(
        "{}.json",
        file.extension().and_then(|x| x.to_str()).unwrap_or("abap")
    ));
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&target, serde_json::to_vec(&program)?)
        .with_context(|| format!("writing {}", target.display()))?;
    Ok(target)
}

fn source_files(input: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    if input.is_file() {
        if is_abap_file(input) {
            files.push(input.to_path_buf());
        }
        return Ok(files);
    }

    for entry in WalkBuilder::new(input).standard_filters(true).build() {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() && is_abap_file(path) {
            files.push(path.to_path_buf());
        }
    }
    files.sort();
    Ok(files)
}

fn is_abap_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|x| x.to_str())
        .is_some_and(|name| name.to_ascii_lowercase().ends_with(".abap"))
}

fn relative_path<'a>(input: &'a Path, file: &'a Path) -> &'a Path {
    if input.is_file() {
        file.file_name().map(Path::new).unwrap_or(file)
    } else {
        file.strip_prefix(input).unwrap_or(file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_abap_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let abap = dir.path().join("a.clas.abap");
        let txt = dir.path().join("a.txt");
        fs::write(&abap, "CLASS a DEFINITION. ENDCLASS.").expect("abap");
        fs::write(&txt, "ignored").expect("txt");

        assert_eq!(source_files(dir.path()).expect("files"), vec![abap]);
    }

    #[test]
    fn output_keeps_abap_extension() {
        let dir = tempfile::tempdir().expect("tempdir");
        let out = tempfile::tempdir().expect("out");
        let abap = dir.path().join("a.clas.abap");
        fs::write(&abap, "CLASS a DEFINITION. ENDCLASS.").expect("abap");

        let target = write_file(dir.path(), out.path(), &abap).expect("write");
        assert_eq!(target, out.path().join("a.clas.abap.json"));
    }
}
