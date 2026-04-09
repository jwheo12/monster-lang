use std::collections::HashMap;

use crate::ast::{BinOp, Expr, Function, Program, Stmt, Type, UnaryOp};

pub fn analyze_program(program: &Program) -> Result<(), String> {
    let mut analyzer = Analyzer::new();
    analyzer.analyze_program(program)
}

#[derive(Clone)]
struct FunctionSig {
    params: Vec<Type>,
    ret_type: Type,
}

#[derive(Clone)]
struct VarInfo {
    ty: Type,
    mutable: bool,
}

#[derive(Clone)]
struct StructInfo {
    fields: Vec<(String, Type)>,
}

struct Analyzer {
    structs: HashMap<String, StructInfo>,
    functions: HashMap<String, FunctionSig>,
    scopes: Vec<HashMap<String, VarInfo>>,
    current_return_type: Option<Type>,
    loop_depth: usize,
}

impl Analyzer {
    fn new() -> Self {
        let mut analyzer = Self {
            structs: HashMap::new(),
            functions: HashMap::new(),
            scopes: Vec::new(),
            current_return_type: None,
            loop_depth: 0,
        };
        analyzer.install_builtins();
        analyzer
    }

    fn analyze_program(&mut self, program: &Program) -> Result<(), String> {
        for struct_def in &program.structs {
            if self
                .structs
                .insert(struct_def.name.clone(), StructInfo { fields: Vec::new() })
                .is_some()
            {
                return Err(format!("duplicate struct '{}'", struct_def.name));
            }
        }

        for struct_def in &program.structs {
            let mut seen_fields = HashMap::new();
            for (field_name, field_ty) in &struct_def.fields {
                self.ensure_value_type(field_ty, "struct field")?;

                if seen_fields.insert(field_name.clone(), ()).is_some() {
                    return Err(format!(
                        "duplicate field '{}' in struct '{}'",
                        field_name, struct_def.name
                    ));
                }
            }

            self.structs.insert(
                struct_def.name.clone(),
                StructInfo {
                    fields: struct_def.fields.clone(),
                },
            );
        }

        for func in &program.functions {
            for (_, ty) in &func.params {
                self.ensure_value_type(ty, "function parameter")?;
            }
            self.ensure_known_type(&func.ret_type)?;

            let sig = FunctionSig {
                params: func.params.iter().map(|(_, ty)| ty.clone()).collect(),
                ret_type: func.ret_type.clone(),
            };

            if self.functions.insert(func.name.clone(), sig).is_some() {
                return Err(format!("duplicate function '{}'", func.name));
            }
        }

        for func in &program.functions {
            self.analyze_function(func)?;
        }

        Ok(())
    }

    fn analyze_function(&mut self, func: &Function) -> Result<(), String> {
        let Some(body) = func.body.as_ref() else {
            return Ok(());
        };

        self.scopes.clear();
        self.current_return_type = Some(func.ret_type.clone());
        self.loop_depth = 0;
        self.enter_scope();

        for (name, ty) in &func.params {
            self.declare_var(name, ty.clone(), false)?;
        }

        for stmt in body {
            self.analyze_stmt(stmt)?;
        }

        self.exit_scope();
        self.current_return_type = None;
        Ok(())
    }

