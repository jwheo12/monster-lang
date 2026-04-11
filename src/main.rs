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
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[cfg(not(windows))]
const UNIX_INSTALL_SCRIPT_URL: &str =
    "https://raw.githubusercontent.com/BitIntx/monster-lang/main/install/install-release.sh";
#[cfg(windows)]
const WINDOWS_INSTALL_SCRIPT_URL: &str =
    "https://raw.githubusercontent.com/BitIntx/monster-lang/main/install/install-release.ps1";

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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct LoadKey {
    path: PathBuf,
    namespace: Option<String>,
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
        "-upgrade" | "--upgrade" | "upgrade" => {
            if args.next().is_some() {
                return Err("too many arguments".into());
            }

            upgrade_to_latest()
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
        "  mst -upgrade",
        "  mst help",
        "  mst version",
        "",
        "options:",
        "  -h, --help      show this help",
        "  -V, --version   show compiler version",
        "  -upgrade        install the latest published release",
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

fn upgrade_to_latest() -> Result<(), String> {
    #[cfg(windows)]
    {
        upgrade_to_latest_windows()
    }

    #[cfg(not(windows))]
    {
        upgrade_to_latest_unix()
    }
}

#[cfg(not(windows))]
fn upgrade_to_latest_unix() -> Result<(), String> {
    let bin_dir = upgrade_bin_dir()?;
    let use_sudo = should_use_sudo_for_upgrade(&bin_dir);
    let command = if use_sudo {
        r#"curl -fsSL "$MST_UPGRADE_INSTALL_URL" | sudo env BIN_DIR="$MST_UPGRADE_BIN_DIR" bash"#
    } else {
        r#"curl -fsSL "$MST_UPGRADE_INSTALL_URL" | env BIN_DIR="$MST_UPGRADE_BIN_DIR" bash"#
    };

    println!("[mst] updating latest release into {}", bin_dir.display());

    if use_sudo {
        println!("[mst] global install detected; sudo may ask for your password");
    }

    let status = Command::new("bash")
        .arg("-lc")
        .arg(command)
        .env("MST_UPGRADE_INSTALL_URL", UNIX_INSTALL_SCRIPT_URL)
        .env("MST_UPGRADE_BIN_DIR", &bin_dir)
        .status()
        .map_err(|e| format!("failed to run release installer: {e}"))?;

    if !status.success() {
        return Err(format!("release installer failed with status {status}"));
    }

    Ok(())
}

#[cfg(windows)]
fn upgrade_to_latest_windows() -> Result<(), String> {
    let install_dir = upgrade_bin_dir()?;
    let command = r#"irm $env:MST_UPGRADE_INSTALL_URL | iex"#;

    println!(
        "[mst] updating latest release into {}",
        install_dir.display()
    );

    let status = Command::new("powershell")
        .arg("-NoProfile")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-Command")
        .arg(command)
        .env("MST_UPGRADE_INSTALL_URL", WINDOWS_INSTALL_SCRIPT_URL)
        .env("MST_INSTALL_DIR", &install_dir)
        .status()
        .map_err(|e| format!("failed to run PowerShell release installer: {e}"))?;

    if !status.success() {
        return Err(format!("release installer failed with status {status}"));
    }

    Ok(())
}

fn upgrade_bin_dir() -> Result<PathBuf, String> {
    if let Ok(bin_dir) = env::var("MST_UPGRADE_BIN_DIR") {
        return Ok(PathBuf::from(bin_dir));
    }

    let current_exe =
        env::current_exe().map_err(|e| format!("failed to resolve current executable: {e}"))?;

    if is_cargo_target_executable(&current_exe) {
        return Ok(default_upgrade_bin_dir());
    }

    current_exe.parent().map(Path::to_path_buf).ok_or_else(|| {
        format!(
            "failed to resolve install directory for '{}'",
            current_exe.display()
        )
    })
}

fn is_cargo_target_executable(path: &Path) -> bool {
    path.components()
        .any(|component| component.as_os_str() == "target")
}

#[cfg(not(windows))]
fn default_upgrade_bin_dir() -> PathBuf {
    PathBuf::from("/usr/local/bin")
}

#[cfg(windows)]
fn default_upgrade_bin_dir() -> PathBuf {
    env::var("LOCALAPPDATA")
        .map(|path| PathBuf::from(path).join("Programs").join("mst").join("bin"))
        .unwrap_or_else(|_| PathBuf::from(r"C:\Program Files\mst\bin"))
}

#[cfg(not(windows))]
fn should_use_sudo_for_upgrade(bin_dir: &Path) -> bool {
    if running_as_root() {
        return false;
    }

    bin_dir.starts_with("/usr") || bin_dir.starts_with("/opt") || bin_dir.starts_with("/bin")
}

#[cfg(not(windows))]
fn running_as_root() -> bool {
    Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .is_some_and(|uid| uid.trim() == "0")
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
    let root_dir = canonical
        .parent()
        .ok_or_else(|| {
            format!(
                "failed to determine parent directory of '{}'",
                canonical.display()
            )
        })?
        .to_path_buf();

    let mut loaded = HashSet::new();
    let mut active = Vec::new();
    let program = load_program_recursive(&canonical, None, &root_dir, &mut loaded, &mut active)?;

    analyze_program(&program).map_err(|e| format!("semantic error: {e}"))?;

    Ok(program)
}

fn load_program_recursive(
    path: &Path,
    namespace: Option<String>,
    root_dir: &Path,
    loaded: &mut HashSet<LoadKey>,
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

    let key = LoadKey {
        path: path.to_path_buf(),
        namespace: namespace.clone(),
    };

    if loaded.contains(&key) {
        return Ok(empty_program());
    }

    active.push(path.to_path_buf());
    let parsed = parse_program_from_file(path)?;
    loaded.insert(key);

    let mut merged = empty_program();
    let base_dir = path.parent().ok_or_else(|| {
        format!(
            "failed to determine parent directory of '{}'",
            path.display()
        )
    })?;
    let mut module_aliases = HashMap::new();
    let mut visible_imported_functions = HashMap::new();

    for import in &parsed.imports {
        let import_path = base_dir.join(&import.path);
        let canonical_import = fs::canonicalize(&import_path).map_err(|e| {
            format!(
                "failed to resolve import '{}' from '{}': {}",
                import.path,
                path.display(),
                e
            )
        })?;

        let child_namespace = match &import.alias {
            Some(_) => Some(module_name_for_path(&canonical_import, root_dir)?),
            None => namespace.clone(),
        };

        let imported = load_program_recursive(
            &canonical_import,
            child_namespace.clone(),
            root_dir,
            loaded,
            active,
        )?;

        if let Some(alias) = &import.alias {
            if let Some(child_namespace) = child_namespace {
                module_aliases.insert(alias.clone(), child_namespace);
            }
        } else {
            collect_visible_imported_functions(
                namespace.as_deref(),
                &imported.functions,
                &mut visible_imported_functions,
            );
        }

        merged.enums.extend(imported.enums);
        merged.structs.extend(imported.structs);
        merged.functions.extend(imported.functions);
    }

    let rewritten = rewrite_program(
        parsed,
        namespace.as_deref(),
        &module_aliases,
        &visible_imported_functions,
    );
    merged.enums.extend(rewritten.enums);
    merged.structs.extend(rewritten.structs);
    merged.functions.extend(rewritten.functions);

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
        enums: Vec::new(),
        structs: Vec::new(),
        functions: Vec::new(),
    }
}

fn module_name_for_path(path: &Path, root_dir: &Path) -> Result<String, String> {
    let relative = path.strip_prefix(root_dir).unwrap_or(path);
    let mut parts = Vec::new();

    for component in relative.components() {
        if let std::path::Component::Normal(segment) = component {
            let segment = segment
                .to_str()
                .ok_or_else(|| format!("non-utf8 import path '{}'", path.display()))?;
            let trimmed = segment.strip_suffix(".mnst").unwrap_or(segment);
            if !trimmed.is_empty() {
                parts.push(
                    trimmed
                        .chars()
                        .map(|ch| {
                            if ch.is_ascii_alphanumeric() || ch == '_' {
                                ch
                            } else {
                                '_'
                            }
                        })
                        .collect::<String>(),
                );
            }
        }
    }

    if parts.is_empty() {
        return Err(format!(
            "failed to derive module name from import '{}'",
            path.display()
        ));
    }

    Ok(parts.join("."))
}

fn collect_visible_imported_functions(
    namespace: Option<&str>,
    functions: &[ast::Function],
    visible: &mut HashMap<String, String>,
) {
    for function in functions {
        if let Some(simple_name) = visible_function_name(namespace, &function.name) {
            visible.insert(simple_name, function.name.clone());
        }
    }
}

fn visible_function_name(namespace: Option<&str>, canonical_name: &str) -> Option<String> {
    match namespace {
        Some(namespace) => {
            let prefix = format!("{namespace}.");
            let remainder = canonical_name.strip_prefix(&prefix)?;
            if remainder.contains('.') {
                None
            } else {
                Some(remainder.to_string())
            }
        }
        None => (!canonical_name.contains('.')).then(|| canonical_name.to_string()),
    }
}

fn rewrite_program(
    program: ast::Program,
    namespace: Option<&str>,
    module_aliases: &HashMap<String, String>,
    visible_imported_functions: &HashMap<String, String>,
) -> ast::Program {
    let mut visible_functions = visible_imported_functions.clone();
    for function in &program.functions {
        visible_functions.insert(
            function.name.clone(),
            qualify_function_name(namespace, &function.name),
        );
    }

    ast::Program {
        imports: Vec::new(),
        enums: program.enums,
        structs: program.structs,
        functions: program
            .functions
            .into_iter()
            .map(|function| rewrite_function(function, module_aliases, &visible_functions))
            .collect(),
    }
}

fn rewrite_function(
    mut function: ast::Function,
    module_aliases: &HashMap<String, String>,
    visible_functions: &HashMap<String, String>,
) -> ast::Function {
    let original_name = function.name.clone();

    if let Some(body) = function.body.take() {
        function.body = Some(
            body.into_iter()
                .map(|stmt| rewrite_stmt(stmt, module_aliases, visible_functions))
                .collect(),
        );
    }

    if let Some(canonical_name) = visible_functions.get(&original_name) {
        function.name = canonical_name.clone();
    }

    function
}

fn rewrite_stmt(
    stmt: ast::Stmt,
    module_aliases: &HashMap<String, String>,
    visible_functions: &HashMap<String, String>,
) -> ast::Stmt {
    match stmt {
        ast::Stmt::Let {
            name,
            ty,
            mutable,
            value,
        } => ast::Stmt::Let {
            name,
            ty,
            mutable,
            value: rewrite_expr(value, module_aliases, visible_functions),
        },
        ast::Stmt::Assign { name, value } => ast::Stmt::Assign {
            name,
            value: rewrite_expr(value, module_aliases, visible_functions),
        },
        ast::Stmt::AssignIndex {
            name,
            indices,
            value,
        } => ast::Stmt::AssignIndex {
            name,
            indices: indices
                .into_iter()
                .map(|expr| rewrite_expr(expr, module_aliases, visible_functions))
                .collect(),
            value: rewrite_expr(value, module_aliases, visible_functions),
        },
        ast::Stmt::AssignField {
            name,
            fields,
            value,
        } => ast::Stmt::AssignField {
            name,
            fields,
            value: rewrite_expr(value, module_aliases, visible_functions),
        },
        ast::Stmt::AssignDeref { target, value } => ast::Stmt::AssignDeref {
            target: rewrite_expr(target, module_aliases, visible_functions),
            value: rewrite_expr(value, module_aliases, visible_functions),
        },
        ast::Stmt::Expr(expr) => {
            ast::Stmt::Expr(rewrite_expr(expr, module_aliases, visible_functions))
        }
        ast::Stmt::If {
            condition,
            then_body,
            else_body,
        } => ast::Stmt::If {
            condition: rewrite_expr(condition, module_aliases, visible_functions),
            then_body: then_body
                .into_iter()
                .map(|stmt| rewrite_stmt(stmt, module_aliases, visible_functions))
                .collect(),
            else_body: else_body.map(|body| {
                body.into_iter()
                    .map(|stmt| rewrite_stmt(stmt, module_aliases, visible_functions))
                    .collect()
            }),
        },
        ast::Stmt::While { condition, body } => ast::Stmt::While {
            condition: rewrite_expr(condition, module_aliases, visible_functions),
            body: body
                .into_iter()
                .map(|stmt| rewrite_stmt(stmt, module_aliases, visible_functions))
                .collect(),
        },
        ast::Stmt::Return(expr) => ast::Stmt::Return(
            expr.map(|expr| rewrite_expr(expr, module_aliases, visible_functions)),
        ),
        ast::Stmt::Break | ast::Stmt::Continue => stmt,
    }
}

fn rewrite_expr(
    expr: ast::Expr,
    module_aliases: &HashMap<String, String>,
    visible_functions: &HashMap<String, String>,
) -> ast::Expr {
    match expr {
        ast::Expr::Match { value, arms } => ast::Expr::Match {
            value: Box::new(rewrite_expr(*value, module_aliases, visible_functions)),
            arms: arms
                .into_iter()
                .map(|arm| ast::MatchArm {
                    pattern: arm.pattern,
                    expr: rewrite_expr(arm.expr, module_aliases, visible_functions),
                })
                .collect(),
        },
        ast::Expr::ArrayLiteral(elements) => ast::Expr::ArrayLiteral(
            elements
                .into_iter()
                .map(|expr| rewrite_expr(expr, module_aliases, visible_functions))
                .collect(),
        ),
        ast::Expr::StructLiteral { name, fields } => ast::Expr::StructLiteral {
            name,
            fields: fields
                .into_iter()
                .map(|(field, expr)| (field, rewrite_expr(expr, module_aliases, visible_functions)))
                .collect(),
        },
        ast::Expr::FieldAccess { base, field } => ast::Expr::FieldAccess {
            base: Box::new(rewrite_expr(*base, module_aliases, visible_functions)),
            field,
        },
        ast::Expr::Index { base, index } => ast::Expr::Index {
            base: Box::new(rewrite_expr(*base, module_aliases, visible_functions)),
            index: Box::new(rewrite_expr(*index, module_aliases, visible_functions)),
        },
        ast::Expr::Call { name, args } => ast::Expr::Call {
            name: rewrite_call_name(&name, module_aliases, visible_functions),
            args: args
                .into_iter()
                .map(|expr| rewrite_expr(expr, module_aliases, visible_functions))
                .collect(),
        },
        ast::Expr::Binary { op, left, right } => ast::Expr::Binary {
            op,
            left: Box::new(rewrite_expr(*left, module_aliases, visible_functions)),
            right: Box::new(rewrite_expr(*right, module_aliases, visible_functions)),
        },
        ast::Expr::Unary { op, expr } => ast::Expr::Unary {
            op,
            expr: Box::new(rewrite_expr(*expr, module_aliases, visible_functions)),
        },
        ast::Expr::Cast { expr, ty } => ast::Expr::Cast {
            expr: Box::new(rewrite_expr(*expr, module_aliases, visible_functions)),
            ty,
        },
        ast::Expr::Int(_)
        | ast::Expr::Bool(_)
        | ast::Expr::Str(_)
        | ast::Expr::Var(_)
        | ast::Expr::SizeOf(_) => expr,
    }
}

fn rewrite_call_name(
    name: &str,
    module_aliases: &HashMap<String, String>,
    visible_functions: &HashMap<String, String>,
) -> String {
    if is_loader_builtin_function(name) {
        return name.to_string();
    }

    if let Some((alias, remainder)) = name.split_once('.') {
        if let Some(module_name) = module_aliases.get(alias) {
            return format!("{module_name}.{remainder}");
        }

        return name.to_string();
    }

    visible_functions
        .get(name)
        .cloned()
        .unwrap_or_else(|| name.to_string())
}

fn qualify_function_name(namespace: Option<&str>, name: &str) -> String {
    match namespace {
        Some(namespace) => format!("{namespace}.{name}"),
        None => name.to_string(),
    }
}

fn is_loader_builtin_function(name: &str) -> bool {
    matches!(
        name,
        "len"
            | "slice"
            | "is"
            | "payload"
            | "print_i32"
            | "print_bool"
            | "print_str"
            | "print_ln_i32"
            | "print_ln_bool"
            | "print_ln_str"
            | "read_i32"
            | "read_file"
            | "write_file"
            | "strlen"
            | "memcmp"
            | "memcpy"
            | "str_eq"
    )
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
    use super::{
        BuildArgs, BuildMode, Lexer, RunArgs, build_to_binary, load_program, parse_build_args,
        parse_run_args,
    };
    use crate::token::TokenKind;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::Mutex;
    use std::time::{SystemTime, UNIX_EPOCH};

    static SELFHOST_LEXER_TEST_LOCK: Mutex<()> = Mutex::new(());

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
    fn detects_cargo_target_executables_for_upgrade_defaults() {
        assert!(super::is_cargo_target_executable(Path::new(
            "/tmp/monster/target/debug/mst"
        )));
        assert!(!super::is_cargo_target_executable(Path::new(
            "/usr/local/bin/mst"
        )));
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
    fn loads_aliased_imports_with_namespaced_functions() {
        let temp_dir = unique_temp_dir("monster-modules");
        fs::create_dir_all(temp_dir.join("lib")).unwrap();

        fs::write(
            temp_dir.join("lib").join("math.mnst"),
            r#"
            fn helper(value: i32) -> i32 {
                return value + 1;
            }

            fn add(a: i32, b: i32) -> i32 {
                return helper(a + b);
            }
            "#,
        )
        .unwrap();

        fs::write(
            temp_dir.join("main.mnst"),
            r#"
            import "lib/math.mnst" as math;

            fn main() -> i32 {
                return math.add(3, 4);
            }
            "#,
        )
        .unwrap();

        let program = load_program(temp_dir.join("main.mnst").to_str().unwrap()).unwrap();
        let function_names = program
            .functions
            .iter()
            .map(|function| function.name.clone())
            .collect::<Vec<_>>();

        assert!(function_names.contains(&"lib.math.helper".to_string()));
        assert!(function_names.contains(&"lib.math.add".to_string()));
        assert!(function_names.contains(&"main".to_string()));

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

    #[test]
    fn selfhost_lexer_matches_rust_lexer_kind_sequence() {
        let _guard = SELFHOST_LEXER_TEST_LOCK.lock().unwrap();
        let temp_dir = unique_temp_dir("monster-selfhost-lexer");
        fs::create_dir_all(&temp_dir).unwrap();

        let source = r#"
        import "lib/math.mnst" as math;

        extern fn puts(text: *u8) -> i32;

        enum Value {
            Int(i32),
            Flag(bool),
            Empty,
        }

        struct Pair {
            left: i32,
            right: i32,
        }

        fn main(argc: i32, argv: **u8) -> i32 {
            // This sample intentionally touches every lexer family.
            let mut value: i32 = sizeof(Pair) as i32;
            value = value + 10 - 3 * 2 / 1;
            let ptr: *i32 = &value;

            if value >= 10 && argc != 0 {
                print_str("value\t");
            } else {
                print_ln_str("small\n");
            }

            while value > 0 {
                value = value - 1;

                if value == 2 || false {
                    continue;
                }

                if !true {
                    break;
                }
            }

            let result: i32 = match value {
                0 => 1,
                1 => 2,
                _ => 3,
            };

            return math.add(result, argv[0][0] as i32) + ptr[0];
        }
        "#;
        let input_path = temp_dir.join("sample.mnst");
        fs::write(&input_path, source).unwrap();

        let expected = rust_lexer_kind_values(source).unwrap();
        let selfhost_lexer = build_to_binary("selfhost/main.mnst", BuildMode::Release).unwrap();
        let output = Command::new(&selfhost_lexer)
            .arg(&input_path)
            .arg("--dump-kinds")
            .output()
            .unwrap();

        assert!(
            output.status.success(),
            "selfhost lexer failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );

        let stdout = String::from_utf8(output.stdout).unwrap();
        let actual = parse_selfhost_lexer_kind_values(&stdout).unwrap();
        assert_eq!(actual, expected);

        fs::remove_dir_all(temp_dir).unwrap();
    }

    fn rust_lexer_kind_values(source: &str) -> Result<Vec<i32>, String> {
        let tokens = Lexer::new(source).tokenize()?;

        Ok(tokens
            .iter()
            .map(|token| token_kind_value(&token.kind))
            .collect())
    }

    fn parse_selfhost_lexer_kind_values(stdout: &str) -> Result<Vec<i32>, String> {
        let mut lines = stdout.lines();
        let header = lines
            .next()
            .ok_or_else(|| format!("missing selfhost lexer header in output:\n{stdout}"))?;

        if header != "Monster selfhost lexer prototype" {
            return Err(format!(
                "unexpected selfhost lexer header '{header}' in output:\n{stdout}"
            ));
        }

        let _path = lines
            .next()
            .ok_or_else(|| format!("missing selfhost lexer path in output:\n{stdout}"))?;

        let kind_line = lines
            .next()
            .ok_or_else(|| format!("missing selfhost lexer token kinds in output:\n{stdout}"))?;

        kind_line
            .split_whitespace()
            .map(|value| {
                value
                    .parse::<i32>()
                    .map_err(|e| format!("invalid selfhost lexer token kind '{value}': {e}"))
            })
            .collect()
    }

    fn token_kind_value(kind: &TokenKind) -> i32 {
        match kind {
            TokenKind::Extern => 1,
            TokenKind::Import => 2,
            TokenKind::Fn => 3,
            TokenKind::Struct => 4,
            TokenKind::Enum => 5,
            TokenKind::Match => 6,
            TokenKind::SizeOf => 7,
            TokenKind::Let => 8,
            TokenKind::Mut => 9,
            TokenKind::As => 10,
            TokenKind::Return => 11,
            TokenKind::If => 12,
            TokenKind::Else => 13,
            TokenKind::While => 14,
            TokenKind::Break => 15,
            TokenKind::Continue => 16,
            TokenKind::True => 17,
            TokenKind::False => 18,
            TokenKind::Arrow => 19,
            TokenKind::Dot => 20,
            TokenKind::Colon => 21,
            TokenKind::Comma => 22,
            TokenKind::Semicolon => 23,
            TokenKind::LBracket => 24,
            TokenKind::RBracket => 25,
            TokenKind::LParen => 26,
            TokenKind::RParen => 27,
            TokenKind::LBrace => 28,
            TokenKind::RBrace => 29,
            TokenKind::Equal => 30,
            TokenKind::FatArrow => 31,
            TokenKind::Bang => 32,
            TokenKind::Amp => 33,
            TokenKind::EqualEqual => 34,
            TokenKind::BangEqual => 35,
            TokenKind::AndAnd => 36,
            TokenKind::OrOr => 37,
            TokenKind::Plus => 38,
            TokenKind::Minus => 39,
            TokenKind::Star => 40,
            TokenKind::Slash => 41,
            TokenKind::Less => 42,
            TokenKind::LessEqual => 43,
            TokenKind::Greater => 44,
            TokenKind::GreaterEqual => 45,
            TokenKind::Ident => 46,
            TokenKind::Int => 47,
            TokenKind::Str => 48,
            TokenKind::Eof => 49,
        }
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{}-{nanos}", std::process::id()))
    }
}
