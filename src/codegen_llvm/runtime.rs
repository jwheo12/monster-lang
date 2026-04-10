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
        "print_ln_i32".to_string(),
        FunctionSig {
            params: vec![Type::I32],
            ret_type: Type::Void,
        },
    );
    sigs.insert(
        "print_ln_bool".to_string(),
        FunctionSig {
            params: vec![Type::Bool],
            ret_type: Type::Void,
        },
    );
    sigs.insert(
        "print_ln_str".to_string(),
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
    sigs.insert(
        "read_file".to_string(),
        FunctionSig {
            params: vec![Type::Str, Type::Ptr(Box::new(Type::USize))],
            ret_type: Type::Ptr(Box::new(Type::U8)),
        },
    );
    sigs.insert(
        "write_file".to_string(),
        FunctionSig {
            params: vec![Type::Str, Type::Ptr(Box::new(Type::U8)), Type::USize],
            ret_type: Type::Void,
        },
    );
    sigs.insert(
        "strlen".to_string(),
        FunctionSig {
            params: vec![Type::Str],
            ret_type: Type::USize,
        },
    );
    sigs.insert(
        "memcmp".to_string(),
        FunctionSig {
            params: vec![
                Type::Ptr(Box::new(Type::U8)),
                Type::Ptr(Box::new(Type::U8)),
                Type::USize,
            ],
            ret_type: Type::I32,
        },
    );
    sigs.insert(
        "memcpy".to_string(),
        FunctionSig {
            params: vec![
                Type::Ptr(Box::new(Type::U8)),
                Type::Ptr(Box::new(Type::U8)),
                Type::USize,
            ],
            ret_type: Type::Void,
        },
    );
    sigs.insert(
        "str_eq".to_string(),
        FunctionSig {
            params: vec![Type::Str, Type::Str],
            ret_type: Type::Bool,
        },
    );
    sigs
}

