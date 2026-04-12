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
    input: Option<String>,
    overrides: BuildOptionOverrides,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedBuildArgs {
    input: String,
    options: BuildOptions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OptLevel {
    O0,
    O1,
    O2,
    O3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TargetCpu {
    Generic,
    Native,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BuildOptions {
    mode: BuildMode,
    opt_level: OptLevel,
    cpu: TargetCpu,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct BuildOptionOverrides {
    mode: Option<BuildMode>,
    opt_level: Option<OptLevel>,
    cpu: Option<TargetCpu>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RunArgs {
    build: BuildArgs,
    program_args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct InitArgs {
    path: PathBuf,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ProjectConfig {
    package: PackageConfig,
    build: BuildConfig,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct PackageConfig {
    name: Option<String>,
    entry: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct BuildConfig {
    mode: Option<BuildMode>,
    opt_level: Option<OptLevel>,
    cpu: Option<TargetCpu>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct LoadKey {
    path: PathBuf,
    namespace: Option<String>,
}

impl BuildMode {
    fn default_opt_level(self) -> OptLevel {
        match self {
            BuildMode::Release => OptLevel::O2,
            BuildMode::Debug => OptLevel::O0,
        }
    }
}

impl OptLevel {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "0" => Ok(Self::O0),
            "1" => Ok(Self::O1),
            "2" => Ok(Self::O2),
            "3" => Ok(Self::O3),
            _ => Err(format!(
                "invalid opt level '{value}', expected 0, 1, 2, or 3"
            )),
        }
    }

    fn as_u8(self) -> u8 {
        match self {
            Self::O0 => 0,
            Self::O1 => 1,
            Self::O2 => 2,
            Self::O3 => 3,
        }
    }

    fn clang_arg(self) -> String {
        format!("-O{}", self.as_u8())
    }

    fn is_optimizing(self) -> bool {
        self != Self::O0
    }
}

impl TargetCpu {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "generic" => Ok(Self::Generic),
            "native" => Ok(Self::Native),
            _ => Err(format!(
                "invalid cpu target '{value}', expected 'generic' or 'native'"
            )),
        }
    }
}

impl Default for BuildOptions {
    fn default() -> Self {
        Self {
            mode: BuildMode::Release,
            opt_level: BuildMode::Release.default_opt_level(),
            cpu: TargetCpu::Generic,
        }
    }
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
        "init" => {
            let init_args = parse_init_args(args)?;
            init_project(&init_args)
        }
        "check" => {
            let input = resolve_input_arg(optional_input_arg(args)?)?;
            let _program = load_program(&input)?;
            println!("OK: {input}");
            Ok(())
        }
        "emit-llvm" => {
            let input = resolve_input_arg(optional_input_arg(args)?)?;
            let program = load_program(&input)?;
            let llvm_ir = emit_llvm_program(&program)?;
            print!("{llvm_ir}");
            Ok(())
        }
        "build" => {
            let build_args = parse_build_args(args)?;
            let resolved = resolve_build_args(build_args)?;
            let output = build_to_binary(&resolved.input, &resolved.options)?;
            println!("built: {}", output.display());
            Ok(())
        }
        "run" => {
            let run_args = parse_run_args(args)?;
            let resolved = resolve_build_args(run_args.build)?;
            let output = build_to_binary(&resolved.input, &resolved.options)?;
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
        "  mst init [path]",
        "  mst check [file.mnst]",
        "  mst emit-llvm [file.mnst]",
        "  mst build [file.mnst]",
        "  mst build --debug [file.mnst]",
        "  mst build --opt-level 3 --cpu native [file.mnst]",
        "  mst run [file.mnst] [-- <args...>]",
        "  mst run --debug [file.mnst] [-- <args...>]",
        "  mst clean",
        "  mst -upgrade",
        "  mst help",
        "  mst version",
        "",
        "options:",
        "  -h, --help      show this help",
        "  -V, --version   show compiler version",
        "  -upgrade        install the latest published release",
        "  --debug         debug build profile, defaults to opt-level 0 and clang -g",
        "  --release       release build profile, defaults to opt-level 2",
        "  --opt-level N   optimize with level 0, 1, 2, or 3",
        "  --cpu TARGET    use 'generic' or 'native' CPU codegen",
        "  --              pass remaining arguments to the compiled program",
    ]
    .join("\n")
        + "\n"
}

fn optional_input_arg(mut args: impl Iterator<Item = String>) -> Result<Option<String>, String> {
    let input = args.next();

    if args.next().is_some() {
        return Err("too many arguments".into());
    }

    Ok(input)
}

fn resolve_input_arg(input: Option<String>) -> Result<String, String> {
    Ok(resolve_build_args(BuildArgs {
        input,
        overrides: BuildOptionOverrides::default(),
    })?
    .input)
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

fn parse_init_args(args: impl Iterator<Item = String>) -> Result<InitArgs, String> {
    let mut path = None;

    for arg in args {
        if arg.starts_with('-') {
            return Err(format!("unknown option: {arg}"));
        }

        if path.is_some() {
            return Err("too many arguments".into());
        }

        path = Some(PathBuf::from(arg));
    }

    Ok(InitArgs {
        path: path.unwrap_or_else(|| PathBuf::from(".")),
    })
}

fn parse_build_args(args: impl Iterator<Item = String>) -> Result<BuildArgs, String> {
    let mut input = None;
    let mut overrides = BuildOptionOverrides::default();
    let mut args = args.peekable();

    while let Some(arg) = args.next() {
        if parse_build_option(arg.as_str(), &mut args, &mut overrides)? {
            continue;
        }

        if arg.starts_with('-') {
            return Err(format!("unknown option: {arg}"));
        }

        if input.is_some() {
            return Err("too many arguments".into());
        }

        input = Some(arg);
    }

    Ok(BuildArgs { input, overrides })
}

fn parse_run_args(args: impl Iterator<Item = String>) -> Result<RunArgs, String> {
    let mut input = None;
    let mut overrides = BuildOptionOverrides::default();
    let mut program_args = Vec::new();
    let mut forwarding = false;
    let mut args = args.peekable();

    while let Some(arg) = args.next() {
        if forwarding {
            program_args.push(arg);
            continue;
        }

        if arg == "--" {
            forwarding = true;
            continue;
        }

        if parse_build_option(arg.as_str(), &mut args, &mut overrides)? {
            continue;
        }

        match arg.as_str() {
            _ if arg.starts_with('-') && input.is_none() => {
                return Err(format!("unknown option: {arg}"));
            }
            _ if input.is_none() => input = Some(arg),
            _ => program_args.push(arg),
        }
    }

    Ok(RunArgs {
        build: BuildArgs { input, overrides },
        program_args,
    })
}

fn parse_build_option(
    arg: &str,
    args: &mut impl Iterator<Item = String>,
    overrides: &mut BuildOptionOverrides,
) -> Result<bool, String> {
    match arg {
        "--debug" => {
            overrides.mode = Some(BuildMode::Debug);
            Ok(true)
        }
        "--release" => {
            overrides.mode = Some(BuildMode::Release);
            Ok(true)
        }
        "--opt-level" => {
            let value = args
                .next()
                .ok_or_else(|| "--opt-level expects 0, 1, 2, or 3".to_string())?;
            overrides.opt_level = Some(OptLevel::parse(&value)?);
            Ok(true)
        }
        "--cpu" => {
            let value = args
                .next()
                .ok_or_else(|| "--cpu expects 'generic' or 'native'".to_string())?;
            overrides.cpu = Some(TargetCpu::parse(&value)?);
            Ok(true)
        }
        _ => {
            if let Some(value) = arg.strip_prefix("--opt-level=") {
                overrides.opt_level = Some(OptLevel::parse(value)?);
                Ok(true)
            } else if let Some(value) = arg.strip_prefix("--cpu=") {
                overrides.cpu = Some(TargetCpu::parse(value)?);
                Ok(true)
            } else {
                Ok(false)
            }
        }
    }
}

fn resolve_build_args(args: BuildArgs) -> Result<ResolvedBuildArgs, String> {
    let manifest_start = match &args.input {
        Some(input) => manifest_start_for_input(input)?,
        None => env::current_dir().map_err(|e| format!("failed to get current directory: {e}"))?,
    };
    let manifest_path = find_project_manifest(&manifest_start);
    let config = match &manifest_path {
        Some(path) => load_project_config(path)?,
        None => ProjectConfig::default(),
    };

    let input = match args.input {
        Some(input) => input,
        None => {
            let entry = config.package.entry.as_deref().ok_or_else(|| {
                "missing input file and no Monster.toml entry found; pass <file.mnst> or run mst init"
                    .to_string()
            })?;
            let manifest_dir = manifest_path
                .as_ref()
                .and_then(|path| path.parent())
                .ok_or_else(|| "internal error: project manifest has no parent".to_string())?;
            manifest_dir.join(entry).to_string_lossy().into_owned()
        }
    };

    Ok(ResolvedBuildArgs {
        input,
        options: resolve_build_options(&config.build, &args.overrides),
    })
}

fn resolve_build_options(config: &BuildConfig, overrides: &BuildOptionOverrides) -> BuildOptions {
    let mut options = BuildOptions::default();

    if let Some(mode) = config.mode {
        options.mode = mode;
        if config.opt_level.is_none() {
            options.opt_level = mode.default_opt_level();
        }
    }

    if let Some(opt_level) = config.opt_level {
        options.opt_level = opt_level;
    }

    if let Some(cpu) = config.cpu {
        options.cpu = cpu;
    }

    if let Some(mode) = overrides.mode {
        options.mode = mode;
        if overrides.opt_level.is_none() {
            options.opt_level = mode.default_opt_level();
        }
    }

    if let Some(opt_level) = overrides.opt_level {
        options.opt_level = opt_level;
    }

    if let Some(cpu) = overrides.cpu {
        options.cpu = cpu;
    }

    options
}

fn manifest_start_for_input(input: &str) -> Result<PathBuf, String> {
    let path = Path::new(input);
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()
            .map_err(|e| format!("failed to get current directory: {e}"))?
            .join(path)
    };

    Ok(absolute
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(".")))
}

fn find_project_manifest(start: &Path) -> Option<PathBuf> {
    let mut current = start.to_path_buf();

    loop {
        for name in ["Monster.toml", "monster.toml"] {
            let candidate = current.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }

        if !current.pop() {
            return None;
        }
    }
}

fn load_project_config(path: &Path) -> Result<ProjectConfig, String> {
    let source = fs::read_to_string(path)
        .map_err(|e| format!("failed to read project config '{}': {}", path.display(), e))?;
    parse_project_config(&source, path)
}

fn parse_project_config(source: &str, path: &Path) -> Result<ProjectConfig, String> {
    let mut config = ProjectConfig::default();
    let mut section = String::new();

    for (line_index, raw_line) in source.lines().enumerate() {
        let line_no = line_index + 1;
        let uncommented = strip_toml_comment(raw_line);
        let line = uncommented.trim();

        if line.is_empty() {
            continue;
        }

        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].trim().to_string();
            continue;
        }

        let Some((key, value)) = line.split_once('=') else {
            return Err(format!(
                "invalid project config '{}': line {} expected key = value",
                path.display(),
                line_no
            ));
        };
        let key = key.trim();
        let value = value.trim();

        match (section.as_str(), key) {
            ("package", "name") => {
                config.package.name = Some(parse_toml_string(value, path, line_no)?);
            }
            ("package", "entry") => {
                config.package.entry = Some(parse_toml_string(value, path, line_no)?);
            }
            ("build", "profile") | ("build", "mode") => {
                let value = parse_toml_string(value, path, line_no)?;
                config.build.mode = Some(parse_build_mode_value(&value, path, line_no)?);
            }
            ("build", "opt-level") | ("build", "opt_level") => {
                config.build.opt_level =
                    Some(OptLevel::parse(parse_toml_integer(value)?.as_str())?);
            }
            ("build", "cpu") => {
                let value = parse_toml_string(value, path, line_no)?;
                config.build.cpu = Some(TargetCpu::parse(&value)?);
            }
            _ => {}
        }
    }

    Ok(config)
}

fn strip_toml_comment(line: &str) -> String {
    let mut escaped = false;
    let mut in_string = false;

    for (index, ch) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }

        match ch {
            '\\' if in_string => escaped = true,
            '"' => in_string = !in_string,
            '#' if !in_string => return line[..index].to_string(),
            _ => {}
        }
    }

    line.to_string()
}

fn parse_toml_string(value: &str, path: &Path, line_no: usize) -> Result<String, String> {
    if !(value.starts_with('"') && value.ends_with('"')) || value.len() < 2 {
        return Err(format!(
            "invalid project config '{}': line {} expected a quoted string",
            path.display(),
            line_no
        ));
    }

    let mut out = String::new();
    let mut chars = value[1..value.len() - 1].chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }

        let escaped = chars.next().ok_or_else(|| {
            format!(
                "invalid project config '{}': line {} has dangling escape",
                path.display(),
                line_no
            )
        })?;

        match escaped {
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            '"' => out.push('"'),
            '\\' => out.push('\\'),
            _ => {
                return Err(format!(
                    "invalid project config '{}': line {} has unsupported escape '\\{}'",
                    path.display(),
                    line_no,
                    escaped
                ));
            }
        }
    }

    Ok(out)
}

