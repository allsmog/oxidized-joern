//! Differential harness for the shipped exact C lowering.

mod production;
mod supplemental;

fn main() {
    let mut args = std::env::args().skip(1);
    let mode = match args.next() {
        Some(arg) if arg == "--production-supplemental" => {
            let paths: Vec<String> = args.collect();
            if paths.is_empty() {
                eprintln!("usage: joern-parity --production-supplemental <file.c>...");
                std::process::exit(2);
            }
            match cpg_lang_c::exact::read_sources_from_paths(&paths) {
                Ok(sources) => print!(
                    "{}",
                    supplemental::snapshot(&production::build_graph(&sources))
                ),
                Err(error) => {
                    eprintln!("read production supplemental inputs: {error}");
                    std::process::exit(1);
                }
            }
            return;
        }
        Some(arg) if arg == "--supplemental-cpg" => {
            let paths: Vec<String> = args.collect();
            if paths.len() != 1 {
                eprintln!("usage: joern-parity --supplemental-cpg <graph.cpg>");
                std::process::exit(2);
            }
            match cpg_core::Cpg::load(&paths[0]) {
                Ok(cpg) => print!("{}", supplemental::snapshot(&cpg)),
                Err(error) => {
                    eprintln!("{error}");
                    std::process::exit(1);
                }
            }
            return;
        }
        Some(arg) if arg == "--lowering" => {
            let paths: Vec<String> = args.collect();
            if paths.is_empty() {
                eprintln!("usage: joern-parity --lowering <file.c>...");
                std::process::exit(2);
            }
            print!("{}", cpg_lang_c::exact::canonical_dump_paths(&paths));
            return;
        }
        Some(arg) if arg == "--update-equivalence" => {
            let paths: Vec<String> = args.collect();
            if paths.is_empty() {
                eprintln!("usage: joern-parity --update-equivalence <file.c>...");
                std::process::exit(2);
            }
            match production::update_equivalence(&paths) {
                Ok(count) => println!("update-equivalent {count} files"),
                Err(error) => {
                    eprintln!("{error}");
                    std::process::exit(1);
                }
            }
            return;
        }
        Some(arg) if arg == "--production" => production::Mode::Production,
        Some(arg) if arg == "--migration-report" => production::Mode::MigrationReport,
        Some(arg) => {
            let mut paths = vec![arg];
            paths.extend(args);
            print!("{}", production::dump_paths(&paths));
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
