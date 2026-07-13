use anyhow::{anyhow, Context, Result};
use clap::Parser;
use ignore::WalkBuilder;
use std::fs;
use std::path::{Path, PathBuf};

const VERSION: &str = "0.1.0";

#[derive(Debug, Parser)]
#[command(name = "rubyastgen", disable_version_flag = true)]
struct Args {
    #[arg(long = "version", action = clap::ArgAction::SetTrue)]
    version: bool,
    #[arg(short = 'v', action = clap::ArgAction::SetTrue)]
    short_version: bool,
    #[arg(short = 'o', long = "output")]
    output: Option<PathBuf>,
    #[arg(short = 'e', long = "exclude")]
    exclude: Option<String>,
    #[arg(name = "args", trailing_var_arg = true, allow_hyphen_values = true)]
    args: Vec<String>,
}

fn main() -> Result<()> {
    let args = Args::parse();
    if args.version || args.short_version || args.args.iter().any(|x| x == "-version") {
        println!("{VERSION}");
        return Ok(());
    }

    let (input, output) = input_and_output(&args)?;
    let files = source_files(&input, args.exclude.as_deref())?;
    for file in files {
        emit_file(&file, &input, &output)
            .with_context(|| format!("generating {}", file.display()))?;
    }
    if let Some(summary) = rubyastgen_core::take_unknown_node_summary() {
        eprintln!("{summary}");
    }
    Ok(())
}

fn input_and_output(args: &Args) -> Result<(PathBuf, PathBuf)> {
    if args.args.len() >= 2 {
        return Ok((PathBuf::from(&args.args[0]), PathBuf::from(&args.args[1])));
    }
    let input = args
        .args
        .first()
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("missing input path"))?;
    let output = args
        .output
        .clone()
        .ok_or_else(|| anyhow!("missing output path"))?;
    Ok((input, output))
}

fn emit_file(file: &Path, input_root: &Path, output_root: &Path) -> Result<()> {
    let json = rubyastgen_core::generate_file(file, input_root)?;
    let rel = output_relative_path(file, input_root);
    let mut out_file = output_root.join(rel);
    let out_name = out_file
        .file_name()
        .map(|name| format!("{}.json", name.to_string_lossy()))
        .ok_or_else(|| anyhow!("invalid output file name for {}", file.display()))?;
    out_file.set_file_name(out_name);
    if let Some(parent) = out_file.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&out_file, serde_json::to_string_pretty(&json)?)?;
    println!(
        "[INFO] Processed: {} -> {}",
        file.display(),
        out_file.display()
    );
    Ok(())
}

fn source_files(input: &Path, exclude: Option<&str>) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    if input.is_file() {
        if is_ruby_file(input) && !is_excluded(input, exclude) {
            files.push(input.to_path_buf());
        }
    } else if input.is_dir() {
        for entry in WalkBuilder::new(input).standard_filters(true).build() {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() && is_ruby_file(path) && !is_excluded(path, exclude) {
                files.push(path.to_path_buf());
            }
        }
    } else {
        return Err(anyhow!("input path does not exist: {}", input.display()));
    }
    files.sort();
    Ok(files)
}

fn is_ruby_file(path: &Path) -> bool {
    path.extension()
        .and_then(|x| x.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("rb") || ext.eq_ignore_ascii_case("erb"))
}

fn is_excluded(path: &Path, exclude: Option<&str>) -> bool {
    let Some(exclude) = exclude.filter(|x| !x.is_empty()) else {
        return false;
    };
    let path = path.to_string_lossy();
    path.contains(exclude)
}

fn output_relative_path(path: &Path, input_root: &Path) -> PathBuf {
    let canonical_path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let canonical_root = fs::canonicalize(input_root).unwrap_or_else(|_| input_root.to_path_buf());
    if canonical_root.is_file() {
        canonical_path
            .file_name()
            .map(PathBuf::from)
            .unwrap_or_else(|| canonical_path.clone())
    } else {
        canonical_path
            .strip_prefix(&canonical_root)
            .map(PathBuf::from)
            .unwrap_or(canonical_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_files_finds_rb_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let rb = dir.path().join("a.rb");
        let txt = dir.path().join("a.txt");
        fs::write(&rb, "puts 1").expect("rb");
        fs::write(txt, "no").expect("txt");
        assert_eq!(source_files(dir.path(), None).expect("files"), vec![rb]);
    }

    #[test]
    fn emit_file_preserves_source_extension_before_json_suffix() {
        let dir = tempfile::tempdir().expect("tempdir");
        let src = dir.path().join("src");
        let out = dir.path().join("out");
        fs::create_dir(&src).expect("src");
        let erb = src.join("test.erb");
        fs::write(&erb, "<%= name %>").expect("erb");

        emit_file(&erb, &src, &out).expect("emit");

        assert!(out.join("test.erb.json").is_file());
    }
}
