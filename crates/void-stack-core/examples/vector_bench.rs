//! `cargo run --release -p void-stack-core --example vector_bench --features bench -- \
//!     <backend|quality|evalset> <project>[,<project>...] [model,model,...]`
//!
//! Models: stored, bge, bge-instr, bge-q, e5-multi, jina-code, nomic-q.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(cmd), Some(names)) = (args.first(), args.get(1)) else {
        eprintln!("usage: vector_bench <backend|quality|evalset> <project,...> [models]");
        std::process::exit(2);
    };
    let models: Vec<String> = args
        .get(2)
        .map(|m| m.split(',').map(str::to_string).collect())
        .unwrap_or_else(|| vec!["stored".into()]);
    let config = void_stack_core::global_config::load_global_config().expect("global config");
    let projects: Vec<_> = names
        .split(',')
        .map(|n| {
            void_stack_core::global_config::find_project(&config, n)
                .unwrap_or_else(|| panic!("no project {n}"))
                .clone()
        })
        .collect();
    if let Err(e) = void_stack_core::vector_index::bench::run(cmd, &projects, &models) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
