use std::collections::HashMap;

use crate::ast::{Expr, Program, Stmt, Type};

mod emitter;
mod runtime;
mod util;

use emitter::FunctionEmitter;
use runtime::{builtin_signatures, emit_runtime_prelude, runtime_declared_function};
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

pub fn emit_program(program: &Program) -> Result<String, String> {
    let struct_layouts = collect_struct_layouts(program);
    let function_sigs = collect_function_sigs(program);
    let string_literals = collect_string_literals(program);

    let mut out = String::new();
    out.push_str("; Monster LLVM IR backend\n");
    out.push_str(&format!("target triple = \"{}\"\n\n", host_target_triple()));
    out.push_str(&emit_runtime_prelude());
    out.push_str(&emit_struct_definitions(program));
    out.push_str(&emit_string_literal_globals(&string_literals));
    out.push_str(&emit_extern_declarations(program));

    for function in &program.functions {
        if function.is_extern {
            continue;
        }
        let mut emitter =
            FunctionEmitter::new(function, &function_sigs, &struct_layouts, &string_literals);
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

fn emit_extern_declarations(program: &Program) -> String {
    let mut out = String::new();

    for function in &program.functions {
        if !function.is_extern || runtime_declared_function(function.name.as_str()) {
            continue;
        }

        let params = function
            .params
            .iter()
            .map(|(_, ty)| llvm_type(ty))
            .collect::<Vec<_>>()
            .join(", ");

        out.push_str(&format!(
            "declare {} @{}({})\n",
            llvm_type(&function.ret_type),
            function.name,
            params
        ));
    }

    if !out.is_empty() {
        out.push('\n');
    }

    out
}

fn emit_struct_definitions(program: &Program) -> String {
    if program.structs.is_empty() {
        return String::new();
    }

    let mut out = String::new();
    for struct_def in &program.structs {
        let fields = struct_def
            .fields
            .iter()
            .map(|(_, ty)| llvm_type(ty))
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
        Expr::Unary { expr, .. } => collect_strings_from_expr(expr, string_literals, next_index),
        Expr::Int(_) | Expr::Bool(_) | Expr::Var(_) => {}
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