fn parse_toml_integer(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.chars().all(|ch| ch.is_ascii_digit()) && !trimmed.is_empty() {
        Ok(trimmed.to_string())
    } else {
        Err(format!("expected integer value, found '{value}'"))
    }
}

fn parse_build_mode_value(value: &str, path: &Path, line_no: usize) -> Result<BuildMode, String> {
    match value {
        "release" => Ok(BuildMode::Release),
        "debug" => Ok(BuildMode::Debug),
        _ => Err(format!(
            "invalid project config '{}': line {} expected profile 'release' or 'debug'",
            path.display(),
            line_no
        )),
    }
}

fn init_project(args: &InitArgs) -> Result<(), String> {
    let root = &args.path;

    if root.exists() && !root.is_dir() {
        return Err(format!(
            "'{}' exists but is not a directory",
            root.display()
        ));
    }

    fs::create_dir_all(root).map_err(|e| {
        format!(
            "failed to create project directory '{}': {}",
            root.display(),
            e
        )
    })?;

    let name = project_name_for_init(root)?;
    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir).map_err(|e| {
        format!(
            "failed to create source directory '{}': {}",
            src_dir.display(),
            e
        )
    })?;

    write_new_file(
        &root.join("Monster.toml"),
        &format!(
            r#"[package]
name = "{name}"
entry = "src/main.mnst"

[build]
profile = "release"
opt-level = 2
cpu = "generic"
"#
        ),
    )?;

    write_new_file(
        &src_dir.join("main.mnst"),
        r#"fn main() -> i32 {
    print_ln_str("Hello, Monster!");
    return 0;
}
"#,
    )?;

    let gitignore = root.join(".gitignore");
    if !gitignore.exists() {
        fs::write(&gitignore, "target/\n")
            .map_err(|e| format!("failed to write '{}': {}", gitignore.display(), e))?;
    }

    println!("created Monster project: {}", root.display());
    if root == Path::new(".") {
        println!("try: mst run");
    } else {
        println!("try: cd {} && mst run", root.display());
    }

    Ok(())
}

