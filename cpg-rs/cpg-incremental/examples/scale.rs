//! Scale demonstration: build a large synthetic C codebase, then show that
//! editing a single file costs work proportional to the *change*, not the
//! whole project. Run with:
//!
//!     cargo run --release -p cpg-incremental --example scale
//!
//! This substantiates the two load-bearing claims: the columnar store handles
//! a large graph in modest memory, and the incremental driver makes a one-file
//! edit cheap regardless of project size.

use cpg_analysis::standard_pipeline;
use cpg_incremental::{Project, UpdateOutcome};
use cpg_lang_c::CFrontend;
use std::time::Instant;

fn gen_file(file_idx: usize, fns_per_file: usize) -> String {
    let mut s = String::new();
    for f in 0..fns_per_file {
        // Each function has params, locals, a call, and a return — realistic
        // shape so the graph isn't trivially small.
        s.push_str(&format!(
            "int f{file_idx}_{f}(int a, int b) {{\n  \
               int t = a + b;\n  \
               int u = helper(t);\n  \
               return u + b;\n}}\n"
        ));
    }
    s
}

fn main() {
    let n_files: usize = std::env::var("FILES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2000);
    let fns_per_file: usize = std::env::var("FNS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20);

    // A shared helper so call resolution has cross-file work to do.
    let mut sources: Vec<(String, String)> = vec![(
        "helper.c".to_string(),
        "int helper(int x) { return x; }\n".to_string(),
    )];
    for i in 0..n_files {
        sources.push((format!("f{i}.c"), gen_file(i, fns_per_file)));
    }
    let total_fns = n_files * fns_per_file + 1;
    let approx_lines: usize = sources.iter().map(|(_, s)| s.lines().count()).sum();

    let refs: Vec<(&str, &str)> = sources
        .iter()
        .map(|(p, s)| (p.as_str(), s.as_str()))
        .collect();

    let mut p = Project::new(|| Box::new(CFrontend::new()), standard_pipeline());

    let t0 = Instant::now();
    let stats = p.build(&refs);
    let build = t0.elapsed();

    println!("== full build ==");
    println!(
        "phase parse+build: {:?}  (parallel {:?} + merge {:?})",
        stats.parse_build, stats.parallel_frontend, stats.merge
    );
    println!("phase passes:      {:?}", stats.passes);
    println!("phase summaries:   {:?}", stats.summaries);
    println!("files:            {}", refs.len());
    println!("functions:        {}", total_fns);
    println!("approx LOC:       {}", approx_lines);
    println!("live nodes:       {}", node_count(&p));
    println!("interned strings: {}", p.cpg.strings.len());
    println!("summaries:        {}", p.summaries.len());
    println!("build time:       {:?}", build);
    println!(
        "throughput:       {:.0} functions/sec",
        total_fns as f64 / build.as_secs_f64()
    );

    // Persist and reload the columnar graph.
    let path = std::env::temp_dir().join("scale.cpg");
    let path = path.to_str().unwrap();
    let ts = Instant::now();
    p.cpg.save(path).unwrap();
    let save = ts.elapsed();
    let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let tl = Instant::now();
    let reloaded = cpg_core::Cpg::load(path).unwrap();
    let load = tl.elapsed();
    println!("\n== persistence ==");
    println!("on-disk size:     {} MB", size / (1024 * 1024));
    println!("bytes/node:       {}", size / node_count(&p).max(1) as u64);
    println!("save time:        {save:?}");
    println!(
        "load time:        {load:?}  ({} nodes, no parsing)",
        reloaded.live_count()
    );
    let _ = std::fs::remove_file(path);

    // Now edit ONE file and measure.
    let edited = gen_file(0, fns_per_file) + "\nint extra(int z){ return z; }\n";
    let t1 = Instant::now();
    let outcome = p.update_file("f0.c", &edited);
    let inc = t1.elapsed();

    println!("\n== incremental edit (1 file of {}) ==", refs.len());
    match outcome {
        UpdateOutcome::Rebuilt {
            files_reanalysed,
            summaries_recomputed,
        } => {
            println!("files re-analysed:     {files_reanalysed}");
            println!(
                "summaries recomputed:  {summaries_recomputed}  (vs {} total)",
                p.summaries.len()
            );
        }
        UpdateOutcome::Unchanged => println!("unchanged"),
        UpdateOutcome::FullRebuildRequired => println!("full source rebuild required"),
    }
    println!("incremental time:      {:?}", inc);
    println!(
        "speedup vs full build: {:.0}x",
        build.as_secs_f64() / inc.as_secs_f64().max(1e-9)
    );
}

fn node_count(p: &Project) -> usize {
    p.cpg.live_count()
}
