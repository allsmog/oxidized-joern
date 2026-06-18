use anyhow::{bail, Context, Result};
use clap::Parser;
use cxxastgen_core::{is_cxx_input, parse_file, write_json, ParseOptions};
use regex::Regex;
use serde::Deserialize;
use std::collections::{hash_map::Entry, HashMap};
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
    let compile_database = args
        .compilation_database
        .as_deref()
        .map(CompileDatabase::load)
        .transpose()?;
    let base_options = ParseOptions {
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
        import_header_declarations: args.compilation_database.is_some() || input.is_file(),
    };

    let files = collect_inputs(&input, exclude.as_ref(), compile_database.as_ref())?;
    for file in files {
        let target = output_path(&input, &out, &file);
        let options = options_for_file(&base_options, compile_database.as_ref(), &file);
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

#[derive(Debug, Deserialize)]
struct CompileCommand {
    directory: Option<PathBuf>,
    file: PathBuf,
    command: Option<String>,
    arguments: Option<Vec<String>>,
}

#[derive(Debug)]
struct CompileDatabase {
    files: Vec<PathBuf>,
    options_by_file: HashMap<PathBuf, CompileCommandOptions>,
}

impl CompileDatabase {
    fn load(path: &Path) -> Result<Self> {
        let data = fs::read_to_string(path)
            .with_context(|| format!("failed to read compilation database '{}'", path.display()))?;
        let commands: Vec<CompileCommand> = serde_json::from_str(&data).with_context(|| {
            format!("failed to parse compilation database '{}'", path.display())
        })?;
        let database_dir = path
            .parent()
            .map(normalized_absolute_path)
            .unwrap_or_else(|| normalized_absolute_path(Path::new(".")));

        let mut files = Vec::new();
        let mut options_by_file = HashMap::new();
        for command in commands {
            let working_dir =
                resolve_compile_command_directory(command.directory.as_deref(), &database_dir);
            let file = resolve_compile_command_file(&command, &working_dir);
            let key = normalized_absolute_path(&file);
            let arguments = compile_command_arguments(&command).with_context(|| {
                format!("failed to read compile command for '{}'", file.display())
            })?;
            let options = CompileCommandOptions::from_arguments(&arguments, &working_dir);
            if let Entry::Vacant(entry) = options_by_file.entry(key) {
                files.push(entry.key().clone());
                entry.insert(options);
            }
        }
        files.sort();

        Ok(Self {
            files,
            options_by_file,
        })
    }

    fn options_for(&self, file: &Path) -> Option<&CompileCommandOptions> {
        let key = normalized_absolute_path(file);
        self.options_by_file.get(&key)
    }
}

#[derive(Debug, Default)]
struct CompileCommandOptions {
    include_paths: Vec<String>,
    defines: Vec<String>,
}

impl CompileCommandOptions {
    fn from_arguments(arguments: &[String], working_dir: &Path) -> Self {
        let mut options = Self::default();
        let mut index = 0;
        while index < arguments.len() {
            let argument = &arguments[index];
            match argument.as_str() {
                "-I" | "/I" | "-isystem" | "-iquote" => {
                    if let Some(value) = arguments.get(index + 1) {
                        options
                            .include_paths
                            .push(normalized_option_path(value, working_dir));
                        index += 2;
                        continue;
                    }
                }
                "-D" | "/D" => {
                    if let Some(value) = arguments.get(index + 1) {
                        options.defines.push(value.clone());
                        index += 2;
                        continue;
                    }
                }
                _ => {
                    if let Some(value) = argument.strip_prefix("-I").filter(|x| !x.is_empty()) {
                        options
                            .include_paths
                            .push(normalized_option_path(value, working_dir));
                    } else if let Some(value) =
                        argument.strip_prefix("/I").filter(|x| !x.is_empty())
                    {
                        options
                            .include_paths
                            .push(normalized_option_path(value, working_dir));
                    } else if let Some(value) =
                        argument.strip_prefix("-D").filter(|x| !x.is_empty())
                    {
                        options.defines.push(value.to_string());
                    } else if let Some(value) =
                        argument.strip_prefix("/D").filter(|x| !x.is_empty())
                    {
                        options.defines.push(value.to_string());
                    }
                }
            }
            index += 1;
        }
        options.include_paths = dedupe(options.include_paths);
        options.defines = dedupe(options.defines);
        options
    }
}

fn resolve_compile_command_directory(directory: Option<&Path>, database_dir: &Path) -> PathBuf {
    match directory {
        Some(path) if path.is_absolute() => normalized_absolute_path(path),
        Some(path) => normalized_absolute_path(&database_dir.join(path)),
        None => database_dir.to_path_buf(),
    }
}

fn resolve_compile_command_file(command: &CompileCommand, working_dir: &Path) -> PathBuf {
    if command.file.is_absolute() {
        command.file.clone()
    } else {
        working_dir.join(&command.file)
    }
}

fn compile_command_arguments(command: &CompileCommand) -> Result<Vec<String>> {
    if let Some(arguments) = &command.arguments {
        return Ok(arguments.clone());
    }
    if let Some(command_line) = &command.command {
        return shlex::split(command_line).context("failed to parse shell command");
    }
    Ok(Vec::new())
}

fn options_for_file(
    base_options: &ParseOptions,
    compile_database: Option<&CompileDatabase>,
    file: &Path,
) -> ParseOptions {
    let mut options = base_options.clone();
    if let Some(command_options) = compile_database.and_then(|database| database.options_for(file))
    {
        options.include_paths = merge_unique(
            options.include_paths,
            command_options.include_paths.iter().cloned(),
        );
        options.defines = merge_unique(options.defines, command_options.defines.iter().cloned());
    }
    options
}

fn collect_inputs(
    input: &Path,
    exclude: Option<&Regex>,
    compile_database: Option<&CompileDatabase>,
) -> Result<Vec<PathBuf>> {
    if input.is_file() {
        if is_cxx_input(input) && !is_excluded(input, exclude) {
            return Ok(vec![input.to_path_buf()]);
        }
        bail!(
            "input file is not a supported C/C++ source: {}",
            input.display()
        );
    }

    if !input.is_dir() {
        bail!("input path is not a file or directory: {}", input.display());
    }

    if let Some(compile_database) = compile_database {
        let input_root = normalized_absolute_path(input);
        let mut files = compile_database
            .files
            .iter()
            .filter(|file| {
                is_cxx_input(file) && file.starts_with(&input_root) && !is_excluded(file, exclude)
            })
            .cloned()
            .collect::<Vec<_>>();
        files.sort();
        return Ok(files);
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
        relative_output_path(input, file)
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

fn relative_output_path(input: &Path, file: &Path) -> PathBuf {
    if let Ok(relative) = file.strip_prefix(input) {
        return relative.to_path_buf();
    }
    let input = normalized_absolute_path(input);
    let file = normalized_absolute_path(file);
    if let Ok(relative) = file.strip_prefix(input) {
        return relative.to_path_buf();
    }
    file.file_name()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("out"))
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn normalized_absolute_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .map(|current_dir| current_dir.join(path))
                .unwrap_or_else(|_| path.to_path_buf())
        }
    })
}

fn normalized_option_path(value: &str, working_dir: &Path) -> String {
    let path = Path::new(value);
    if path.is_absolute() {
        display_path(path)
    } else {
        display_path(&working_dir.join(path))
    }
}

fn merge_unique<I>(mut base: Vec<String>, additional: I) -> Vec<String>
where
    I: IntoIterator<Item = String>,
{
    for item in additional {
        if !base.contains(&item) {
            base.push(item);
        }
    }
    base
}

fn dedupe(values: Vec<String>) -> Vec<String> {
    merge_unique(Vec::new(), values)
}
