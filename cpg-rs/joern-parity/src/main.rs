//! Differential harness for the shipped exact C lowering.

mod production;

fn main() {
    let mut args = std::env::args().skip(1);
    let mode = match args.next() {
        Some(arg) if arg == "--production" => production::Mode::Production,
        Some(arg) if arg == "--migration-report" => production::Mode::MigrationReport,
        Some(arg) => {
            let mut paths = vec![arg];
            paths.extend(args);
            print!("{}", cpg_lang_c::exact::canonical_dump_paths(&paths));
            return;
        }
        None => {
            eprintln!("usage: joern-parity [--production|--migration-report] <file.c>...");
            std::process::exit(2);
        }
    };
    let paths: Vec<String> = args.collect();
    if paths.is_empty() {
        eprintln!("usage: joern-parity [--production|--migration-report] <file.c>...");
        std::process::exit(2);
    }

    match mode {
        production::Mode::Production => print!("{}", production::dump_paths(&paths)),
        production::Mode::MigrationReport => {
            let standalone = cpg_lang_c::exact::canonical_dump_paths(&paths);
            let shipped = production::dump_paths(&paths);
            print!("{}", production::migration_report(&standalone, &shipped));
        }
    }
}
