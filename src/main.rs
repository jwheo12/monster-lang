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
use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BuildMode {
    Release,
    Debug,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BuildArgs {
    input: String,
    mode: BuildMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RunArgs {
    build: BuildArgs,
    program_args: Vec<String>,
}

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
            let build_args = parse_build_args(args)?;
            let output = build_to_binary(&build_args.input, build_args.mode)?;
            println!("built: {}", output.display());
            Ok(())
        }
        "run" => {
            let run_args = parse_run_args(args)?;
            let output = build_to_binary(&run_args.build.input, run_args.build.mode)?;
            let status = Command::new(&output)
                .args(&run_args.program_args)
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
        "  mst build --debug <file.mnst>",
        "  mst run <file.mnst> [-- <args...>]",
        "  mst run --debug <file.mnst> [-- <args...>]",
        "  mst clean",
        "  mst help",
        "  mst version",
        "",
        "options:",
        "  -h, --help      show this help",
        "  -V, --version   show compiler version",
        "  --debug         build without LLVM -O2 and link with clang -g -O0",
        "  --              pass remaining arguments to the compiled program",
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

fn parse_build_args(args: impl Iterator<Item = String>) -> Result<BuildArgs, String> {
    let mut input = None;
    let mut mode = BuildMode::Release;

    for arg in args {
        match arg.as_str() {
            "--debug" => mode = BuildMode::Debug,
            _ if arg.starts_with('-') => return Err(format!("unknown option: {arg}")),
            _ => {
                if input.is_some() {
                    return Err("too many arguments".into());
                }
                input = Some(arg);
            }
        }
    }

    let input = input.ok_or_else(usage)?;
    Ok(BuildArgs { input, mode })
}

fn parse_run_args(args: impl Iterator<Item = String>) -> Result<RunArgs, String> {
    let mut input = None;
    let mut mode = BuildMode::Release;
    let mut program_args = Vec::new();
    let mut forwarding = false;

    for arg in args {
        if forwarding {
            program_args.push(arg);
            continue;
        }

        match arg.as_str() {
            "--" => forwarding = true,
            "--debug" => mode = BuildMode::Debug,
            _ if arg.starts_with('-') && input.is_none() => {
                return Err(format!("unknown option: {arg}"));
            }
            _ if input.is_none() => input = Some(arg),
            _ => program_args.push(arg),
        }
    }

    let input = input.ok_or_else(usage)?;
    Ok(RunArgs {
        build: BuildArgs { input, mode },
        program_args,
    })
}

fn load_program(path: &str) -> Result<ast::Program, String> {
    let canonical =
        fs::canonicalize(path).map_err(|e| format!("failed to resolve '{}': {}", path, e))?;

    let mut loaded = HashSet::new();
    let mut active = Vec::new();
    let program = load_program_recursive(&canonical, &mut loaded, &mut active)?;

    analyze_program(&program).map_err(|e| format!("semantic error: {e}"))?;

    Ok(program)
}

fn load_program_recursive(
    path: &Path,
    loaded: &mut HashSet<PathBuf>,
    active: &mut Vec<PathBuf>,
) -> Result<ast::Program, String> {
    if let Some(index) = active.iter().position(|current| current == path) {
        let mut cycle = active[index..]
            .iter()
            .map(|item| item.display().to_string())
            .collect::<Vec<_>>();
        cycle.push(path.display().to_string());
        return Err(format!("import cycle detected: {}", cycle.join(" -> ")));
    }

    if loaded.contains(path) {
        return Ok(empty_program());
    }

    active.push(path.to_path_buf());
    let parsed = parse_program_from_file(path)?;
    loaded.insert(path.to_path_buf());

    let mut merged = empty_program();
    let base_dir = path.parent().ok_or_else(|| {
        format!(
            "failed to determine parent directory of '{}'",
            path.display()
        )
    })?;

    for import in &parsed.imports {
        let import_path = base_dir.join(import);
        let canonical_import = fs::canonicalize(&import_path).map_err(|e| {
            format!(
                "failed to resolve import '{}' from '{}': {}",
                import,
                path.display(),
                e
            )
        })?;

        let imported = load_program_recursive(&canonical_import, loaded, active)?;
        merged.structs.extend(imported.structs);
        merged.functions.extend(imported.functions);
    }

    merged.structs.extend(parsed.structs);
    merged.functions.extend(parsed.functions);

    active.pop();
    Ok(merged)
}

fn parse_program_from_file(path: &Path) -> Result<ast::Program, String> {
    let source = fs::read_to_string(path)
        .map_err(|e| format!("failed to read '{}': {}", path.display(), e))?;

    let mut lexer = Lexer::new(&source);
    let tokens = lexer.tokenize().map_err(|e| format!("lexer error: {e}"))?;

    let mut parser = Parser::new(tokens);
    parser
        .parse_program()
        .map_err(|e| format!("parser error in '{}': {e}", path.display()))
}

fn empty_program() -> ast::Program {
    ast::Program {
        imports: Vec::new(),
        structs: Vec::new(),
        functions: Vec::new(),
    }
}

fn build_to_binary(input: &str, mode: BuildMode) -> Result<PathBuf, String> {
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

    let compile_input = match mode {
        BuildMode::Release => {
            optimize_llvm_ir(&ll_path, &opt_ll_path)?;
            verify_llvm_ir(&opt_ll_path)?;
            opt_ll_path.as_path()
        }
        BuildMode::Debug => {
            if opt_ll_path.exists() {
                fs::remove_file(&opt_ll_path).map_err(|e| {
                    format!(
                        "failed to remove stale optimized LLVM IR '{}': {}",
                        opt_ll_path.display(),
                        e
                    )
                })?;
            }
            ll_path.as_path()
        }
    };

    compile_to_native(input, compile_input, &out_path, mode)?;

    fs::canonicalize(&out_path).map_err(|e| {
        format!(
            "built '{}', but failed to resolve output path '{}': {}",
            input,
            out_path.display(),
            e
        )
    })
}

fn compile_to_native(
    input: &str,
    llvm_input: &Path,
    output_path: &Path,
    mode: BuildMode,
) -> Result<(), String> {
    let clang = find_tool(&["clang-18", "clang"])
        .ok_or_else(|| "failed to find clang-18 or clang on PATH".to_string())?;

    let mut command = Command::new(&clang);
    command.arg(llvm_input);

    if mode == BuildMode::Debug {
        command.arg("-g").arg("-O0");
    }

    let output = command
        .arg("-o")
        .arg(output_path)
        .output()
        .map_err(|e| format!("failed to execute {}: {}", clang, e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "{} failed while building '{}':\n{}",
            clang, input, stderr
        ));
    }

    Ok(())
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

#[cfg(test)]
mod tests {
    use super::{BuildArgs, BuildMode, RunArgs, load_program, parse_build_args, parse_run_args};
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn parses_release_build_args() {
        let parsed = parse_build_args(vec!["exam.mnst".to_string()].into_iter()).unwrap();
        assert_eq!(
            parsed,
            BuildArgs {
                input: "exam.mnst".to_string(),
                mode: BuildMode::Release,
            }
        );
    }

    #[test]
    fn parses_debug_build_args_before_input() {
        let parsed =
            parse_build_args(vec!["--debug".to_string(), "exam.mnst".to_string()].into_iter())
                .unwrap();
        assert_eq!(
            parsed,
            BuildArgs {
                input: "exam.mnst".to_string(),
                mode: BuildMode::Debug,
            }
        );
    }

    #[test]
    fn parses_debug_build_args_after_input() {
        let parsed =
            parse_build_args(vec!["exam.mnst".to_string(), "--debug".to_string()].into_iter())
                .unwrap();
        assert_eq!(
            parsed,
            BuildArgs {
                input: "exam.mnst".to_string(),
                mode: BuildMode::Debug,
            }
        );
    }

    #[test]
    fn rejects_unknown_build_option() {
        let err = parse_build_args(vec!["--wat".to_string(), "exam.mnst".to_string()].into_iter())
            .unwrap_err();
        assert!(err.contains("unknown option"));
    }

    #[test]
    fn parses_run_args_without_forwarded_program_args() {
        let parsed = parse_run_args(vec!["exam.mnst".to_string()].into_iter()).unwrap();
        assert_eq!(
            parsed,
            RunArgs {
                build: BuildArgs {
                    input: "exam.mnst".to_string(),
                    mode: BuildMode::Release,
                },
                program_args: Vec::new(),
            }
        );
    }

    #[test]
    fn parses_run_args_with_debug_and_forwarded_program_args() {
        let parsed = parse_run_args(
            vec![
                "--debug".to_string(),
                "exam.mnst".to_string(),
                "--".to_string(),
                "alpha".to_string(),
                "--flag".to_string(),
            ]
            .into_iter(),
        )
        .unwrap();

        assert_eq!(
            parsed,
            RunArgs {
                build: BuildArgs {
                    input: "exam.mnst".to_string(),
                    mode: BuildMode::Debug,
                },
                program_args: vec!["alpha".to_string(), "--flag".to_string()],
            }
        );
    }

    #[test]
    fn loads_relative_imports_recursively() {
        let temp_dir = unique_temp_dir("monster-imports");
        fs::create_dir_all(temp_dir.join("lib")).unwrap();

        fs::write(
            temp_dir.join("lib").join("math.mnst"),
            r#"
            fn add(a: i32, b: i32) -> i32 {
                return a + b;
            }
            "#,
        )
        .unwrap();

        fs::write(
            temp_dir.join("main.mnst"),
            r#"
            import "lib/math.mnst";

            fn main() -> i32 {
                return add(3, 4);
            }
            "#,
        )
        .unwrap();

        let program = load_program(temp_dir.join("main.mnst").to_str().unwrap()).unwrap();
        assert_eq!(program.imports.len(), 0);
        assert_eq!(program.functions.len(), 2);
        assert_eq!(program.functions[0].name, "add");
        assert_eq!(program.functions[1].name, "main");

        fs::remove_dir_all(temp_dir).unwrap();
    }

    #[test]
    fn rejects_import_cycles() {
        let temp_dir = unique_temp_dir("monster-cycle");
        fs::create_dir_all(temp_dir.join("lib")).unwrap();

        fs::write(
            temp_dir.join("main.mnst"),
            r#"
            import "lib/loop.mnst";

            fn main() -> i32 {
                return 0;
            }
            "#,
        )
        .unwrap();

        fs::write(
            temp_dir.join("lib").join("loop.mnst"),
            r#"
            import "../main.mnst";
            "#,
        )
        .unwrap();

        let err = load_program(temp_dir.join("main.mnst").to_str().unwrap()).unwrap_err();
        assert!(err.contains("import cycle detected"));

        fs::remove_dir_all(temp_dir).unwrap();
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{}-{nanos}", std::process::id()))
    }
}