pub(super) fn emit_runtime_prelude() -> String {
    [
        "@.fmt.print_i32 = private unnamed_addr constant [3 x i8] c\"%d\\00\"",
        "@.fmt.print_ln_i32 = private unnamed_addr constant [4 x i8] c\"%d\\0A\\00\"",
        "@.fmt.print_str = private unnamed_addr constant [3 x i8] c\"%s\\00\"",
        "@.fmt.scan_i32 = private unnamed_addr constant [3 x i8] c\"%d\\00\"",
        "@.file.mode.read = private unnamed_addr constant [3 x i8] c\"rb\\00\"",
        "@.file.mode.write = private unnamed_addr constant [3 x i8] c\"wb\\00\"",
        "@.str.true = private unnamed_addr constant [5 x i8] c\"true\\00\"",
        "@.str.false = private unnamed_addr constant [6 x i8] c\"false\\00\"",
        "@.str.read_i32_error = private unnamed_addr constant [42 x i8] c\"Monster runtime error: expected i32 input\\00\"",
        "@.str.file_open_error = private unnamed_addr constant [43 x i8] c\"Monster runtime error: failed to open file\\00\"",
        "@.str.file_seek_error = private unnamed_addr constant [43 x i8] c\"Monster runtime error: failed to seek file\\00\"",
        "@.str.file_alloc_error = private unnamed_addr constant [54 x i8] c\"Monster runtime error: failed to allocate file buffer\\00\"",
        "@.str.file_read_error = private unnamed_addr constant [43 x i8] c\"Monster runtime error: failed to read file\\00\"",
        "@.str.file_write_error = private unnamed_addr constant [44 x i8] c\"Monster runtime error: failed to write file\\00\"",
        "@.str.enum_payload_error = private unnamed_addr constant [49 x i8] c\"Monster runtime error: wrong enum payload access\\00\"",
        "",
        "declare i32 @printf(ptr, ...)",
        "declare i32 @puts(ptr)",
        "declare i32 @scanf(ptr, ...)",
        "declare ptr @fopen(ptr, ptr)",
        "declare i32 @fclose(ptr)",
        "declare i32 @fseek(ptr, i64, i32)",
        "declare i64 @ftell(ptr)",
        "declare i64 @fread(ptr, i64, i64, ptr)",
        "declare i64 @fwrite(ptr, i64, i64, ptr)",
        "declare ptr @calloc(i64, i64)",
        "declare i64 @strlen(ptr)",
        "declare i32 @memcmp(ptr, ptr, i64)",
        "declare ptr @memcpy(ptr, ptr, i64)",
        "declare void @exit(i32)",
        "",
        "define internal void @__monster_builtin_print_i32(i32 %value) {",
        "entry:",
        "  %call.0 = call i32 (ptr, ...) @printf(ptr getelementptr inbounds ([3 x i8], ptr @.fmt.print_i32, i64 0, i64 0), i32 %value)",
        "  ret void",
        "}",
        "",
        "define internal void @__monster_builtin_print_ln_i32(i32 %value) {",
        "entry:",
        "  %call.1 = call i32 (ptr, ...) @printf(ptr getelementptr inbounds ([4 x i8], ptr @.fmt.print_ln_i32, i64 0, i64 0), i32 %value)",
        "  ret void",
        "}",
        "",
        "define internal void @__monster_builtin_print_bool(i1 %value) {",
        "entry:",
        "  %str.0 = select i1 %value, ptr getelementptr inbounds ([5 x i8], ptr @.str.true, i64 0, i64 0), ptr getelementptr inbounds ([6 x i8], ptr @.str.false, i64 0, i64 0)",
        "  %call.2 = call i32 (ptr, ...) @printf(ptr getelementptr inbounds ([3 x i8], ptr @.fmt.print_str, i64 0, i64 0), ptr %str.0)",
        "  ret void",
        "}",
        "",
        "define internal void @__monster_builtin_print_ln_bool(i1 %value) {",
        "entry:",
        "  %str.1 = select i1 %value, ptr getelementptr inbounds ([5 x i8], ptr @.str.true, i64 0, i64 0), ptr getelementptr inbounds ([6 x i8], ptr @.str.false, i64 0, i64 0)",
        "  %call.3 = call i32 @puts(ptr %str.1)",
        "  ret void",
        "}",
        "",
        "define internal void @__monster_builtin_print_str(ptr %value) {",
        "entry:",
        "  %call.4 = call i32 (ptr, ...) @printf(ptr getelementptr inbounds ([3 x i8], ptr @.fmt.print_str, i64 0, i64 0), ptr %value)",
        "  ret void",
        "}",
        "",
        "define internal void @__monster_builtin_print_ln_str(ptr %value) {",
        "entry:",
        "  %call.5 = call i32 @puts(ptr %value)",
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
        "define internal ptr @__monster_builtin_read_file(ptr %path, ptr %out_len) {",
        "entry:",
        "  %file.0 = call ptr @fopen(ptr %path, ptr getelementptr inbounds ([3 x i8], ptr @.file.mode.read, i64 0, i64 0))",
        "  %file.ok = icmp ne ptr %file.0, null",
        "  br i1 %file.ok, label %seek.end, label %open.fail",
        "",
        "open.fail:",
        "  %call.open = call i32 @puts(ptr getelementptr inbounds ([43 x i8], ptr @.str.file_open_error, i64 0, i64 0))",
        "  call void @exit(i32 1)",
        "  unreachable",
        "",
        "seek.end:",
        "  %seek.end.result = call i32 @fseek(ptr %file.0, i64 0, i32 2)",
        "  %seek.end.ok = icmp eq i32 %seek.end.result, 0",
        "  br i1 %seek.end.ok, label %tell, label %seek.fail",
        "",
        "tell:",
        "  %size.0 = call i64 @ftell(ptr %file.0)",
        "  %size.ok = icmp sge i64 %size.0, 0",
        "  br i1 %size.ok, label %rewind, label %seek.fail",
        "",
        "rewind:",
        "  %seek.start.result = call i32 @fseek(ptr %file.0, i64 0, i32 0)",
        "  %seek.start.ok = icmp eq i32 %seek.start.result, 0",
        "  br i1 %seek.start.ok, label %alloc, label %seek.fail",
        "",
        "alloc:",
        "  %alloc.size = add i64 %size.0, 1",
        "  %buffer.0 = call ptr @calloc(i64 1, i64 %alloc.size)",
        "  %buffer.ok = icmp ne ptr %buffer.0, null",
        "  br i1 %buffer.ok, label %read, label %alloc.fail",
        "",
        "alloc.fail:",
        "  %close.alloc = call i32 @fclose(ptr %file.0)",
        "  %call.alloc = call i32 @puts(ptr getelementptr inbounds ([54 x i8], ptr @.str.file_alloc_error, i64 0, i64 0))",
        "  call void @exit(i32 1)",
        "  unreachable",
        "",
        "read:",
        "  %bytes.read = call i64 @fread(ptr %buffer.0, i64 1, i64 %size.0, ptr %file.0)",
        "  %read.ok = icmp eq i64 %bytes.read, %size.0",
        "  br i1 %read.ok, label %finish, label %read.fail",
        "",
        "seek.fail:",
        "  %close.seek = call i32 @fclose(ptr %file.0)",
        "  %call.seek = call i32 @puts(ptr getelementptr inbounds ([43 x i8], ptr @.str.file_seek_error, i64 0, i64 0))",
        "  call void @exit(i32 1)",
        "  unreachable",
        "",
        "read.fail:",
        "  %close.read = call i32 @fclose(ptr %file.0)",
        "  %call.read = call i32 @puts(ptr getelementptr inbounds ([43 x i8], ptr @.str.file_read_error, i64 0, i64 0))",
        "  call void @exit(i32 1)",
        "  unreachable",
        "",
        "finish:",
        "  %close.finish = call i32 @fclose(ptr %file.0)",
        "  store i64 %size.0, ptr %out_len",
        "  ret ptr %buffer.0",
        "}",
        "",
        "define internal void @__monster_builtin_write_file(ptr %path, ptr %data, i64 %len) {",
        "entry:",
        "  %file.1 = call ptr @fopen(ptr %path, ptr getelementptr inbounds ([3 x i8], ptr @.file.mode.write, i64 0, i64 0))",
        "  %file.ok = icmp ne ptr %file.1, null",
        "  br i1 %file.ok, label %write, label %write.fail",
        "",
        "write.fail:",
        "  %call.open = call i32 @puts(ptr getelementptr inbounds ([44 x i8], ptr @.str.file_write_error, i64 0, i64 0))",
        "  call void @exit(i32 1)",
        "  unreachable",
        "",
        "write:",
        "  %bytes.written = call i64 @fwrite(ptr %data, i64 1, i64 %len, ptr %file.1)",
        "  %close.write = call i32 @fclose(ptr %file.1)",
        "  %write.ok = icmp eq i64 %bytes.written, %len",
        "  br i1 %write.ok, label %done, label %write.error",
        "",
        "write.error:",
        "  %call.write = call i32 @puts(ptr getelementptr inbounds ([44 x i8], ptr @.str.file_write_error, i64 0, i64 0))",
        "  call void @exit(i32 1)",
        "  unreachable",
        "",
        "done:",
        "  ret void",
        "}",
        "",
        "define internal i64 @__monster_builtin_strlen(ptr %value) {",
        "entry:",
        "  %len.0 = call i64 @strlen(ptr %value)",
        "  ret i64 %len.0",
        "}",
        "",
        "define internal i32 @__monster_builtin_memcmp(ptr %lhs, ptr %rhs, i64 %len) {",
        "entry:",
        "  %cmp.0 = call i32 @memcmp(ptr %lhs, ptr %rhs, i64 %len)",
        "  ret i32 %cmp.0",
        "}",
        "",
        "define internal void @__monster_builtin_memcpy(ptr %dst, ptr %src, i64 %len) {",
        "entry:",
        "  %copy.0 = call ptr @memcpy(ptr %dst, ptr %src, i64 %len)",
        "  ret void",
        "}",
        "",
        "define internal i1 @__monster_builtin_str_eq(ptr %lhs, ptr %rhs) {",
        "entry:",
        "  %lhs.len = call i64 @strlen(ptr %lhs)",
        "  %rhs.len = call i64 @strlen(ptr %rhs)",
        "  %same.len = icmp eq i64 %lhs.len, %rhs.len",
        "  br i1 %same.len, label %compare, label %not.equal",
        "",
        "compare:",
        "  %cmp.1 = call i32 @memcmp(ptr %lhs, ptr %rhs, i64 %lhs.len)",
        "  %same.bytes = icmp eq i32 %cmp.1, 0",
        "  ret i1 %same.bytes",
        "",
        "not.equal:",
        "  ret i1 0",
        "}",
        "",
    ]
    .join("\n")
}

