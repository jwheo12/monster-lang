use crate::ast::Type;

pub(super) fn llvm_type(ty: &Type) -> String {
    match ty {
        Type::I32 => "i32".to_string(),
        Type::Bool => "i1".to_string(),
        Type::Str => "ptr".to_string(),
        Type::Void => "void".to_string(),
        Type::Named(name) => format!("%struct.{name}"),
        Type::Array(element_ty, len) => format!("[{} x {}]", len, llvm_type(element_ty)),
        Type::Slice(_) => "{ ptr, i32 }".to_string(),
        Type::Ptr(_) => "ptr".to_string(),
    }
}

pub(super) fn llvm_escape_string_literal(value: &str) -> String {
    let mut out = String::new();

    for byte in value.as_bytes() {
        match *byte {
            b' '..=b'!' | b'#'..=b'[' | b']'..=b'~' => out.push(*byte as char),
            _ => out.push_str(&format!("\\{:02X}", byte)),
        }
    }

    out.push_str("\\00");
    out
}

pub(super) fn host_target_triple() -> &'static str {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        "x86_64-pc-linux-gnu"
    }

    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        "x86_64-apple-darwin"
    }

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        "arm64-apple-darwin"
    }

    #[cfg(all(target_os = "windows", target_arch = "x86_64", target_env = "msvc"))]
    {
        "x86_64-pc-windows-msvc"
    }

    #[cfg(all(target_os = "windows", target_arch = "x86_64", target_env = "gnu"))]
    {
        "x86_64-pc-windows-gnu"
    }

    #[cfg(not(any(
        all(target_os = "linux", target_arch = "x86_64"),
        all(target_os = "macos", target_arch = "x86_64"),
        all(target_os = "macos", target_arch = "aarch64"),
        all(target_os = "windows", target_arch = "x86_64", target_env = "msvc"),
        all(target_os = "windows", target_arch = "x86_64", target_env = "gnu")
    )))]
    {
        "x86_64-pc-linux-gnu"
    }
}
