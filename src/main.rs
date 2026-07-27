//! The process driver: argument handling, file I/O, stdout, exit codes.
//!
//! Language behaviour lives in `lib.rs` (ADR-031). Nothing here decides
//! anything about the language — if a change to this file changes what a
//! program means, it is in the wrong file.

use apolisp::session::{self, Ended, Session};
use apolisp::{bytecode, compile, expand, printer, reader, value, vm};
use std::io::Write;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: apolisp <read|spans|sizes|repl|expand|compile|run> [file.xs]");
        return ExitCode::from(2);
    }
    if args[1] == "sizes" {
        return report_sizes();
    }
    // Milestone 9. The only command with no file: a session's input is the
    // session, and there is nothing to read off disk (ADR-044 part 1).
    if args[1] == "repl" {
        return repl();
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
        // Milestone 5: expansion, printed as forms exactly as `read` prints
        // them. What each phase did is then a diff between two goldens.
        "expand" => {
            let mut vm = vm::Vm::new();
            match pipeline(&src, path, &mut vm) {
                Ok(forms) => {
                    for f in &forms {
                        println!("{}", printer::print(&f.root, &vm.interner));
                    }
                    ExitCode::SUCCESS
                }
                Err(code) => code,
            }
        }
        // Milestone 2, and since milestone 5 the compiler is downstream of a
        // VM: macros are language code, so compilation is not a function of
        // source alone (ADR-004 said so before there was any).
        "compile" => {
            let mut vm = vm::Vm::new();
            let forms = match pipeline(&src, path, &mut vm) {
                Ok(forms) => forms,
                Err(code) => return code,
            };
            let interner = &mut vm.interner;
            match compile::compile(&forms, interner) {
                Ok(chunk) => {
                    print!("{}", bytecode::disassemble(&chunk, interner, &src));
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("{}", e.render(path, &src));
                    ExitCode::FAILURE
                }
            }
        }
        // Milestone 3. The `.out` transcript is canonical rather than raw
        // stdout (BUILD.md): a failure with no defined record is a failure that
        // cannot be pinned, and milestone 4 adds the thrown-value half.
        "run" => {
            let mut vm = vm::Vm::new();
            let forms = match pipeline(&src, path, &mut vm) {
                Ok(forms) => forms,
                Err(code) => return code,
            };
            let chunk = match compile::compile(&forms, &mut vm.interner) {
                Ok(chunk) => chunk,
                Err(e) => {
                    eprintln!("{}", e.render(path, &src));
                    return ExitCode::FAILURE;
                }
            };
            let outcome = vm::run(&mut vm, &chunk);
            let stdout = vm.take_output();
            print!("--- stdout\n{stdout}");
            match outcome {
                // The driver runs un-fuelled. Suspension is a library facility
                // for the round-trip property (ADR-043); giving the CLI a fuel
                // flag would make `.out` depend on a step limit, and a golden
                // that changes with a step limit is not a golden.
                vm::Outcome::Suspended => unreachable!("`run` is un-fuelled"),
                vm::Outcome::Returned(v) => {
                    println!("--- value\n{}", printer::print(&v, &vm.interner));
                    println!("--- exit\n0");
                    ExitCode::SUCCESS
                }
                // ADR-039: a VM fault is a throw, so there is one section for
                // both. Position and the suppressed chain travel beside the
                // value rather than inside it, so they print as their own
                // sections — and a section is absent rather than empty when
                // there is nothing to say.
                vm::Outcome::Threw(u) => {
                    println!("--- threw\n{}", printer::print(&u.value, &vm.interner));
                    if let Some(at) = u.position(path, &src) {
                        println!("--- at\n{at}");
                    }
                    if !u.suppressed.is_empty() {
                        println!("--- suppressed");
                        for v in &u.suppressed {
                            println!("{}", printer::print(v, &vm.interner));
                        }
                    }
                    println!("--- exit\n1");
                    ExitCode::FAILURE
                }
            }
        }
        _ => {
            eprintln!("apolisp: unknown command `{cmd}`");
            ExitCode::from(2)
        }
    }
}

/// The prompt. Everything about *what an input means* is in `session`; this
/// function owns stdin, the two prompts, and when to stop (ADR-031).
///
/// A thrown value does not end the session. ADR-039 gives the VM one failure
/// path that leaves the machine usable afterwards, and ADR-044 part 5 spends
/// that guarantee here rather than re-earning it: a REPL that died on a typo is
/// not a development interface.
fn repl() -> ExitCode {
    let mut s = Session::new();
    let mut buffered = String::new();
    loop {
        // `> ` for a new form, `  ` for a continuation, so a half-typed form is
        // visible as one. Flushed explicitly because a prompt has no newline
        // and would otherwise sit in the buffer until after the answer.
        print!("{}", if buffered.is_empty() { "> " } else { "  " });
        if std::io::stdout().flush().is_err() {
            return ExitCode::FAILURE;
        }
        let mut line = String::new();
        match std::io::stdin().read_line(&mut line) {
            // End of input. A form still open at EOF is abandoned rather than
            // guessed at.
            Ok(0) => {
                println!();
                return ExitCode::SUCCESS;
            }
            Ok(_) => {}
            Err(e) => {
                eprintln!("apolisp: {e}");
                return ExitCode::FAILURE;
            }
        }
        buffered.push_str(&line);
        if buffered.trim().is_empty() {
            buffered.clear();
            continue;
        }
        // Keep typing only when the reader says the input stopped *mid-form*.
        // A genuine syntax error falls through to `eval`, which reports it —
        // waiting for more input on a malformed line would hang the prompt on a
        // typo (ADR-044 part 5).
        if session::wants_more(&buffered) {
            continue;
        }
        let src = std::mem::take(&mut buffered);
        match s.eval(&src) {
            Ok(ended) => {
                print!("{}", s.take_output());
                match ended {
                    Ended::Value(v) => println!("{}", printer::print(&v, &s.vm.interner)),
                    // The same sections a `.out` transcript uses, minus the
                    // exit status — there is no exit, which is the point.
                    Ended::Threw(u) => {
                        println!("--- threw\n{}", printer::print(&u.value, &s.vm.interner));
                        for v in &u.suppressed {
                            println!("--- suppressed\n{}", printer::print(v, &s.vm.interner));
                        }
                    }
                }
            }
            // `<repl>` rather than a path: the position is real, and the file
            // it points into is the line just typed.
            Err(e) => println!("{}", e.render("<repl>", &src)),
        }
    }
}

/// Read and expand, which is what every stage from `expand` on starts with.
/// Errors are rendered here so the three stages report them identically.
fn pipeline(src: &str, path: &str, vm: &mut vm::Vm) -> Result<Vec<value::LocatedForm>, ExitCode> {
    let forms = reader::read_all(src, &mut vm.interner).map_err(|e| {
        eprintln!("{}", e.render(path, src));
        ExitCode::FAILURE
    })?;
    expand::expand_all(forms, vm).map_err(|e| {
        eprintln!("{}", e.render(path, src));
        ExitCode::FAILURE
    })
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