    fn analyze_stmt(&mut self, stmt: &Stmt) -> Result<(), String> {
        match stmt {
            Stmt::Let {
                name,
                ty,
                mutable,
                value,
            } => {
                self.ensure_value_type(ty, "local variable")?;
                let value_ty = self.analyze_expr(value)?;
                self.expect_type(&value_ty, ty, &format!("initializer for '{name}'"))?;
                self.declare_var(name, ty.clone(), *mutable)?;
                Ok(())
            }
            Stmt::Assign { name, value } => {
                let var = self
                    .lookup_var(name)
                    .cloned()
                    .ok_or_else(|| format!("cannot assign to undeclared variable '{name}'"))?;

                if !var.mutable {
                    return Err(format!("cannot assign to immutable variable '{name}'"));
                }

                let value_ty = self.analyze_expr(value)?;
                self.expect_type(&value_ty, &var.ty, &format!("assignment to '{name}'"))
            }
            Stmt::AssignIndex {
                name,
                indices,
                value,
            } => {
                let var = self
                    .lookup_var(name)
                    .cloned()
                    .ok_or_else(|| format!("cannot assign to undeclared variable '{name}'"))?;

                if !var.mutable && !matches!(var.ty, Type::Ptr(_)) {
                    return Err(format!("cannot assign to immutable variable '{name}'"));
                }

                let mut current_ty = var.ty;
                for index in indices {
                    let index_ty = self.analyze_expr(index)?;
                    self.expect_index_type(&index_ty, "array index")?;

                    current_ty = match current_ty {
                        Type::Array(element_ty, _)
                        | Type::Slice(element_ty)
                        | Type::Ptr(element_ty) => *element_ty,
                        _ => {
                            return Err(format!("cannot index-assign non-array variable '{name}'"));
                        }
                    };
                }

                let value_ty = self.analyze_expr(value)?;
                self.expect_type(
                    &value_ty,
                    &current_ty,
                    &format!("assignment to '{name}[...]'"),
                )
            }
            Stmt::AssignField {
                name,
                fields,
                value,
            } => {
                let var = self
                    .lookup_var(name)
                    .cloned()
                    .ok_or_else(|| format!("cannot assign to undeclared variable '{name}'"))?;

                if !var.mutable {
                    return Err(format!("cannot assign to immutable variable '{name}'"));
                }

                let mut current_ty = var.ty;
                for field in fields {
                    let Type::Named(struct_name) = current_ty else {
                        return Err(format!("cannot field-assign non-struct variable '{name}'"));
                    };

                    let struct_info = self
                        .structs
                        .get(&struct_name)
                        .ok_or_else(|| format!("unknown struct '{}'", struct_name))?;

                    current_ty = struct_info
                        .fields
                        .iter()
                        .find_map(|(name, ty)| (name == field).then_some(ty.clone()))
                        .ok_or_else(|| {
                            format!("struct '{}' has no field '{}'", struct_name, field)
                        })?;
                }

                let value_ty = self.analyze_expr(value)?;
                self.expect_type(
                    &value_ty,
                    &current_ty,
                    &format!("assignment to '{name}.{}'", fields.join(".")),
                )
            }
            Stmt::AssignDeref { target, value } => {
                let target_ty = self.analyze_place_expr(target)?;
                let value_ty = self.analyze_expr(value)?;
                self.expect_type(&value_ty, &target_ty, "dereference assignment")
            }
            Stmt::Expr(expr) => {
                let _ = self.analyze_expr(expr)?;
                Ok(())
            }
            Stmt::If {
                condition,
                then_body,
                else_body,
            } => {
                let cond_ty = self.analyze_expr(condition)?;
                self.expect_type(&cond_ty, &Type::Bool, "if condition")?;

                self.analyze_nested_block(then_body)?;
                if let Some(else_body) = else_body {
                    self.analyze_nested_block(else_body)?;
                }

                Ok(())
            }
            Stmt::While { condition, body } => {
                let cond_ty = self.analyze_expr(condition)?;
                self.expect_type(&cond_ty, &Type::Bool, "while condition")?;
                self.loop_depth += 1;
                let result = self.analyze_nested_block(body);
                self.loop_depth -= 1;
                result
            }
            Stmt::Break => {
                if self.loop_depth == 0 {
                    Err("break used outside of loop".to_string())
                } else {
                    Ok(())
                }
            }
            Stmt::Continue => {
                if self.loop_depth == 0 {
                    Err("continue used outside of loop".to_string())
                } else {
                    Ok(())
                }
            }
            Stmt::Return(expr) => {
                let expected = self
                    .current_return_type
                    .clone()
                    .ok_or_else(|| "return used outside of function".to_string())?;

                match expr {
                    Some(expr) => {
                        if expected == Type::Void {
                            return Err("void function cannot return a value".to_string());
                        }

                        let actual = self.analyze_expr(expr)?;
                        self.expect_type(&actual, &expected, "return value")
                    }
                    None => {
                        if expected == Type::Void {
                            Ok(())
                        } else {
                            Err(format!(
                                "non-void function must return a {} value",
                                type_name(&expected)
                            ))
                        }
                    }
                }
            }
        }
    }

    fn analyze_nested_block(&mut self, stmts: &[Stmt]) -> Result<(), String> {
        self.enter_scope();

        for stmt in stmts {
            self.analyze_stmt(stmt)?;
        }

        self.exit_scope();
        Ok(())
    }

