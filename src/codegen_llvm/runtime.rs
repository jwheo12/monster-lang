use std::collections::HashMap;

use crate::ast::Type;

use super::FunctionSig;

pub(super) fn builtin_signatures() -> HashMap<String, FunctionSig> {
    let mut sigs = HashMap::new();
    sigs.insert(
        "print_i32".to_string(),
        FunctionSig {
            params: vec![Type::I32],
            ret_type: Type::Void,
        },
    );
    sigs.insert(
        "print_bool".to_string(),
        FunctionSig {
            params: vec![Type::Bool],
            ret_type: Type::Void,
        },
    );
    sigs.insert(
        "print_str".to_string(),
        FunctionSig {
            params: vec![Type::Str],
            ret_type: Type::Void,
        },
    );
    sigs.insert(
        "read_i32".to_string(),
        FunctionSig {
            params: vec![],
            ret_type: Type::I32,
        },
    );
    sigs
}

pub(super) fn emit_runtime_prelude() -> String {
    [
        "@.fmt.print_i32 = private unnamed_addr constant [4 x i8] c\"%d\\0A\\00\"",
        "@.fmt.scan_i32 = private unnamed_addr constant [3 x i8] c\"%d\\00\"",
        "@.str.true = private unnamed_addr constant [5 x i8] c\"true\\00\"",
        "@.str.false = private unnamed_addr constant [6 x i8] c\"false\\00\"",
        "@.str.read_i32_error = private unnamed_addr constant [42 x i8] c\"Monster runtime error: expected i32 input\\00\"",
        "",
        "declare i32 @printf(ptr, ...)",
        "declare i32 @puts(ptr)",
        "declare i32 @scanf(ptr, ...)",
        "declare void @exit(i32)",
        "",
        "define internal void @__monster_builtin_print_i32(i32 %value) {",
        "entry:",
        "  %call.0 = call i32 (ptr, ...) @printf(ptr getelementptr inbounds ([4 x i8], ptr @.fmt.print_i32, i64 0, i64 0), i32 %value)",
        "  ret void",
        "}",
        "",
        "define internal void @__monster_builtin_print_bool(i1 %value) {",
        "entry:",
        "  %str.0 = select i1 %value, ptr getelementptr inbounds ([5 x i8], ptr @.str.true, i64 0, i64 0), ptr getelementptr inbounds ([6 x i8], ptr @.str.false, i64 0, i64 0)",
        "  %call.1 = call i32 @puts(ptr %str.0)",
        "  ret void",
        "}",
        "",
        "define internal void @__monster_builtin_print_str(ptr %value) {",
        "entry:",
        "  %call.3 = call i32 @puts(ptr %value)",
        "  ret void",
        "}",
        "",
        "define internal i32 @__monster_builtin_read_i32() {",
        "entry:",
        "  %value.addr = alloca i32",
        "  %scan.0 = call i32 (ptr, ...) @scanf(ptr getelementptr inbounds ([3 x i8], ptr @.fmt.scan_i32, i64 0, i64 0), ptr %value.addr)",
        "  %scan.ok = icmp eq i32 %scan.0, 1",
        "  br i1 %scan.ok, label %read.ok, label %read.fail",
        "",
        "read.fail:",
        "  %call.2 = call i32 @puts(ptr getelementptr inbounds ([42 x i8], ptr @.str.read_i32_error, i64 0, i64 0))",
        "  call void @exit(i32 1)",
        "  unreachable",
        "",
        "read.ok:",
        "  %value.0 = load i32, ptr %value.addr",
        "  ret i32 %value.0",
        "}",
        "",
    ]
    .join("\n")
}

pub(super) fn runtime_declared_function(name: &str) -> bool {
    matches!(name, "printf" | "puts" | "scanf" | "exit")
}

pub(super) fn llvm_function_name(name: &str) -> String {
    match name {
        "print_i32" => "@__monster_builtin_print_i32".to_string(),
        "print_bool" => "@__monster_builtin_print_bool".to_string(),
        "print_str" => "@__monster_builtin_print_str".to_string(),
        "read_i32" => "@__monster_builtin_read_i32".to_string(),
        _ => format!("@{name}"),
    }
}