pub(super) fn runtime_declared_function(name: &str) -> bool {
    matches!(
        name,
        "printf"
            | "puts"
            | "scanf"
            | "fopen"
            | "fclose"
            | "fseek"
            | "ftell"
            | "fread"
            | "fwrite"
            | "calloc"
            | "strlen"
            | "memcmp"
            | "memcpy"
            | "exit"
    )
}

pub(super) fn llvm_function_name(name: &str) -> String {
    match name {
        "print_i32" => "@__monster_builtin_print_i32".to_string(),
        "print_bool" => "@__monster_builtin_print_bool".to_string(),
        "print_str" => "@__monster_builtin_print_str".to_string(),
        "print_ln_i32" => "@__monster_builtin_print_ln_i32".to_string(),
        "print_ln_bool" => "@__monster_builtin_print_ln_bool".to_string(),
        "print_ln_str" => "@__monster_builtin_print_ln_str".to_string(),
        "read_i32" => "@__monster_builtin_read_i32".to_string(),
        "read_file" => "@__monster_builtin_read_file".to_string(),
        "write_file" => "@__monster_builtin_write_file".to_string(),
        "strlen" => "@__monster_builtin_strlen".to_string(),
        "memcmp" => "@__monster_builtin_memcmp".to_string(),
        "memcpy" => "@__monster_builtin_memcpy".to_string(),
        "str_eq" => "@__monster_builtin_str_eq".to_string(),
        _ => format!("@{}", name.replace('.', "__")),
    }
}