    fn analyze_expr(&mut self, expr: &Expr) -> Result<Type, String> {
        match expr {
            Expr::Int(_) => Ok(Type::I32),
            Expr::Cast { expr, ty } => {
                self.ensure_value_type(ty, "cast target")?;
                let expr_ty = self.analyze_expr(expr)?;

                if can_cast(&expr_ty, ty) {
                    Ok(ty.clone())
                } else {
                    Err(format!(
                        "cannot cast {} to {}",
                        type_name(&expr_ty),
                        type_name(ty)
                    ))
                }
            }
            Expr::Bool(_) => Ok(Type::Bool),
            Expr::Str(_) => Ok(Type::Str),
            Expr::Var(name) => self
                .lookup_var(name)
                .map(|var| var.ty.clone())
                .ok_or_else(|| format!("use of undeclared variable '{name}'")),
            Expr::ArrayLiteral(elements) => {
                let Some(first) = elements.first() else {
                    return Err("array literals cannot be empty yet".to_string());
                };

                let element_ty = self.analyze_expr(first)?;
                self.ensure_value_type(&element_ty, "array element")?;

                for (index, element) in elements.iter().enumerate().skip(1) {
                    let actual_ty = self.analyze_expr(element)?;
                    self.expect_type(
                        &actual_ty,
                        &element_ty,
                        &format!("array element {}", index + 1),
                    )?;
                }

                Ok(Type::Array(Box::new(element_ty), elements.len()))
            }
            Expr::StructLiteral { name, fields } => {
                let struct_info = self
                    .structs
                    .get(name)
                    .cloned()
                    .ok_or_else(|| format!("unknown struct '{name}'"))?;

                let mut seen_fields = HashMap::new();
                for (field_name, value) in fields {
                    let expected_ty = struct_info
                        .fields
                        .iter()
                        .find_map(|(name, ty)| (name == field_name).then_some(ty))
                        .ok_or_else(|| {
                            format!("struct '{}' has no field '{}'", name, field_name)
                        })?;

                    if seen_fields.insert(field_name.clone(), ()).is_some() {
                        return Err(format!(
                            "duplicate field '{}' in struct literal '{}'",
                            field_name, name
                        ));
                    }

                    let actual_ty = self.analyze_expr(value)?;
                    self.expect_type(
                        &actual_ty,
                        expected_ty,
                        &format!("field '{}' of '{}'", field_name, name),
                    )?;
                }

                for (field_name, _) in &struct_info.fields {
                    if !seen_fields.contains_key(field_name) {
                        return Err(format!(
                            "missing field '{}' in struct literal '{}'",
                            field_name, name
                        ));
                    }
                }

                Ok(Type::Named(name.clone()))
            }
            Expr::Index { base, index } => {
                let base_ty = self.analyze_expr(base)?;
                let index_ty = self.analyze_expr(index)?;
                self.expect_index_type(&index_ty, "array index")?;

                let element_ty = match base_ty {
                    Type::Array(element_ty, _) | Type::Slice(element_ty) => element_ty,
                    Type::Ptr(element_ty) => element_ty,
                    _ => {
                        return Err(
                            "indexing requires an array, slice, or pointer value".to_string()
                        );
                    }
                };

                Ok(*element_ty)
            }
            Expr::FieldAccess { base, field } => {
                let base_ty = self.analyze_expr(base)?;
                let Type::Named(struct_name) = base_ty else {
                    return Err(format!("field access '.{}' requires a struct value", field));
                };

                let struct_info = self
                    .structs
                    .get(&struct_name)
                    .ok_or_else(|| format!("unknown struct '{}'", struct_name))?;

                struct_info
                    .fields
                    .iter()
                    .find_map(|(name, ty)| (name == field).then_some(ty.clone()))
                    .ok_or_else(|| format!("struct '{}' has no field '{}'", struct_name, field))
            }
            Expr::Call { name, args } => {
                if name == "len" {
                    return self.analyze_len_call(args);
                }
                if name == "slice" {
                    return self.analyze_slice_call(args);
                }

                let sig = self
                    .functions
                    .get(name)
                    .cloned()
                    .ok_or_else(|| format!("call to unknown function '{name}'"))?;

                if args.len() != sig.params.len() {
                    return Err(format!(
                        "function '{name}' expects {} args, got {}",
                        sig.params.len(),
                        args.len()
                    ));
                }

                for (index, (arg, expected_ty)) in args.iter().zip(sig.params.iter()).enumerate() {
                    let actual_ty = self.analyze_expr(arg)?;
                    self.expect_type(
                        &actual_ty,
                        expected_ty,
                        &format!("argument {} of '{name}'", index + 1),
                    )?;
                }

                Ok(sig.ret_type)
            }
            Expr::Binary { op, left, right } => {
                let left_ty = self.analyze_expr(left)?;
                let right_ty = self.analyze_expr(right)?;

                match op {
                    BinOp::Or | BinOp::And => {
                        self.expect_type(&left_ty, &Type::Bool, "left-hand logical operand")?;
                        self.expect_type(&right_ty, &Type::Bool, "right-hand logical operand")?;
                        Ok(Type::Bool)
                    }
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => {
                        self.expect_integer_type(&left_ty, "left-hand arithmetic operand")?;
                        self.expect_type(&right_ty, &left_ty, "right-hand arithmetic operand")?;
                        Ok(left_ty)
                    }
                    BinOp::Eq | BinOp::Ne => {
                        if matches!(left_ty, Type::Named(_) | Type::Array(_, _) | Type::Slice(_)) {
                            return Err(
                                "aggregate values cannot be compared with == or !=".to_string()
                            );
                        }
                        self.expect_type(&right_ty, &left_ty, "comparison operand")?;
                        Ok(Type::Bool)
                    }
                    BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                        self.expect_integer_type(&left_ty, "left-hand comparison operand")?;
                        self.expect_type(&right_ty, &left_ty, "right-hand comparison operand")?;
                        Ok(Type::Bool)
                    }
                }
            }
            Expr::Unary { op, expr } => match op {
                UnaryOp::Neg => {
                    let expr_ty = self.analyze_expr(expr)?;
                    self.expect_type(&expr_ty, &Type::I32, "unary '-' operand")?;
                    Ok(Type::I32)
                }
                UnaryOp::Not => {
                    let expr_ty = self.analyze_expr(expr)?;
                    self.expect_type(&expr_ty, &Type::Bool, "unary '!' operand")?;
                    Ok(Type::Bool)
                }
                UnaryOp::AddrOf => {
                    let pointee_ty = self.analyze_place_expr(expr)?;
                    Ok(Type::Ptr(Box::new(pointee_ty)))
                }
                UnaryOp::Deref => {
                    let expr_ty = self.analyze_expr(expr)?;
                    let Type::Ptr(inner_ty) = expr_ty else {
                        return Err("unary '*' requires a pointer value".to_string());
                    };
                    Ok(*inner_ty)
                }
            },
        }
    }

    fn analyze_place_expr(&mut self, expr: &Expr) -> Result<Type, String> {
        match expr {
            Expr::Var(name) => self
                .lookup_var(name)
                .map(|var| var.ty.clone())
                .ok_or_else(|| format!("use of undeclared variable '{name}'")),
            Expr::FieldAccess { base, field } => {
                let base_ty = self.analyze_place_expr(base)?;
                let Type::Named(struct_name) = base_ty else {
                    return Err(format!("field access '.{}' requires a struct value", field));
                };

                let struct_info = self
                    .structs
                    .get(&struct_name)
                    .ok_or_else(|| format!("unknown struct '{}'", struct_name))?;

                struct_info
                    .fields
                    .iter()
                    .find_map(|(name, ty)| (name == field).then_some(ty.clone()))
                    .ok_or_else(|| format!("struct '{}' has no field '{}'", struct_name, field))
            }
            Expr::Index { base, index } => {
                let base_ty = self.analyze_expr(base)?;
                let index_ty = self.analyze_expr(index)?;
                self.expect_index_type(&index_ty, "array index")?;

                match base_ty {
                    Type::Array(element_ty, _)
                    | Type::Slice(element_ty)
                    | Type::Ptr(element_ty) => Ok(*element_ty),
                    _ => Err("indexing requires an array, slice, or pointer value".to_string()),
                }
            }
            Expr::Unary {
                op: UnaryOp::Deref,
                expr,
            } => {
                let ptr_ty = self.analyze_expr(expr)?;
                let Type::Ptr(inner_ty) = ptr_ty else {
                    return Err("unary '*' requires a pointer value".to_string());
                };
                Ok(*inner_ty)
            }
            _ => Err("expression is not addressable".to_string()),
        }
    }

    fn declare_var(&mut self, name: &str, ty: Type, mutable: bool) -> Result<(), String> {
        let scope = self
            .scopes
            .last_mut()
            .ok_or_else(|| "internal error: no active scope".to_string())?;

        if scope.contains_key(name) {
            return Err(format!("duplicate declaration of '{name}'"));
        }

        scope.insert(name.to_string(), VarInfo { ty, mutable });
        Ok(())
    }

    fn lookup_var(&self, name: &str) -> Option<&VarInfo> {
        self.scopes.iter().rev().find_map(|scope| scope.get(name))
    }

    fn expect_type(&self, actual: &Type, expected: &Type, context: &str) -> Result<(), String> {
        if actual == expected {
            Ok(())
        } else {
            Err(format!(
                "{context} has type {}, expected {}",
                type_name(actual),
                type_name(expected)
            ))
        }
    }

    fn expect_integer_type(&self, ty: &Type, context: &str) -> Result<(), String> {
        if is_integer_type(ty) {
            Ok(())
        } else {
            Err(format!(
                "{context} has type {}, expected integer",
                type_name(ty)
            ))
        }
    }

    fn expect_index_type(&self, ty: &Type, context: &str) -> Result<(), String> {
        if is_index_type(ty) {
            Ok(())
        } else {
            Err(format!(
                "{context} has type {}, expected i32 or usize",
                type_name(ty)
            ))
        }
    }

    fn ensure_known_type(&self, ty: &Type) -> Result<(), String> {
        match ty {
            Type::I32 | Type::U8 | Type::USize | Type::Bool | Type::Str | Type::Void => Ok(()),
            Type::Array(element_ty, _) => self.ensure_value_type(element_ty, "array element"),
            Type::Slice(element_ty) => self.ensure_value_type(element_ty, "slice element"),
            Type::Ptr(element_ty) => self.ensure_known_type(element_ty),
            Type::Named(name) => {
                if self.structs.contains_key(name) {
                    Ok(())
                } else {
                    Err(format!("unknown type '{}'", name))
                }
            }
        }
    }

    fn ensure_value_type(&self, ty: &Type, context: &str) -> Result<(), String> {
        self.ensure_known_type(ty)?;

        if *ty == Type::Void {
            Err(format!("{context} cannot have type void"))
        } else {
            Ok(())
        }
    }

    fn enter_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn exit_scope(&mut self) {
        self.scopes.pop();
    }

    fn install_builtins(&mut self) {
        self.functions.insert(
            "print_i32".to_string(),
            FunctionSig {
                params: vec![Type::I32],
                ret_type: Type::Void,
            },
        );
        self.functions.insert(
            "print_bool".to_string(),
            FunctionSig {
                params: vec![Type::Bool],
                ret_type: Type::Void,
            },
        );
        self.functions.insert(
            "print_str".to_string(),
            FunctionSig {
                params: vec![Type::Str],
                ret_type: Type::Void,
            },
        );
        self.functions.insert(
            "print_ln_i32".to_string(),
            FunctionSig {
                params: vec![Type::I32],
                ret_type: Type::Void,
            },
        );
        self.functions.insert(
            "print_ln_bool".to_string(),
            FunctionSig {
                params: vec![Type::Bool],
                ret_type: Type::Void,
            },
        );
        self.functions.insert(
            "print_ln_str".to_string(),
            FunctionSig {
                params: vec![Type::Str],
                ret_type: Type::Void,
            },
        );
        self.functions.insert(
            "read_i32".to_string(),
            FunctionSig {
                params: vec![],
                ret_type: Type::I32,
            },
        );
    }

    fn analyze_len_call(&mut self, args: &[Expr]) -> Result<Type, String> {
        if args.len() != 1 {
            return Err(format!("function 'len' expects 1 args, got {}", args.len()));
        }

        let value_ty = self.analyze_expr(&args[0])?;
        match value_ty {
            Type::Array(_, _) | Type::Slice(_) => Ok(Type::USize),
            _ => Err(format!(
                "argument 1 of 'len' has type {}, expected array or slice",
                type_name(&value_ty)
            )),
        }
    }

    fn analyze_slice_call(&mut self, args: &[Expr]) -> Result<Type, String> {
        if args.len() != 1 {
            return Err(format!(
                "function 'slice' expects 1 args, got {}",
                args.len()
            ));
        }

        let value_ty = self.analyze_expr(&args[0])?;
        match value_ty {
            Type::Array(element_ty, _) => Ok(Type::Slice(element_ty)),
            Type::Slice(element_ty) => Ok(Type::Slice(element_ty)),
            _ => Err(format!(
                "argument 1 of 'slice' has type {}, expected array",
                type_name(&value_ty)
            )),
        }
    }
}