fn project_name_for_init(root: &Path) -> Result<String, String> {
    let source = if root == Path::new(".") {
        env::current_dir()
            .map_err(|e| format!("failed to get current directory: {e}"))?
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("monster-app")
            .to_string()
    } else {
        root.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("monster-app")
            .to_string()
    };

    let sanitized = source
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();

    Ok(if sanitized.is_empty() {
        "monster-app".to_string()
    } else {
        sanitized
    })
}

fn write_new_file(path: &Path, contents: &str) -> Result<(), String> {
    if path.exists() {
        return Err(format!(
            "'{}' already exists; refusing to overwrite it",
            path.display()
        ));
    }

    fs::write(path, contents).map_err(|e| format!("failed to write '{}': {}", path.display(), e))
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
        let canonical_import = resolve_import_path(&import.path, base_dir, path)?;

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

        merged.consts.extend(imported.consts);
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
    merged.consts.extend(rewritten.consts);
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

fn resolve_import_path(
    import_path: &str,
    base_dir: &Path,
    importer: &Path,
) -> Result<PathBuf, String> {
    let relative_candidate = base_dir.join(import_path);
    let mut searched = vec![relative_candidate.clone()];

    if let Ok(canonical) = fs::canonicalize(&relative_candidate) {
        return Ok(canonical);
    }

    if let Some(std_relative_path) = std_import_relative_path(import_path) {
        for std_root in std_search_roots() {
            let candidate = std_root.join(&std_relative_path);
            searched.push(candidate.clone());

            if let Ok(canonical) = fs::canonicalize(&candidate) {
                return Ok(canonical);
            }
        }
    }

    let searched_paths = searched
        .iter()
        .map(|path| format!("'{}'", path.display()))
        .collect::<Vec<_>>()
        .join(", ");

    Err(format!(
        "failed to resolve import '{}' from '{}'; searched {}",
        import_path,
        importer.display(),
        searched_paths
    ))
}

fn std_import_relative_path(import_path: &str) -> Option<PathBuf> {
    let mut components = Path::new(import_path).components();
    let first = components.next()?;

    if !matches!(
        first,
        std::path::Component::Normal(segment) if segment.to_str() == Some("std")
    ) {
        return None;
    }

    let relative = components.collect::<PathBuf>();
    (!relative.as_os_str().is_empty()).then_some(relative)
}

fn std_search_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();

    if let Some(paths) = env::var_os("MST_STD_PATH") {
        for path in env::split_paths(&paths) {
            push_unique_path(&mut roots, path);
        }
    }

    if let Ok(exe) = env::current_exe() {
        if let Some(bin_dir) = exe.parent() {
            if let Some(prefix) = bin_dir.parent() {
                push_unique_path(&mut roots, prefix.join("share").join("mst").join("std"));
                push_unique_path(&mut roots, prefix.join("std"));
            }

            push_unique_path(&mut roots, bin_dir.join("std"));
        }
    }

    push_unique_path(
        &mut roots,
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("std"),
    );

    if let Some(home) = env::var_os("HOME") {
        push_unique_path(
            &mut roots,
            PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("mst")
                .join("std"),
        );
    }

    push_unique_path(
        &mut roots,
        PathBuf::from("/usr/local")
            .join("share")
            .join("mst")
            .join("std"),
    );

    roots
}

