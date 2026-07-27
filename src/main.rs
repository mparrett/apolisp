//! The process driver: argument handling, file I/O, stdout, exit codes.
//!
//! Language behaviour lives in `lib.rs` (ADR-031). Nothing here decides
//! anything about the language — if a change to this file changes what a
//! program means, it is in the wrong file.

use apolisp::{bytecode, compile, printer, reader, value};
use std::process::ExitCode;

/// A stage whose milestone has not landed, as distinct from a stage that ran
/// and failed. `smoke.sh` needs to tell those apart: without it, the first gap
/// in the pipeline hides every stage behind it, and the stages are not built in
/// pipeline order (expand is milestone 5; compile and run are 2 and 3).
///
/// Keep in sync with `NOT_IMPLEMENTED` in `smoke.sh`.
const EXIT_NOT_IMPLEMENTED: u8 = 3;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: apolisp <read|spans|sizes|expand|compile|run> [file.xs]");
        return ExitCode::from(2);
    }
    if args[1] == "sizes" {
        return report_sizes();
    }
    if args.len() < 3 {
        eprintln!("usage: apolisp <read|spans|expand|compile|run> <file.xs>");
        return ExitCode::from(2);
    }
    let (cmd, path) = (args[1].as_str(), args[2].as_str());

    let src = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("apolisp: {path}: {e}");
            return ExitCode::from(2);
        }
    };

    match cmd {
        "read" => {
            let mut interner = value::Interner::new();
            match reader::read_all(&src, &mut interner) {
                Ok(forms) => {
                    for f in &forms {
                        println!("{}", printer::print(&f.root, &interner));
                    }
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("{}", e.render(path, &src));
                    ExitCode::FAILURE
                }
            }
        }
        // Debug views. These exist because ADR-026 puts origins outside the
        // value graph, so the ordinary printed form cannot show them — and an
        // invariant nobody can see is one nobody checks.
        "spans" => {
            let mut interner = value::Interner::new();
            match reader::read_all(&src, &mut interner) {
                Ok(forms) => {
                    let mut problems = Vec::new();
                    for f in &forms {
                        value::check_origins(&f.root, &f.origins, &src, &mut problems);
                    }
                    for f in &forms {
                        println!(
                            "{}",
                            value::print_origins(&f.root, &f.origins, &interner, 0)
                        );
                    }
                    if problems.is_empty() {
                        println!("ok: span invariants hold");
                        ExitCode::SUCCESS
                    } else {
                        for p in &problems {
                            println!("VIOLATION: {p}");
                        }
                        ExitCode::FAILURE
                    }
                }
                Err(e) => {
                    eprintln!("{}", e.render(path, &src));
                    ExitCode::FAILURE
                }
            }
        }
        // Milestone 2. Compilation is not yet a function of source alone — it
        // acquires a VM dependency at milestone 5, when macros make it
        // unavoidable (ADR-004) — so for now reading and compiling is the whole
        // pipeline.
        "compile" => {
            let mut interner = value::Interner::new();
            let forms = match reader::read_all(&src, &mut interner) {
                Ok(forms) => forms,
                Err(e) => {
                    eprintln!("{}", e.render(path, &src));
                    return ExitCode::FAILURE;
                }
            };
            match compile::compile(&forms, &mut interner) {
                Ok(chunk) => {
                    print!("{}", bytecode::disassemble(&chunk, &interner, &src));
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("{}", e.render(path, &src));
                    ExitCode::FAILURE
                }
            }
        }
        // Stages whose milestone has not landed. They fail rather than no-op:
        // a smoke test that silently skips a stage stops being an oracle.
        "expand" | "run" => {
            eprintln!("apolisp: `{cmd}` is not implemented yet (see BUILD.md)");
            ExitCode::from(EXIT_NOT_IMPLEMENTED)
        }
        _ => {
            eprintln!("apolisp: unknown command `{cmd}`");
            ExitCode::from(2)
        }
    }
}

/// ADR-025: the size is asserted, not assumed. Reported here rather than in the
/// library because printing and exit codes are this file's business; the number
/// itself comes from `value::VALUE_SIZE_LIMIT`.
fn report_sizes() -> ExitCode {
    let limit = value::VALUE_SIZE_LIMIT;
    let n = value::value_size();
    let instr = bytecode::instr_size();
    let instr_limit = bytecode::INSTR_SIZE_LIMIT;
    println!("Value: {n} bytes (limit {limit}, ADR-025)");
    println!("Origins: {} bytes", value::origins_size());
    println!("Instr: {instr} bytes (limit {instr_limit}, ADR-034)");
    if n > limit {
        println!("VIOLATION: Value exceeds {limit} bytes");
        return ExitCode::FAILURE;
    }
    // Not packing the encoding was the decision; the assertion is what keeps
    // that from drifting into an instruction that carries a `String`.
    if instr > instr_limit {
        println!("VIOLATION: Instr exceeds {instr_limit} bytes");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