fn type_name(ty: &Type) -> String {
    match ty {
        Type::I32 => "i32".to_string(),
        Type::U8 => "u8".to_string(),
        Type::USize => "usize".to_string(),
        Type::Bool => "bool".to_string(),
        Type::Str => "str".to_string(),
        Type::Void => "void".to_string(),
        Type::Named(name) => name.clone(),
        Type::Array(element_ty, len) => format!("[{}; {}]", type_name(element_ty), len),
        Type::Slice(element_ty) => format!("[{}]", type_name(element_ty)),
        Type::Ptr(element_ty) => format!("*{}", type_name(element_ty)),
    }
}

fn is_integer_type(ty: &Type) -> bool {
    matches!(ty, Type::I32 | Type::U8 | Type::USize)
}

fn is_index_type(ty: &Type) -> bool {
    matches!(ty, Type::I32 | Type::USize)
}

fn can_cast(from: &Type, to: &Type) -> bool {
    if from == to {
        return true;
    }

    if is_integer_type(from) && is_integer_type(to) {
        return true;
    }

    if matches!(from, Type::Bool) && is_integer_type(to) {
        return true;
    }

    if is_integer_type(from) && matches!(to, Type::Bool) {
        return true;
    }

    if matches!(from, Type::Ptr(_)) && matches!(to, Type::Ptr(_)) {
        return true;
    }

    if (matches!(from, Type::Ptr(_)) && matches!(to, Type::USize))
        || (matches!(from, Type::USize) && matches!(to, Type::Ptr(_)))
    {
        return true;
    }

    if (matches!(from, Type::Str) && matches!(to, Type::Ptr(inner) if **inner == Type::U8))
        || (matches!(from, Type::Ptr(inner) if **inner == Type::U8) && matches!(to, Type::Str))
    {
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::analyze_program;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn analyze_source(source: &str) -> Result<(), String> {
        let mut lexer = Lexer::new(source);
        let tokens = lexer.tokenize().expect("tokenize should succeed");
        let mut parser = Parser::new(tokens);
        let program = parser.parse_program().expect("parse should succeed");
        analyze_program(&program)
    }

    #[test]
    fn rejects_assignment_to_immutable_variable() {
        let result = analyze_source(
            r#"
            fn main() -> i32 {
                let x: i32 = 1;
                x = 2;
                return x;
            }
            "#,
        );

        assert!(matches!(
            result,
            Err(message) if message.contains("immutable variable 'x'")
        ));
    }

    #[test]
    fn accepts_mutable_loop_state() {
        let result = analyze_source(
            r#"
            fn main() -> i32 {
                let mut i: i32 = 0;

                while i < 3 {
                    i = i + 1;
                }

                return i;
            }
            "#,
        );

        assert!(result.is_ok());
    }

    #[test]
    fn accepts_break_and_continue_inside_loop() {
        let result = analyze_source(
            r#"
            fn main() -> i32 {
                let mut i: i32 = 0;

                while i < 5 {
                    if i == 2 {
                        break;
                    }

                    i = i + 1;
                    continue;
                }

                return i;
            }
            "#,
        );

        assert!(result.is_ok());
    }

    #[test]
    fn rejects_break_outside_loop() {
        let result = analyze_source(
            r#"
            fn main() -> i32 {
                break;
                return 0;
            }
            "#,
        );

        assert!(matches!(
            result,
            Err(message) if message.contains("break used outside of loop")
        ));
    }

    #[test]
    fn rejects_continue_outside_loop() {
        let result = analyze_source(
            r#"
            fn main() -> i32 {
                continue;
                return 0;
            }
            "#,
        );

        assert!(matches!(
            result,
            Err(message) if message.contains("continue used outside of loop")
        ));
    }

    #[test]
    fn accepts_logical_and_unary_expressions() {
        let result = analyze_source(
            r#"
            fn main() -> bool {
                return !false || (-1 < 0 && true);
            }
            "#,
        );

        assert!(result.is_ok());
    }

    #[test]
    fn rejects_non_bool_logical_operands() {
        let result = analyze_source(
            r#"
            fn main() -> bool {
                return 1 && true;
            }
            "#,
        );

        assert!(matches!(
            result,
            Err(message) if message.contains("logical operand")
        ));
    }

    #[test]
    fn accepts_builtin_print_calls() {
        let result = analyze_source(
            r#"
            fn main() -> i32 {
                print_i32(42);
                print_bool(true);
                print_str("Hello, World!");
                print_ln_i32(42);
                print_ln_bool(true);
                print_ln_str("Hello, World!");
                return 0;
            }
            "#,
        );

        assert!(result.is_ok());
    }

    #[test]
    fn rejects_wrong_builtin_argument_type() {
        let result = analyze_source(
            r#"
            fn main() -> i32 {
                print_str(1);
                return 0;
            }
            "#,
        );

        assert!(matches!(
            result,
            Err(message) if message.contains("argument 1 of 'print_str'")
        ));
    }

    #[test]
    fn accepts_builtin_read_call() {
        let result = analyze_source(
            r#"
            fn main() -> i32 {
                let x: i32 = read_i32();
                print_ln_i32(x);
                return 0;
            }
            "#,
        );

        assert!(result.is_ok());
    }

    #[test]
    fn rejects_builtin_read_with_arguments() {
        let result = analyze_source(
            r#"
            fn main() -> i32 {
                return read_i32(1);
            }
            "#,
        );

        assert!(matches!(
            result,
            Err(message) if message.contains("function 'read_i32' expects 0 args, got 1")
        ));
    }

    #[test]
    fn accepts_void_function_with_bare_return() {
        let result = analyze_source(
            r#"
            fn log_message() -> void {
                print_ln_str("Hello");
                return;
            }

            fn main() -> i32 {
                log_message();
                return 0;
            }
            "#,
        );

        assert!(result.is_ok());
    }

    #[test]
    fn rejects_bare_return_in_non_void_function() {
        let result = analyze_source(
            r#"
            fn main() -> i32 {
                return;
            }
            "#,
        );

        assert!(matches!(
            result,
            Err(message) if message.contains("must return a i32 value")
        ));
    }

    #[test]
    fn rejects_value_return_in_void_function() {
        let result = analyze_source(
            r#"
            fn log_message() -> void {
                return 1;
            }

            fn main() -> i32 {
                log_message();
                return 0;
            }
            "#,
        );

        assert!(matches!(
            result,
            Err(message) if message.contains("void function cannot return a value")
        ));
    }

    #[test]
    fn accepts_calls_to_extern_functions() {
        let result = analyze_source(
            r#"
            extern fn abs(value: i32) -> i32;

            fn main() -> i32 {
                return abs(-7);
            }
            "#,
        );

        assert!(result.is_ok());
    }

    #[test]
    fn accepts_struct_literals_and_field_access() {
        let result = analyze_source(
            r#"
            struct Pair {
                left: i32,
                right: i32,
            }

            fn main() -> i32 {
                let pair: Pair = Pair { left: 10, right: 20 };
                return pair.left + pair.right;
            }
            "#,
        );

        assert!(result.is_ok());
    }

    #[test]
    fn rejects_missing_struct_field() {
        let result = analyze_source(
            r#"
            struct Pair {
                left: i32,
                right: i32,
            }

            fn main() -> i32 {
                let pair: Pair = Pair { left: 10 };
                return pair.left;
            }
            "#,
        );

        assert!(matches!(
            result,
            Err(message) if message.contains("missing field 'right'")
        ));
    }

    #[test]
    fn accepts_nested_struct_field_assignment() {
        let result = analyze_source(
            r#"
            struct Inner {
                value: i32,
            }

            struct Pair {
                inner: Inner,
                right: i32,
            }

            fn main() -> i32 {
                let mut pair: Pair = Pair { inner: Inner { value: 10 }, right: 20 };
                pair.inner.value = 42;
                return pair.inner.value;
            }
            "#,
        );

        assert!(result.is_ok());
    }

    #[test]
    fn rejects_field_assignment_to_immutable_struct() {
        let result = analyze_source(
            r#"
            struct Pair {
                left: i32,
                right: i32,
            }

            fn main() -> i32 {
                let pair: Pair = Pair { left: 10, right: 20 };
                pair.left = 42;
                return pair.left;
            }
            "#,
        );

        assert!(matches!(
            result,
            Err(message) if message.contains("immutable variable 'pair'")
        ));
    }

    #[test]
    fn accepts_array_literals_and_indexing() {
        let result = analyze_source(
            r#"
            fn main() -> i32 {
                let values: [i32; 3] = [10, 20, 30];
                return values[1];
            }
            "#,
        );

        assert!(result.is_ok());
    }

    #[test]
    fn accepts_slice_params_and_len() {
        let result = analyze_source(
            r#"
            fn head(values: [i32]) -> i32 {
                return values[0] + (len(values) as i32);
            }

            fn main() -> i32 {
                let values: [i32; 3] = [10, 20, 30];
                return head(slice(values));
            }
            "#,
        );

        assert!(result.is_ok());
    }

    #[test]
    fn rejects_non_integer_array_index() {
        let result = analyze_source(
            r#"
            fn main() -> i32 {
                let values: [i32; 3] = [10, 20, 30];
                return values[true];
            }
            "#,
        );

        assert!(matches!(
            result,
            Err(message) if message.contains("array index")
        ));
    }

    #[test]
    fn accepts_array_index_assignment_and_len() {
        let result = analyze_source(
            r#"
            fn main() -> i32 {
                let mut values: [i32; 3] = [10, 20, 30];
                values[1] = 99;
                return len(values) as i32;
            }
            "#,
        );

        assert!(result.is_ok());
    }

    #[test]
    fn accepts_u8_usize_arithmetic_and_casts() {
        let result = analyze_source(
            r#"
            fn main() -> i32 {
                let byte: u8 = 255 as u8;
                let size: usize = len([1, 2, 3]);
                let total: usize = (byte as usize) + size;
                return total as i32;
            }
            "#,
        );

        assert!(result.is_ok());
    }

    #[test]
    fn rejects_invalid_scalar_cast() {
        let result = analyze_source(
            r#"
            struct Pair {
                left: i32,
                right: i32,
            }

            fn main() -> i32 {
                let pair: Pair = Pair { left: 1, right: 2 };
                return pair as i32;
            }
            "#,
        );

        assert!(matches!(
            result,
            Err(message) if message.contains("cannot cast Pair to i32")
        ));
    }

    #[test]
    fn rejects_index_assignment_to_immutable_array() {
        let result = analyze_source(
            r#"
            fn main() -> i32 {
                let values: [i32; 3] = [10, 20, 30];
                values[1] = 99;
                return 0;
            }
            "#,
        );

        assert!(matches!(
            result,
            Err(message) if message.contains("immutable variable 'values'")
        ));
    }

    #[test]
    fn rejects_len_on_non_array() {
        let result = analyze_source(
            r#"
            fn main() -> i32 {
                return len(123);
            }
            "#,
        );

        assert!(matches!(
            result,
            Err(message) if message.contains("expected array or slice")
        ));
    }

    #[test]
    fn rejects_slice_on_non_array() {
        let result = analyze_source(
            r#"
            fn main() -> i32 {
                return len(slice(123));
            }
            "#,
        );

        assert!(matches!(
            result,
            Err(message) if message.contains("argument 1 of 'slice'")
        ));
    }

    #[test]
    fn accepts_pointer_address_deref_and_indexing() {
        let result = analyze_source(
            r#"
            fn main() -> i32 {
                let mut values: [i32; 3] = [10, 20, 30];
                let p: *i32 = &values[0];
                p[1] = 99;
                *p = 42;
                return values[0] + values[1];
            }
            "#,
        );

        assert!(result.is_ok());
    }

    #[test]
    fn rejects_deref_on_non_pointer() {
        let result = analyze_source(
            r#"
            fn main() -> i32 {
                return *1;
            }
            "#,
        );

        assert!(matches!(
            result,
            Err(message) if message.contains("pointer value")
        ));
    }

    #[test]
    fn accepts_nested_array_index_assignment() {
        let result = analyze_source(
            r#"
            fn main() -> i32 {
                let mut matrix: [[i32; 2]; 2] = [[1, 2], [3, 4]];
                matrix[1][0] = 99;
                return matrix[1][0];
            }
            "#,
        );

        assert!(result.is_ok());
    }

    #[test]
    fn rejects_too_many_indices_in_array_assignment() {
        let result = analyze_source(
            r#"
            fn main() -> i32 {
                let mut values: [i32; 2] = [1, 2];
                values[0][0] = 99;
                return 0;
            }
            "#,
        );

        assert!(matches!(
            result,
            Err(message) if message.contains("cannot index-assign non-array variable 'values'")
        ));
    }
}
