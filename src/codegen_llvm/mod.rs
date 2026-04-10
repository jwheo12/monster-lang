use std::collections::{HashMap, HashSet};

use crate::ast::{Expr, Program, Stmt, Type};

mod emitter;
mod runtime;
mod util;

use emitter::FunctionEmitter;
use runtime::{
    builtin_signatures, emit_runtime_prelude, llvm_function_name, runtime_declared_function,
};
use util::{host_target_triple, llvm_escape_string_literal, llvm_type};

#[derive(Clone)]
pub(super) struct FunctionSig {
    params: Vec<Type>,
    ret_type: Type,
}

#[derive(Clone)]
pub(super) struct StructLayout {
    fields: Vec<(String, Type)>,
}

#[derive(Clone)]
pub(super) struct StringLiteralData {
    global_name: String,
    len: usize,
}

#[derive(Clone)]
pub(super) struct EnumVariantInfo {
    pub enum_name: String,
    pub discriminant: i32,
    pub payload_ty: Option<Type>,
}

#[derive(Clone)]
pub(super) struct EnumLayout {
    pub has_payload: bool,
    pub payload_words: usize,
}

pub fn emit_program(program: &Program) -> Result<String, String> {
    let struct_layouts = collect_struct_layouts(program);
    let enum_layouts = collect_enum_layouts(program, &struct_layouts)?;
    let enum_variants = collect_enum_variants(program);
    let function_sigs = collect_function_sigs(program);
    let string_literals = collect_string_literals(program);

    let mut out = String::new();
    out.push_str("; Monster LLVM IR backend\n");
    out.push_str(&format!("target triple = \"{}\"\n\n", host_target_triple()));
    out.push_str(&emit_runtime_prelude());
    out.push_str(&emit_enum_definitions(program, &enum_layouts));
    out.push_str(&emit_struct_definitions(program, &enum_layouts));
    out.push_str(&emit_string_literal_globals(&string_literals));
    out.push_str(&emit_extern_declarations(program, &enum_layouts));

    for function in &program.functions {
        if function.is_extern {
            continue;
        }
        let mut emitter = FunctionEmitter::new(
            function,
            &function_sigs,
            &struct_layouts,
            &enum_layouts,
            &enum_variants,
            &string_literals,
        );
        out.push_str(&emitter.emit()?);
        out.push('\n');
    }

    Ok(out)
}

fn collect_struct_layouts(program: &Program) -> HashMap<String, StructLayout> {
    let mut layouts = HashMap::new();

    for struct_def in &program.structs {
        layouts.insert(
            struct_def.name.clone(),
            StructLayout {
                fields: struct_def.fields.clone(),
            },
        );
    }

    layouts
}

fn collect_enum_variants(program: &Program) -> HashMap<String, EnumVariantInfo> {
    let mut variants = HashMap::new();

    for enum_def in &program.enums {
        for (index, variant) in enum_def.variants.iter().enumerate() {
            variants.insert(
                variant.name.clone(),
                EnumVariantInfo {
                    enum_name: enum_def.name.clone(),
                    discriminant: index as i32,
                    payload_ty: variant.payload.clone(),
                },
            );
        }
    }

    variants
}

fn collect_enum_layouts(
    program: &Program,
    struct_layouts: &HashMap<String, StructLayout>,
) -> Result<HashMap<String, EnumLayout>, String> {
    let enum_defs = program
        .enums
        .iter()
        .map(|enum_def| (enum_def.name.clone(), enum_def))
        .collect::<HashMap<_, _>>();

    let mut cache = HashMap::new();
    let mut active = HashSet::new();

    for enum_def in &program.enums {
        compute_enum_layout(
            &enum_def.name,
            &enum_defs,
            struct_layouts,
            &mut cache,
            &mut active,
        )?;
    }

    Ok(cache)
}

fn collect_function_sigs(program: &Program) -> HashMap<String, FunctionSig> {
    let mut sigs = builtin_signatures();

    for function in &program.functions {
        sigs.insert(
            function.name.clone(),
            FunctionSig {
                params: function.params.iter().map(|(_, ty)| ty.clone()).collect(),
                ret_type: function.ret_type.clone(),
            },
        );
    }

    sigs
}

fn collect_string_literals(program: &Program) -> HashMap<String, StringLiteralData> {
    let mut string_literals = HashMap::new();
    let mut next_index = 0;

    for function in &program.functions {
        if let Some(body) = &function.body {
            collect_strings_from_stmts(body, &mut string_literals, &mut next_index);
        }
    }

    string_literals
}

