use std::collections::HashMap;

use crate::ast::Type;

use super::EnumLayout;

pub(super) fn llvm_type(ty: &Type, enum_layouts: &HashMap<String, EnumLayout>) -> String {
    match ty {
        Type::I32 => "i32".to_string(),
        Type::U8 => "i8".to_string(),
        Type::USize => "i64".to_string(),
        Type::Bool => "i1".to_string(),
        Type::Str => "ptr".to_string(),
        Type::Void => "void".to_string(),
        Type::Named(name) => {
            if let Some(layout) = enum_layouts.get(name) {
                if layout.has_payload {
                    format!("%enum.{name}")
                } else {
                    "i32".to_string()
                }
            } else {
                format!("%struct.{name}")
            }
        }
        Type::Array(element_ty, len) => {
            format!("[{} x {}]", len, llvm_type(element_ty, enum_layouts))
        }
        Type::Slice(_) => "{ ptr, i64 }".to_string(),
        Type::Ptr(_) => "ptr".to_string(),
    }
}

pub(super) fn integer_bit_width(ty: &Type) -> Option<u32> {
    match ty {
        Type::Bool => Some(1),
        Type::U8 => Some(8),
        Type::I32 => Some(32),
        Type::USize => Some(64),
        _ => None,
    }
}

pub(super) fn is_signed_integer_type(ty: &Type) -> bool {
    matches!(ty, Type::I32)
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

    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    {
        "aarch64-pc-linux-gnu"
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
        all(target_os = "linux", target_arch = "aarch64"),
        all(target_os = "macos", target_arch = "x86_64"),
        all(target_os = "macos", target_arch = "aarch64"),
        all(target_os = "windows", target_arch = "x86_64", target_env = "msvc"),
        all(target_os = "windows", target_arch = "x86_64", target_env = "gnu")
    )))]
    {
        "x86_64-pc-linux-gnu"
    }
}
