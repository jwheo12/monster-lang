mod ast;
mod codegen_llvm;
mod lexer;
mod parser;
mod semantic;
mod token;

use codegen_llvm::emit_program as emit_llvm_program;
use lexer::Lexer;
use parser::Parser;
use semantic::analyze_program;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn main() {
    if let Err(err) = real_main() {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

fn real_main() -> Result<(), String> {
    let mut args = env::args().skip(1);

    let Some(cmd) = args.next() else {
        print!("{}", usage());
        return Ok(());
    };

    match cmd.as_str() {
        "-h" | "--help" | "help" => {
            print!("{}", usage());
            Ok(())
        }
        "-V" | "--version" | "version" => {
            println!("mst {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        "clean" => {
            if args.next().is_some() {
                return Err("too many arguments".into());
            }

            let artifact_dir = build_artifact_dir()?;
            clean_artifacts(&artifact_dir)?;
            println!("cleaned: {}", artifact_dir.display());
            Ok(())
        }
        "check" => {
            let input = single_input_arg(args)?;
            let _program = load_program(&input)?;
            println!("OK: {input}");
            Ok(())
        }
        "emit-llvm" => {
            let input = single_input_arg(args)?;
            let program = load_program(&input)?;
            let llvm_ir = emit_llvm_program(&program)?;
            print!("{llvm_ir}");
            Ok(())
        }
        "build" => {
            let input = single_input_arg(args)?;
            let output = build_to_binary(&input)?;
            println!("built: {}", output.display());
            Ok(())
        }
        "run" => {
            let input = single_input_arg(args)?;
            let output = build_to_binary(&input)?;
            let status = Command::new(&output)
                .status()
                .map_err(|e| format!("failed to run '{}': {}", output.display(), e))?;

            match status.code() {
                Some(code) => {
                    println!("\n[Monster] process exited with code {code}");
                }
                None => {
                    println!("\n[Monster] process terminated by signal");
                }
            }

            Ok(())
        }
        _ => Err(format!("unknown command: {cmd}\n{}", usage())),
    }
}

fn usage() -> String {
    [
        "Monster compiler",
        "",
        "usage:",
        "  mst check <file.mnst>",
        "  mst emit-llvm <file.mnst>",
        "  mst build <file.mnst>",
        "  mst run <file.mnst>",
        "  mst clean",
        "  mst help",
        "  mst version",
        "",
        "options:",
        "  -h, --help      show this help",
        "  -V, --version   show compiler version",
    ]
    .join("\n")
        + "\n"
}

fn single_input_arg(mut args: impl Iterator<Item = String>) -> Result<String, String> {
    let input = args.next().ok_or_else(usage)?;

    if args.next().is_some() {
        return Err("too many arguments".into());
    }

    Ok(input)
}

fn load_program(path: &str) -> Result<ast::Program, String> {
    let source =
        fs::read_to_string(path).map_err(|e| format!("failed to read '{}': {}", path, e))?;

    let mut lexer = Lexer::new(&source);
    let tokens = lexer.tokenize().map_err(|e| format!("lexer error: {e}"))?;

    let mut parser = Parser::new(tokens);
    let program = parser
        .parse_program()
        .map_err(|e| format!("parser error: {e}"))?;

    analyze_program(&program).map_err(|e| format!("semantic error: {e}"))?;

    Ok(program)
}

fn build_to_binary(input: &str) -> Result<PathBuf, String> {
    let program = load_program(input)?;
    let llvm_ir = emit_llvm_program(&program)?;

    let input_path = Path::new(input);
    let stem = input_path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| format!("invalid input filename: '{input}'"))?;

    let artifact_dir = build_artifact_dir()?;
    let ll_path = artifact_dir.join(format!("{stem}.ll"));
    let opt_ll_path = artifact_dir.join(format!("{stem}.opt.ll"));
    let out_path = artifact_dir.join(stem);

    fs::create_dir_all(&artifact_dir).map_err(|e| {
        format!(
            "failed to create build artifact directory '{}': {}",
            artifact_dir.display(),
            e
        )
    })?;

    fs::write(&ll_path, llvm_ir)
        .map_err(|e| format!("failed to write '{}': {}", ll_path.display(), e))?;

    verify_llvm_ir(&ll_path)?;
    optimize_llvm_ir(&ll_path, &opt_ll_path)?;
    verify_llvm_ir(&opt_ll_path)?;

    let clang = find_tool(&["clang-18", "clang"])
        .ok_or_else(|| "failed to find clang-18 or clang on PATH".to_string())?;

    let output = Command::new(&clang)
        .arg(&opt_ll_path)
        .arg("-o")
        .arg(&out_path)
        .output()
        .map_err(|e| format!("failed to execute {}: {}", clang, e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "{} failed while building '{}':\n{}",
            clang, input, stderr
        ));
    }

    fs::canonicalize(&out_path).map_err(|e| {
        format!(
            "built '{}', but failed to resolve output path '{}': {}",
            input,
            out_path.display(),
            e
        )
    })
}

fn build_artifact_dir() -> Result<PathBuf, String> {
    let cwd = env::current_dir().map_err(|e| format!("failed to get current directory: {e}"))?;
    Ok(cwd.join("target").join("mst"))
}

fn clean_artifacts(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }

    fs::remove_dir_all(path).map_err(|e| {
        format!(
            "failed to remove artifact directory '{}': {}",
            path.display(),
            e
        )
    })
}

fn optimize_llvm_ir(input: &Path, output: &Path) -> Result<(), String> {
    let opt = find_tool(&["opt-18", "opt"])
        .ok_or_else(|| "failed to find opt-18 or opt on PATH".to_string())?;

    let command_output = Command::new(&opt)
        .arg("-passes=default<O2>")
        .arg("-S")
        .arg(input)
        .arg("-o")
        .arg(output)
        .output()
        .map_err(|e| format!("failed to execute {}: {}", opt, e))?;

    if !command_output.status.success() {
        let stderr = String::from_utf8_lossy(&command_output.stderr);
        return Err(format!(
            "{} failed while optimizing LLVM IR '{}':\n{}",
            opt,
            input.display(),
            stderr
        ));
    }

    Ok(())
}

fn verify_llvm_ir(path: &Path) -> Result<(), String> {
    let opt = find_tool(&["opt-18", "opt"])
        .ok_or_else(|| "failed to find opt-18 or opt on PATH".to_string())?;

    let output = Command::new(&opt)
        .arg("-passes=verify")
        .arg("-disable-output")
        .arg(path)
        .output()
        .map_err(|e| format!("failed to execute {}: {}", opt, e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "{} rejected generated LLVM IR '{}':\n{}",
            opt,
            path.display(),
            stderr
        ));
    }

    Ok(())
}

fn find_tool(candidates: &[&str]) -> Option<String> {
    candidates.iter().find_map(|candidate| {
        let status = Command::new(candidate)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .ok()?;

        if status.success() {
            Some((*candidate).to_string())
        } else {
            None
        }
    })
}