fn compute_enum_layout<'a>(
    name: &str,
    enum_defs: &HashMap<String, &'a crate::ast::EnumDef>,
    struct_layouts: &HashMap<String, StructLayout>,
    cache: &mut HashMap<String, EnumLayout>,
    active: &mut HashSet<String>,
) -> Result<EnumLayout, String> {
    if let Some(layout) = cache.get(name) {
        return Ok(layout.clone());
    }

    if !active.insert(name.to_string()) {
        return Err(format!(
            "unsupported recursive enum layout involving '{}'",
            name
        ));
    }

    let enum_def = enum_defs
        .get(name)
        .ok_or_else(|| format!("internal error: unknown enum '{}'", name))?;

    let mut max_payload_bytes = 0usize;
    for variant in &enum_def.variants {
        if let Some(payload_ty) = &variant.payload {
            let (size, _) = type_size_align(payload_ty, struct_layouts, enum_defs, cache, active)?;
            max_payload_bytes = max_payload_bytes.max(size);
        }
    }

    let layout = if max_payload_bytes == 0 {
        EnumLayout {
            has_payload: false,
            payload_words: 0,
        }
    } else {
        EnumLayout {
            has_payload: true,
            payload_words: max_payload_bytes.div_ceil(8),
        }
    };

    active.remove(name);
    cache.insert(name.to_string(), layout.clone());
    Ok(layout)
}

fn type_size_align<'a>(
    ty: &Type,
    struct_layouts: &HashMap<String, StructLayout>,
    enum_defs: &HashMap<String, &'a crate::ast::EnumDef>,
    enum_layouts: &mut HashMap<String, EnumLayout>,
    active_enums: &mut HashSet<String>,
) -> Result<(usize, usize), String> {
    match ty {
        Type::I32 => Ok((4, 4)),
        Type::U8 => Ok((1, 1)),
        Type::USize | Type::Str | Type::Ptr(_) => Ok((8, 8)),
        Type::Bool => Ok((1, 1)),
        Type::Void => Err("internal error: void has no size".to_string()),
        Type::Array(element_ty, len) => {
            let (element_size, element_align) = type_size_align(
                element_ty,
                struct_layouts,
                enum_defs,
                enum_layouts,
                active_enums,
            )?;
            Ok((align_to(element_size, element_align) * len, element_align))
        }
        Type::Slice(_) => Ok((16, 8)),
        Type::Named(name) => {
            if let Some(struct_layout) = struct_layouts.get(name) {
                let mut size = 0usize;
                let mut max_align = 1usize;

                for (_, field_ty) in &struct_layout.fields {
                    let (field_size, field_align) = type_size_align(
                        field_ty,
                        struct_layouts,
                        enum_defs,
                        enum_layouts,
                        active_enums,
                    )?;
                    size = align_to(size, field_align);
                    size += field_size;
                    max_align = max_align.max(field_align);
                }

                Ok((align_to(size, max_align), max_align))
            } else {
                let layout = compute_enum_layout(
                    name,
                    enum_defs,
                    struct_layouts,
                    enum_layouts,
                    active_enums,
                )?;
                if layout.has_payload {
                    Ok((8 + (layout.payload_words * 8), 8))
                } else {
                    Ok((4, 4))
                }
            }
        }
    }
}

fn align_to(size: usize, align: usize) -> usize {
    if align == 0 {
        size
    } else {
        size.div_ceil(align) * align
    }
}

fn emit_extern_declarations(
    program: &Program,
    enum_layouts: &HashMap<String, EnumLayout>,
) -> String {
    let mut out = String::new();

    for function in &program.functions {
        if !function.is_extern || runtime_declared_function(function.name.as_str()) {
            continue;
        }

        let params = function
            .params
            .iter()
            .map(|(_, ty)| llvm_type(ty, enum_layouts))
            .collect::<Vec<_>>()
            .join(", ");

        out.push_str(&format!(
            "declare {} @{}({})\n",
            llvm_type(&function.ret_type, enum_layouts),
            llvm_function_name(&function.name).trim_start_matches('@'),
            params
        ));
    }

    if !out.is_empty() {
        out.push('\n');
    }

    out
}

fn emit_enum_definitions(program: &Program, enum_layouts: &HashMap<String, EnumLayout>) -> String {
    let mut out = String::new();

    for enum_def in &program.enums {
        let Some(layout) = enum_layouts.get(&enum_def.name) else {
            continue;
        };

        if !layout.has_payload {
            continue;
        }

        out.push_str(&format!(
            "%enum.{} = type {{ i32, i32, [{} x i64] }}\n",
            enum_def.name, layout.payload_words
        ));
    }

    if !out.is_empty() {
        out.push('\n');
    }

    out
}