fn push_unique_path(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !paths.iter().any(|existing| existing == &path) {
        paths.push(path);
    }
}

fn empty_program() -> ast::Program {
    ast::Program {
        imports: Vec::new(),
        consts: Vec::new(),
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
        consts: program
            .consts
            .into_iter()
            .map(|const_def| rewrite_const(const_def, module_aliases, &visible_functions))
            .collect(),
        enums: program.enums,
        structs: program.structs,
        functions: program
            .functions
            .into_iter()
            .map(|function| rewrite_function(function, module_aliases, &visible_functions))
            .collect(),
    }
}

fn rewrite_const(
    mut const_def: ast::ConstDef,
    module_aliases: &HashMap<String, String>,
    visible_functions: &HashMap<String, String>,
) -> ast::ConstDef {
    const_def.value = rewrite_expr(const_def.value, module_aliases, visible_functions);
    const_def
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
        ast::Stmt::Defer { expr } => ast::Stmt::Defer {
            expr: rewrite_expr(expr, module_aliases, visible_functions),
        },
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

fn build_to_binary(input: &str, options: &BuildOptions) -> Result<PathBuf, String> {
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

    let compile_input = if options.opt_level.is_optimizing() {
        optimize_llvm_ir(&ll_path, &opt_ll_path, options.opt_level)?;
        verify_llvm_ir(&opt_ll_path)?;
        opt_ll_path.as_path()
    } else {
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
    };

    compile_to_native(input, compile_input, &out_path, options)?;

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
    options: &BuildOptions,
) -> Result<(), String> {
    let clang = find_tool(&["clang-18", "clang"])
        .ok_or_else(|| "failed to find clang-18 or clang on PATH".to_string())?;

    let mut command = Command::new(&clang);
    command.arg(llvm_input);
    command.arg(options.opt_level.clang_arg());

    if options.mode == BuildMode::Debug {
        command.arg("-g");
    }

    if options.cpu == TargetCpu::Native {
        command.arg("-march=native");
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

fn optimize_llvm_ir(input: &Path, output: &Path, opt_level: OptLevel) -> Result<(), String> {
    let opt = find_tool(&["opt-18", "opt"])
        .ok_or_else(|| "failed to find opt-18 or opt on PATH".to_string())?;
    let passes = format!("default<O{}>", opt_level.as_u8());

    let command_output = Command::new(&opt)
        .arg(format!("-passes={passes}"))
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
        BuildArgs, BuildMode, BuildOptionOverrides, BuildOptions, InitArgs, Lexer, OptLevel,
        RunArgs, TargetCpu, build_to_binary, init_project, load_program, parse_build_args,
        parse_project_config, parse_run_args, resolve_build_args,
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
                input: Some("exam.mnst".to_string()),
                overrides: BuildOptionOverrides::default(),
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
                input: Some("exam.mnst".to_string()),
                overrides: BuildOptionOverrides {
                    mode: Some(BuildMode::Debug),
                    opt_level: None,
                    cpu: None,
                },
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
                input: Some("exam.mnst".to_string()),
                overrides: BuildOptionOverrides {
                    mode: Some(BuildMode::Debug),
                    opt_level: None,
                    cpu: None,
                },
            }
        );
    }

    #[test]
    fn parses_build_optimization_flags() {
        let parsed = parse_build_args(
            vec![
                "--release".to_string(),
                "--opt-level=3".to_string(),
                "--cpu".to_string(),
                "native".to_string(),
                "exam.mnst".to_string(),
            ]
            .into_iter(),
        )
        .unwrap();

        assert_eq!(
            parsed,
            BuildArgs {
                input: Some("exam.mnst".to_string()),
                overrides: BuildOptionOverrides {
                    mode: Some(BuildMode::Release),
                    opt_level: Some(OptLevel::O3),
                    cpu: Some(TargetCpu::Native),
                },
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
                    input: Some("exam.mnst".to_string()),
                    overrides: BuildOptionOverrides::default(),
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
                    input: Some("exam.mnst".to_string()),
                    overrides: BuildOptionOverrides {
                        mode: Some(BuildMode::Debug),
                        opt_level: None,
                        cpu: None,
                    },
                },
                program_args: vec!["alpha".to_string(), "--flag".to_string()],
            }
        );
    }

    #[test]
    fn parses_project_config_build_options() {
        let config = parse_project_config(
            r#"
            [package]
            name = "demo"
            entry = "src/main.mnst"

            [build]
            profile = "debug"
            opt-level = 1
            cpu = "native"
            "#,
            Path::new("Monster.toml"),
        )
        .unwrap();

        assert_eq!(config.package.name, Some("demo".to_string()));
        assert_eq!(config.package.entry, Some("src/main.mnst".to_string()));
        assert_eq!(config.build.mode, Some(BuildMode::Debug));
        assert_eq!(config.build.opt_level, Some(OptLevel::O1));
        assert_eq!(config.build.cpu, Some(TargetCpu::Native));
    }

    #[test]
    fn resolves_manifest_entry_and_cli_overrides() {
        let _guard = SELFHOST_LEXER_TEST_LOCK.lock().unwrap();
        let temp_dir = unique_temp_dir("monster-manifest");
        fs::create_dir_all(temp_dir.join("src")).unwrap();
        fs::write(
            temp_dir.join("Monster.toml"),
            r#"
            [package]
            name = "demo"
            entry = "src/main.mnst"

            [build]
            profile = "debug"
            cpu = "native"
            "#,
        )
        .unwrap();
        fs::write(
            temp_dir.join("src").join("main.mnst"),
            r#"
            fn main() -> i32 {
                return 0;
            }
            "#,
        )
        .unwrap();

        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(&temp_dir).unwrap();
        let resolved = resolve_build_args(BuildArgs {
            input: None,
            overrides: BuildOptionOverrides {
                mode: Some(BuildMode::Release),
                opt_level: Some(OptLevel::O3),
                cpu: None,
            },
        })
        .unwrap();
        std::env::set_current_dir(original_dir).unwrap();

        assert_eq!(
            PathBuf::from(&resolved.input),
            temp_dir.join("src/main.mnst")
        );
        assert_eq!(
            resolved.options,
            BuildOptions {
                mode: BuildMode::Release,
                opt_level: OptLevel::O3,
                cpu: TargetCpu::Native,
            }
        );

        fs::remove_dir_all(temp_dir).unwrap();
    }

    #[test]
    fn init_project_writes_default_skeleton() {
        let temp_dir = unique_temp_dir("monster-init");
        let project_dir = temp_dir.join("demo-app");

        init_project(&InitArgs {
            path: project_dir.clone(),
        })
        .unwrap();

        let manifest = fs::read_to_string(project_dir.join("Monster.toml")).unwrap();
        let main = fs::read_to_string(project_dir.join("src/main.mnst")).unwrap();
        let gitignore = fs::read_to_string(project_dir.join(".gitignore")).unwrap();

        assert!(manifest.contains("name = \"demo-app\""));
        assert!(manifest.contains("entry = \"src/main.mnst\""));
        assert!(manifest.contains("opt-level = 2"));
        assert!(main.contains("Hello, Monster!"));
        assert!(gitignore.contains("target/"));

        fs::remove_dir_all(temp_dir).unwrap();
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
    fn loads_std_imports_from_compiler_std_dir() {
        let temp_dir = unique_temp_dir("monster-std-imports");
        fs::create_dir_all(&temp_dir).unwrap();

        fs::write(
            temp_dir.join("main.mnst"),
            r#"
            import "std/vec_i32.mnst";

            fn main() -> i32 {
                let mut vec = vec_i32_new();
                defer vec_i32_free(vec);

                vec_i32_push(&vec, 7);
                return vec_i32_get(vec, 0);
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
        let struct_names = program
            .structs
            .iter()
            .map(|struct_def| struct_def.name.clone())
            .collect::<Vec<_>>();

        assert!(struct_names.contains(&"VecI32".to_string()));
        assert!(function_names.contains(&"vec_i32_new".to_string()));
        assert!(function_names.contains(&"vec_i32_push".to_string()));
        assert!(function_names.contains(&"vec_i32_get".to_string()));
        assert!(function_names.contains(&"vec_i32_free".to_string()));
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

        const LIMIT: usize = 64 as usize;

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
            let mut value: i32 = (sizeof(Pair) + LIMIT) as i32;
            value = value + 10 - 3 * 2 / 1;
            let ptr: *i32 = &value;
            defer print_ln_str("done");

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
        let selfhost_lexer =
            build_to_binary("selfhost/main.mnst", &BuildOptions::default()).unwrap();
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
            TokenKind::Defer => 51,
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
            TokenKind::Const => 49,
            TokenKind::Eof => 50,
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