fn emit_struct_definitions(
    program: &Program,
    enum_layouts: &HashMap<String, EnumLayout>,
) -> String {
    if program.structs.is_empty() {
        return String::new();
    }

    let mut out = String::new();
    for struct_def in &program.structs {
        let fields = struct_def
            .fields
            .iter()
            .map(|(_, ty)| llvm_type(ty, enum_layouts))
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!(
            "%struct.{} = type {{ {} }}\n",
            struct_def.name, fields
        ));
    }
    out.push('\n');
    out
}

fn collect_strings_from_stmts(
    stmts: &[Stmt],
    string_literals: &mut HashMap<String, StringLiteralData>,
    next_index: &mut usize,
) {
    for stmt in stmts {
        collect_strings_from_stmt(stmt, string_literals, next_index);
    }
}

fn collect_strings_from_stmt(
    stmt: &Stmt,
    string_literals: &mut HashMap<String, StringLiteralData>,
    next_index: &mut usize,
) {
    match stmt {
        Stmt::Let { value, .. } => collect_strings_from_expr(value, string_literals, next_index),
        Stmt::Assign { value, .. } => collect_strings_from_expr(value, string_literals, next_index),
        Stmt::AssignIndex { indices, value, .. } => {
            for index in indices {
                collect_strings_from_expr(index, string_literals, next_index);
            }
            collect_strings_from_expr(value, string_literals, next_index);
        }
        Stmt::AssignField { value, .. } => {
            collect_strings_from_expr(value, string_literals, next_index);
        }
        Stmt::AssignDeref { target, value } => {
            collect_strings_from_expr(target, string_literals, next_index);
            collect_strings_from_expr(value, string_literals, next_index);
        }
        Stmt::Expr(expr) => collect_strings_from_expr(expr, string_literals, next_index),
        Stmt::Break | Stmt::Continue => {}
        Stmt::Return(Some(expr)) => collect_strings_from_expr(expr, string_literals, next_index),
        Stmt::Return(None) => {}
        Stmt::If {
            condition,
            then_body,
            else_body,
        } => {
            collect_strings_from_expr(condition, string_literals, next_index);
            collect_strings_from_stmts(then_body, string_literals, next_index);
            if let Some(else_body) = else_body {
                collect_strings_from_stmts(else_body, string_literals, next_index);
            }
        }
        Stmt::While { condition, body } => {
            collect_strings_from_expr(condition, string_literals, next_index);
            collect_strings_from_stmts(body, string_literals, next_index);
        }
    }
}

fn collect_strings_from_expr(
    expr: &Expr,
    string_literals: &mut HashMap<String, StringLiteralData>,
    next_index: &mut usize,
) {
    match expr {
        Expr::Str(value) => {
            string_literals.entry(value.clone()).or_insert_with(|| {
                let data = StringLiteralData {
                    global_name: format!("@.str.user.{}", *next_index),
                    len: value.len() + 1,
                };
                *next_index += 1;
                data
            });
        }
        Expr::StructLiteral { fields, .. } => {
            for (_, value) in fields {
                collect_strings_from_expr(value, string_literals, next_index);
            }
        }
        Expr::ArrayLiteral(elements) => {
            for element in elements {
                collect_strings_from_expr(element, string_literals, next_index);
            }
        }
        Expr::FieldAccess { base, .. } => {
            collect_strings_from_expr(base, string_literals, next_index);
        }
        Expr::Index { base, index } => {
            collect_strings_from_expr(base, string_literals, next_index);
            collect_strings_from_expr(index, string_literals, next_index);
        }
        Expr::Call { args, .. } => {
            for arg in args {
                collect_strings_from_expr(arg, string_literals, next_index);
            }
        }
        Expr::Binary { left, right, .. } => {
            collect_strings_from_expr(left, string_literals, next_index);
            collect_strings_from_expr(right, string_literals, next_index);
        }
        Expr::Cast { expr, .. } => collect_strings_from_expr(expr, string_literals, next_index),
        Expr::Unary { expr, .. } => collect_strings_from_expr(expr, string_literals, next_index),
        Expr::Int(_) | Expr::Bool(_) | Expr::Var(_) | Expr::SizeOf(_) => {}
    }
}

fn emit_string_literal_globals(string_literals: &HashMap<String, StringLiteralData>) -> String {
    if string_literals.is_empty() {
        return String::new();
    }

    let mut entries = string_literals.iter().collect::<Vec<_>>();
    entries.sort_by(|(_, left), (_, right)| left.global_name.cmp(&right.global_name));

    let mut out = String::new();
    for (literal, data) in entries {
        out.push_str(&format!(
            "{} = private unnamed_addr constant [{} x i8] c\"{}\"\n",
            data.global_name,
            data.len,
            llvm_escape_string_literal(literal)
        ));
    }
    out.push('\n');
    out
}

#[cfg(test)]
mod tests;
