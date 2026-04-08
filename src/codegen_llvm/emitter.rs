use std::collections::HashMap;

use crate::ast::{BinOp, Expr, Function, Stmt, Type, UnaryOp};

use super::{
    FunctionSig, StringLiteralData, StructLayout, runtime::llvm_function_name, util::llvm_type,
};

#[derive(Clone)]
struct LocalVar {
    ptr: String,
    ty: Type,
}

#[derive(Clone)]
struct Value {
    repr: String,
    ty: Type,
}

#[derive(Clone)]
struct Place {
    ptr: String,
    ty: Type,
}

pub(super) struct FunctionEmitter<'a> {
    function: &'a Function,
    function_sigs: &'a HashMap<String, FunctionSig>,
    struct_layouts: &'a HashMap<String, StructLayout>,
    string_literals: &'a HashMap<String, StringLiteralData>,
    entry_allocas: Vec<String>,
    body_lines: Vec<String>,
    scopes: Vec<HashMap<String, LocalVar>>,
    temp_counter: usize,
    label_counter: usize,
    slot_counter: usize,
    current_block: String,
    terminated: bool,
}

impl<'a> FunctionEmitter<'a> {
    pub(super) fn new(
        function: &'a Function,
        function_sigs: &'a HashMap<String, FunctionSig>,
        struct_layouts: &'a HashMap<String, StructLayout>,
        string_literals: &'a HashMap<String, StringLiteralData>,
    ) -> Self {
        Self {
            function,
            function_sigs,
            struct_layouts,
            string_literals,
            entry_allocas: Vec::new(),
            body_lines: Vec::new(),
            scopes: Vec::new(),
            temp_counter: 0,
            label_counter: 0,
            slot_counter: 0,
            current_block: String::new(),
            terminated: false,
        }
    }

    pub(super) fn emit(&mut self) -> Result<String, String> {
        let body = self.function.body.as_ref().ok_or_else(|| {
            format!(
                "internal error: extern function '{}' has no body",
                self.function.name
            )
        })?;

        self.start_block("entry");
        self.enter_scope();

        for (name, ty) in &self.function.params {
            let ptr = self.create_stack_slot(name, ty);
            self.declare_local(name, ty.clone(), ptr.clone())?;
            self.emit_line(format!("store {} %{}, ptr {}", llvm_type(ty), name, ptr));
        }

        self.emit_stmts(body, false)?;
        self.exit_scope();

        if !self.terminated {
            self.emit_default_return();
        }

        let params = self
            .function
            .params
            .iter()
            .map(|(name, ty)| format!("{} %{}", llvm_type(ty), name))
            .collect::<Vec<_>>()
            .join(", ");

        let mut out = String::new();
        out.push_str(&format!(
            "define {} @{}({}) {{\n",
            llvm_type(&self.function.ret_type),
            self.function.name,
            params
        ));

        for (idx, line) in self.body_lines.iter().enumerate() {
            out.push_str(line);
            out.push('\n');

            if idx == 0 {
                for alloca in &self.entry_allocas {
                    out.push_str("  ");
                    out.push_str(alloca);
                    out.push('\n');
                }
            }
        }

        out.push_str("}\n");
        Ok(out)
    }

    fn emit_stmts(&mut self, stmts: &[Stmt], new_scope: bool) -> Result<(), String> {
        if new_scope {
            self.enter_scope();
        }

        for stmt in stmts {
            if self.terminated {
                break;
            }
            self.emit_stmt(stmt)?;
        }

        if new_scope {
            self.exit_scope();
        }

        Ok(())
    }

    fn emit_stmt(&mut self, stmt: &Stmt) -> Result<(), String> {
        match stmt {
            Stmt::Let {
                name,
                ty,
                mutable: _,
                value,
            } => {
                let value = self.emit_expr(value)?;
                let ptr = self.create_stack_slot(name, ty);
                self.declare_local(name, ty.clone(), ptr.clone())?;
                self.emit_store(&value, &ptr);
                Ok(())
            }
            Stmt::Assign { name, value } => {
                let value = self.emit_expr(value)?;
                let local = self
                    .lookup_local(name)
                    .cloned()
                    .ok_or_else(|| format!("internal error: unknown variable '{name}'"))?;
                self.emit_store(&value, &local.ptr);
                Ok(())
            }
            Stmt::AssignIndex {
                name,
                indices,
                value,
            } => {
                self.emit_index_assign(name, indices, value)?;
                Ok(())
            }
            Stmt::AssignField {
                name,
                fields,
                value,
            } => {
                self.emit_field_assign(name, fields, value)?;
                Ok(())
            }
            Stmt::AssignDeref { target, value } => {
                let place = self.emit_place(target)?;
                let value = self.emit_expr(value)?;
                self.emit_store(&value, &place.ptr);
                Ok(())
            }
            Stmt::Expr(expr) => {
                let _ = self.emit_expr(expr)?;
                Ok(())
            }
            Stmt::Return(Some(expr)) => {
                let value = self.emit_expr(expr)?;
                self.emit_terminator(format!("ret {} {}", llvm_type(&value.ty), value.repr));
                Ok(())
            }
            Stmt::Return(None) => {
                self.emit_terminator("ret void".to_string());
                Ok(())
            }
            Stmt::If {
                condition,
                then_body,
                else_body,
            } => self.emit_if(condition, then_body, else_body.as_deref()),
            Stmt::While { condition, body } => self.emit_while(condition, body),
        }
    }

    fn emit_if(
        &mut self,
        condition: &Expr,
        then_body: &[Stmt],
        else_body: Option<&[Stmt]>,
    ) -> Result<(), String> {
        let condition = self.emit_expr(condition)?;
        let then_label = self.fresh_label("if.then");
        let else_label = else_body.map(|_| self.fresh_label("if.else"));
        let end_label = self.fresh_label("if.end");

        let false_target = else_label.as_deref().unwrap_or(&end_label);
        self.emit_terminator(format!(
            "br i1 {}, label %{}, label %{}",
            condition.repr, then_label, false_target
        ));

        self.start_block(&then_label);
        self.emit_stmts(then_body, true)?;
        let then_terminated = self.terminated;
        if !then_terminated {
            self.emit_terminator(format!("br label %{}", end_label));
        }

        let mut else_terminated = false;
        if let Some(else_body) = else_body {
            let else_label = else_label.expect("else label should exist");
            self.start_block(&else_label);
            self.emit_stmts(else_body, true)?;
            else_terminated = self.terminated;
            if !else_terminated {
                self.emit_terminator(format!("br label %{}", end_label));
            }
        }

        if !then_terminated || else_body.is_none() || !else_terminated {
            self.start_block(&end_label);
        } else {
            self.terminated = true;
        }

        Ok(())
    }

    fn emit_while(&mut self, condition: &Expr, body: &[Stmt]) -> Result<(), String> {
        let cond_label = self.fresh_label("while.cond");
        let body_label = self.fresh_label("while.body");
        let end_label = self.fresh_label("while.end");

        self.emit_terminator(format!("br label %{}", cond_label));

        self.start_block(&cond_label);
        let condition = self.emit_expr(condition)?;
        self.emit_terminator(format!(
            "br i1 {}, label %{}, label %{}",
            condition.repr, body_label, end_label
        ));

        self.start_block(&body_label);
        self.emit_stmts(body, true)?;
        if !self.terminated {
            self.emit_terminator(format!("br label %{}", cond_label));
        }

        self.start_block(&end_label);
        Ok(())
    }

    fn emit_expr(&mut self, expr: &Expr) -> Result<Value, String> {
        match expr {
            Expr::Int(value) => Ok(Value {
                repr: value.to_string(),
                ty: Type::I32,
            }),
            Expr::Bool(value) => Ok(Value {
                repr: if *value { "1".into() } else { "0".into() },
                ty: Type::Bool,
            }),
            Expr::Str(value) => {
                let data = self
                    .string_literals
                    .get(value)
                    .ok_or_else(|| "internal error: unknown string literal".to_string())?;
                Ok(Value {
                    repr: format!(
                        "getelementptr inbounds ([{} x i8], ptr {}, i64 0, i64 0)",
                        data.len, data.global_name
                    ),
                    ty: Type::Str,
                })
            }
            Expr::Var(name) => {
                let local = self
                    .lookup_local(name)
                    .cloned()
                    .ok_or_else(|| format!("internal error: unknown variable '{name}'"))?;
                let temp = self.fresh_temp("load");
                self.emit_assign(
                    &temp,
                    format!("load {}, ptr {}", llvm_type(&local.ty), local.ptr),
                );
                Ok(Value {
                    repr: temp,
                    ty: local.ty,
                })
            }
            Expr::ArrayLiteral(elements) => self.emit_array_literal(elements),
            Expr::StructLiteral { name, fields } => self.emit_struct_literal(name, fields),
            Expr::FieldAccess { base, field } => self.emit_field_access(base, field),
            Expr::Index { base, index } => self.emit_index(base, index),
            Expr::Call { name, args } => self.emit_call(name, args),
            Expr::Binary { op, left, right } => match op {
                BinOp::Or => self.emit_short_circuit_or(left, right),
                BinOp::And => self.emit_short_circuit_and(left, right),
                _ => self.emit_regular_binary(op, left, right),
            },
            Expr::Unary { op, expr } => self.emit_unary(op, expr),
        }
    }

    fn emit_struct_literal(
        &mut self,
        name: &str,
        fields: &[(String, Expr)],
    ) -> Result<Value, String> {
        let layout = self
            .struct_layouts
            .get(name)
            .cloned()
            .ok_or_else(|| format!("internal error: unknown struct '{}'", name))?;
        let struct_ty = Type::Named(name.to_string());
        let mut current = "poison".to_string();

        for (index, (field_name, _)) in layout.fields.iter().enumerate() {
            let field_expr = fields
                .iter()
                .find_map(|(name, expr)| (name == field_name).then_some(expr))
                .ok_or_else(|| {
                    format!(
                        "internal error: missing field '{}' in struct literal '{}'",
                        field_name, name
                    )
                })?;
            let value = self.emit_expr(field_expr)?;
            let next = self.fresh_temp("insert");
            self.emit_assign(
                &next,
                format!(
                    "insertvalue {} {}, {} {}, {}",
                    llvm_type(&struct_ty),
                    current,
                    llvm_type(&value.ty),
                    value.repr,
                    index
                ),
            );
            current = next;
        }

        Ok(Value {
            repr: current,
            ty: struct_ty,
        })
    }

    fn emit_array_literal(&mut self, elements: &[Expr]) -> Result<Value, String> {
        let Some(first) = elements.first() else {
            return Err("internal error: empty array literal".to_string());
        };

        let first_value = self.emit_expr(first)?;
        let array_ty = Type::Array(Box::new(first_value.ty.clone()), elements.len());
        let mut current = "poison".to_string();

        let first_temp = self.fresh_temp("insert");
        self.emit_assign(
            &first_temp,
            format!(
                "insertvalue {} {}, {} {}, 0",
                llvm_type(&array_ty),
                current,
                llvm_type(&first_value.ty),
                first_value.repr
            ),
        );
        current = first_temp;

        for (index, element) in elements.iter().enumerate().skip(1) {
            let value = self.emit_expr(element)?;
            let next = self.fresh_temp("insert");
            self.emit_assign(
                &next,
                format!(
                    "insertvalue {} {}, {} {}, {}",
                    llvm_type(&array_ty),
                    current,
                    llvm_type(&value.ty),
                    value.repr,
                    index
                ),
            );
            current = next;
        }

        Ok(Value {
            repr: current,
            ty: array_ty,
        })
    }

    fn emit_field_access(&mut self, base: &Expr, field: &str) -> Result<Value, String> {
        let base = self.emit_expr(base)?;
        let Type::Named(struct_name) = &base.ty else {
            return Err(format!(
                "internal error: field access '.{}' on non-struct value",
                field
            ));
        };
        let layout = self
            .struct_layouts
            .get(struct_name)
            .ok_or_else(|| format!("internal error: unknown struct '{}'", struct_name))?;
        let (index, field_ty) = layout
            .fields
            .iter()
            .enumerate()
            .find_map(|(index, (name, ty))| (name == field).then_some((index, ty.clone())))
            .ok_or_else(|| {
                format!(
                    "internal error: struct '{}' has no field '{}'",
                    struct_name, field
                )
            })?;

        let temp = self.fresh_temp("field");
        self.emit_assign(
            &temp,
            format!(
                "extractvalue {} {}, {}",
                llvm_type(&base.ty),
                base.repr,
                index
            ),
        );
        Ok(Value {
            repr: temp,
            ty: field_ty,
        })
    }

    fn emit_index(&mut self, base: &Expr, index: &Expr) -> Result<Value, String> {
        let base = self.emit_expr(base)?;
        let index = self.emit_expr(index)?;
        let index_i64 = self.fresh_temp("idx");
        self.emit_assign(&index_i64, format!("sext i32 {} to i64", index.repr));

        match &base.ty {
            Type::Array(element_ty, _) => {
                let spill_ptr = self.create_stack_slot("array.tmp", &base.ty);
                self.emit_store(&base, &spill_ptr);

                let element_ptr = self.fresh_temp("elem.ptr");
                self.emit_assign(
                    &element_ptr,
                    format!(
                        "getelementptr inbounds {}, ptr {}, i64 0, i64 {}",
                        llvm_type(&base.ty),
                        spill_ptr,
                        index_i64
                    ),
                );

                let loaded = self.fresh_temp("elem");
                self.emit_assign(
                    &loaded,
                    format!("load {}, ptr {}", llvm_type(element_ty), element_ptr),
                );

                Ok(Value {
                    repr: loaded,
                    ty: (**element_ty).clone(),
                })
            }
            Type::Slice(element_ty) => {
                let data_ptr = self.fresh_temp("slice.ptr");
                self.emit_assign(
                    &data_ptr,
                    format!("extractvalue {} {}, 0", llvm_type(&base.ty), base.repr),
                );

                let element_ptr = self.fresh_temp("elem.ptr");
                self.emit_assign(
                    &element_ptr,
                    format!(
                        "getelementptr inbounds {}, ptr {}, i64 {}",
                        llvm_type(element_ty),
                        data_ptr,
                        index_i64
                    ),
                );

                let loaded = self.fresh_temp("elem");
                self.emit_assign(
                    &loaded,
                    format!("load {}, ptr {}", llvm_type(element_ty), element_ptr),
                );

                Ok(Value {
                    repr: loaded,
                    ty: (**element_ty).clone(),
                })
            }
            Type::Ptr(element_ty) => {
                let element_ptr = self.fresh_temp("elem.ptr");
                self.emit_assign(
                    &element_ptr,
                    format!(
                        "getelementptr inbounds {}, ptr {}, i64 {}",
                        llvm_type(element_ty),
                        base.repr,
                        index_i64
                    ),
                );

                let loaded = self.fresh_temp("elem");
                self.emit_assign(
                    &loaded,
                    format!("load {}, ptr {}", llvm_type(element_ty), element_ptr),
                );

                Ok(Value {
                    repr: loaded,
                    ty: (**element_ty).clone(),
                })
            }
            _ => Err(
                "internal error: indexing non-array, non-slice, or non-pointer value".to_string(),
            ),
        }
    }

    fn emit_call(&mut self, name: &str, args: &[Expr]) -> Result<Value, String> {
        if name == "len" {
            return self.emit_len_call(args);
        }
        if name == "slice" {
            return self.emit_slice_call(args);
        }

        let sig = self
            .function_sigs
            .get(name)
            .cloned()
            .ok_or_else(|| format!("internal error: unknown function '{name}'"))?;

        let mut rendered_args = Vec::new();
        for (arg, ty) in args.iter().zip(sig.params.iter()) {
            let value = self.emit_expr(arg)?;
            rendered_args.push(format!("{} {}", llvm_type(ty), value.repr));
        }

        let callee = llvm_function_name(name);
        let rendered_args = rendered_args.join(", ");

        if sig.ret_type == Type::Void {
            self.emit_line(format!("call void {}({})", callee, rendered_args));
            Ok(Value {
                repr: String::new(),
                ty: Type::Void,
            })
        } else {
            let temp = self.fresh_temp("call");
            self.emit_assign(
                &temp,
                format!(
                    "call {} {}({})",
                    llvm_type(&sig.ret_type),
                    callee,
                    rendered_args
                ),
            );
            Ok(Value {
                repr: temp,
                ty: sig.ret_type,
            })
        }
    }

    fn emit_len_call(&mut self, args: &[Expr]) -> Result<Value, String> {
        if args.len() != 1 {
            return Err(format!(
                "internal error: len() expects 1 arg, got {}",
                args.len()
            ));
        }

        let value = self.emit_expr(&args[0])?;
        match value.ty {
            Type::Array(_, len) => Ok(Value {
                repr: len.to_string(),
                ty: Type::I32,
            }),
            Type::Slice(_) => {
                let len = self.fresh_temp("slice.len");
                self.emit_assign(
                    &len,
                    format!("extractvalue {} {}, 1", llvm_type(&value.ty), value.repr),
                );
                Ok(Value {
                    repr: len,
                    ty: Type::I32,
                })
            }
            _ => Err("internal error: len() requires an array or slice value".to_string()),
        }
    }

    fn emit_slice_call(&mut self, args: &[Expr]) -> Result<Value, String> {
        if args.len() != 1 {
            return Err(format!(
                "internal error: slice() expects 1 arg, got {}",
                args.len()
            ));
        }

        let arg = &args[0];
        let value = self.emit_expr(arg)?;
        let value_ty = value.ty.clone();
        match value_ty {
            Type::Slice(_) => Ok(value),
            Type::Array(element_ty, len) => {
                self.emit_array_value_as_slice(arg, &element_ty, len, value)
            }
            _ => Err("internal error: slice() requires an array or slice value".to_string()),
        }
    }

    fn emit_array_value_as_slice(
        &mut self,
        source_expr: &Expr,
        element_ty: &Type,
        len: usize,
        value: Value,
    ) -> Result<Value, String> {
        let array_ty = value.ty.clone();
        let array_ptr = if let Expr::Var(name) = source_expr {
            if let Some(local) = self.lookup_local(name) {
                if matches!(local.ty, Type::Array(_, _)) {
                    local.ptr.clone()
                } else {
                    let spill_ptr = self.create_stack_slot("slice.tmp", &array_ty);
                    self.emit_store(&value, &spill_ptr);
                    spill_ptr
                }
            } else {
                let spill_ptr = self.create_stack_slot("slice.tmp", &array_ty);
                self.emit_store(&value, &spill_ptr);
                spill_ptr
            }
        } else {
            let spill_ptr = self.create_stack_slot("slice.tmp", &array_ty);
            self.emit_store(&value, &spill_ptr);
            spill_ptr
        };

        let data_ptr = self.fresh_temp("slice.data");
        self.emit_assign(
            &data_ptr,
            format!(
                "getelementptr inbounds {}, ptr {}, i64 0, i64 0",
                llvm_type(&array_ty),
                array_ptr
            ),
        );

        let slice_ty = Type::Slice(Box::new(element_ty.clone()));
        let with_ptr = self.fresh_temp("slice");
        self.emit_assign(
            &with_ptr,
            format!(
                "insertvalue {} poison, ptr {}, 0",
                llvm_type(&slice_ty),
                data_ptr
            ),
        );

        let with_len = self.fresh_temp("slice");
        self.emit_assign(
            &with_len,
            format!(
                "insertvalue {} {}, i32 {}, 1",
                llvm_type(&slice_ty),
                with_ptr,
                len
            ),
        );

        Ok(Value {
            repr: with_len,
            ty: slice_ty,
        })
    }

    fn emit_short_circuit_or(&mut self, left: &Expr, right: &Expr) -> Result<Value, String> {
        let left = self.emit_expr(left)?;
        let left_block = self.current_block.clone();
        let rhs_label = self.fresh_label("or.rhs");
        let end_label = self.fresh_label("or.end");

        self.emit_terminator(format!(
            "br i1 {}, label %{}, label %{}",
            left.repr, end_label, rhs_label
        ));

        self.start_block(&rhs_label);
        let right = self.emit_expr(right)?;
        let rhs_block = self.current_block.clone();
        self.emit_terminator(format!("br label %{}", end_label));

        self.start_block(&end_label);
        let result = self.fresh_temp("or");
        self.emit_assign(
            &result,
            format!(
                "phi i1 [ 1, %{} ], [ {}, %{} ]",
                left_block, right.repr, rhs_block
            ),
        );

        Ok(Value {
            repr: result,
            ty: Type::Bool,
        })
    }

    fn emit_short_circuit_and(&mut self, left: &Expr, right: &Expr) -> Result<Value, String> {
        let left = self.emit_expr(left)?;
        let left_block = self.current_block.clone();
        let rhs_label = self.fresh_label("and.rhs");
        let end_label = self.fresh_label("and.end");

        self.emit_terminator(format!(
            "br i1 {}, label %{}, label %{}",
            left.repr, rhs_label, end_label
        ));

        self.start_block(&rhs_label);
        let right = self.emit_expr(right)?;
        let rhs_block = self.current_block.clone();
        self.emit_terminator(format!("br label %{}", end_label));

        self.start_block(&end_label);
        let result = self.fresh_temp("and");
        self.emit_assign(
            &result,
            format!(
                "phi i1 [ 0, %{} ], [ {}, %{} ]",
                left_block, right.repr, rhs_block
            ),
        );

        Ok(Value {
            repr: result,
            ty: Type::Bool,
        })
    }

    fn emit_regular_binary(
        &mut self,
        op: &BinOp,
        left: &Expr,
        right: &Expr,
    ) -> Result<Value, String> {
        let left = self.emit_expr(left)?;
        let right = self.emit_expr(right)?;
        let temp = self.fresh_temp("bin");

        let instruction = match op {
            BinOp::Add => format!("add i32 {}, {}", left.repr, right.repr),
            BinOp::Sub => format!("sub i32 {}, {}", left.repr, right.repr),
            BinOp::Mul => format!("mul i32 {}, {}", left.repr, right.repr),
            BinOp::Div => format!("sdiv i32 {}, {}", left.repr, right.repr),
            BinOp::Eq => format!(
                "icmp eq {} {}, {}",
                llvm_type(&left.ty),
                left.repr,
                right.repr
            ),
            BinOp::Ne => format!(
                "icmp ne {} {}, {}",
                llvm_type(&left.ty),
                left.repr,
                right.repr
            ),
            BinOp::Lt => format!("icmp slt i32 {}, {}", left.repr, right.repr),
            BinOp::Le => format!("icmp sle i32 {}, {}", left.repr, right.repr),
            BinOp::Gt => format!("icmp sgt i32 {}, {}", left.repr, right.repr),
            BinOp::Ge => format!("icmp sge i32 {}, {}", left.repr, right.repr),
            BinOp::Or | BinOp::And => unreachable!("handled separately"),
        };

        self.emit_assign(&temp, instruction);

        let ty = match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => Type::I32,
            BinOp::Eq
            | BinOp::Ne
            | BinOp::Lt
            | BinOp::Le
            | BinOp::Gt
            | BinOp::Ge
            | BinOp::Or
            | BinOp::And => Type::Bool,
        };

        Ok(Value { repr: temp, ty })
    }

    fn emit_unary(&mut self, op: &UnaryOp, expr: &Expr) -> Result<Value, String> {
        match op {
            UnaryOp::Neg => {
                let expr = self.emit_expr(expr)?;
                let temp = self.fresh_temp("unary");
                self.emit_assign(&temp, format!("sub i32 0, {}", expr.repr));
                Ok(Value {
                    repr: temp,
                    ty: Type::I32,
                })
            }
            UnaryOp::Not => {
                let expr = self.emit_expr(expr)?;
                let temp = self.fresh_temp("unary");
                self.emit_assign(&temp, format!("xor i1 {}, 1", expr.repr));
                Ok(Value {
                    repr: temp,
                    ty: Type::Bool,
                })
            }
            UnaryOp::AddrOf => {
                let place = self.emit_place(expr)?;
                Ok(Value {
                    repr: place.ptr,
                    ty: Type::Ptr(Box::new(place.ty)),
                })
            }
            UnaryOp::Deref => {
                let expr = self.emit_expr(expr)?;
                let Type::Ptr(inner_ty) = &expr.ty else {
                    return Err("internal error: unary '*' on non-pointer value".to_string());
                };

                let temp = self.fresh_temp("deref");
                self.emit_assign(
                    &temp,
                    format!("load {}, ptr {}", llvm_type(inner_ty), expr.repr),
                );
                Ok(Value {
                    repr: temp,
                    ty: (**inner_ty).clone(),
                })
            }
        }
    }

    fn emit_store(&mut self, value: &Value, ptr: &str) {
        self.emit_line(format!(
            "store {} {}, ptr {}",
            llvm_type(&value.ty),
            value.repr,
            ptr
        ));
    }

    fn emit_index_assign(
        &mut self,
        name: &str,
        indices: &[Expr],
        value: &Expr,
    ) -> Result<(), String> {
        let target = self.build_index_target(name, indices);
        let place = self.emit_place(&target)?;
        let value = self.emit_expr(value)?;
        self.emit_store(&value, &place.ptr);
        Ok(())
    }

    fn emit_field_assign(
        &mut self,
        name: &str,
        fields: &[String],
        value: &Expr,
    ) -> Result<(), String> {
        let target = self.build_field_target(name, fields);
        let place = self.emit_place(&target)?;
        let value = self.emit_expr(value)?;
        self.emit_store(&value, &place.ptr);
        Ok(())
    }

    fn emit_default_return(&mut self) {
        match &self.function.ret_type {
            Type::I32 => self.emit_terminator("ret i32 0".to_string()),
            Type::Bool => self.emit_terminator("ret i1 0".to_string()),
            Type::Str => self.emit_terminator("ret ptr null".to_string()),
            Type::Ptr(_) => self.emit_terminator("ret ptr null".to_string()),
            Type::Void => self.emit_terminator("ret void".to_string()),
            Type::Named(_) | Type::Array(_, _) | Type::Slice(_) => self.emit_terminator(format!(
                "ret {} zeroinitializer",
                llvm_type(&self.function.ret_type)
            )),
        }
    }

    fn emit_place(&mut self, expr: &Expr) -> Result<Place, String> {
        match expr {
            Expr::Var(name) => {
                let local = self
                    .lookup_local(name)
                    .cloned()
                    .ok_or_else(|| format!("internal error: unknown variable '{name}'"))?;
                Ok(Place {
                    ptr: local.ptr,
                    ty: local.ty,
                })
            }
            Expr::FieldAccess { base, field } => {
                let base = self.emit_place(base)?;
                let Type::Named(struct_name) = &base.ty else {
                    return Err(format!(
                        "internal error: field access '.{}' on non-struct place",
                        field
                    ));
                };

                let layout = self
                    .struct_layouts
                    .get(struct_name)
                    .ok_or_else(|| format!("internal error: unknown struct '{}'", struct_name))?;
                let (index, field_ty) = layout
                    .fields
                    .iter()
                    .enumerate()
                    .find_map(|(index, (name, ty))| (name == field).then_some((index, ty.clone())))
                    .ok_or_else(|| {
                        format!(
                            "internal error: struct '{}' has no field '{}'",
                            struct_name, field
                        )
                    })?;

                let field_ptr = self.fresh_temp("field.ptr");
                self.emit_assign(
                    &field_ptr,
                    format!(
                        "getelementptr inbounds {}, ptr {}, i32 0, i32 {}",
                        llvm_type(&base.ty),
                        base.ptr,
                        index
                    ),
                );

                Ok(Place {
                    ptr: field_ptr,
                    ty: field_ty,
                })
            }
            Expr::Index { base, index } => {
                let index = self.emit_expr(index)?;
                let index_i64 = self.fresh_temp("idx");
                self.emit_assign(&index_i64, format!("sext i32 {} to i64", index.repr));

                let base_value = self.emit_expr(base)?;
                match &base_value.ty {
                    Type::Array(element_ty, _) => {
                        let array_ptr = match self.emit_place(base) {
                            Ok(place) => place.ptr,
                            Err(_) => {
                                let spill_ptr =
                                    self.create_stack_slot("array.place.tmp", &base_value.ty);
                                self.emit_store(&base_value, &spill_ptr);
                                spill_ptr
                            }
                        };

                        let element_ptr = self.fresh_temp("elem.ptr");
                        self.emit_assign(
                            &element_ptr,
                            format!(
                                "getelementptr inbounds {}, ptr {}, i64 0, i64 {}",
                                llvm_type(&base_value.ty),
                                array_ptr,
                                index_i64
                            ),
                        );

                        Ok(Place {
                            ptr: element_ptr,
                            ty: (**element_ty).clone(),
                        })
                    }
                    Type::Slice(element_ty) => {
                        let data_ptr = self.fresh_temp("slice.ptr");
                        self.emit_assign(
                            &data_ptr,
                            format!(
                                "extractvalue {} {}, 0",
                                llvm_type(&base_value.ty),
                                base_value.repr
                            ),
                        );

                        let element_ptr = self.fresh_temp("elem.ptr");
                        self.emit_assign(
                            &element_ptr,
                            format!(
                                "getelementptr inbounds {}, ptr {}, i64 {}",
                                llvm_type(element_ty),
                                data_ptr,
                                index_i64
                            ),
                        );

                        Ok(Place {
                            ptr: element_ptr,
                            ty: (**element_ty).clone(),
                        })
                    }
                    Type::Ptr(element_ty) => {
                        let element_ptr = self.fresh_temp("elem.ptr");
                        self.emit_assign(
                            &element_ptr,
                            format!(
                                "getelementptr inbounds {}, ptr {}, i64 {}",
                                llvm_type(element_ty),
                                base_value.repr,
                                index_i64
                            ),
                        );

                        Ok(Place {
                            ptr: element_ptr,
                            ty: (**element_ty).clone(),
                        })
                    }
                    _ => Err(
                        "internal error: indexing requires an array, slice, or pointer value"
                            .to_string(),
                    ),
                }
            }
            Expr::Unary {
                op: UnaryOp::Deref,
                expr,
            } => {
                let value = self.emit_expr(expr)?;
                let Type::Ptr(inner_ty) = &value.ty else {
                    return Err("internal error: unary '*' on non-pointer value".to_string());
                };

                Ok(Place {
                    ptr: value.repr,
                    ty: (**inner_ty).clone(),
                })
            }
            _ => Err("internal error: expression is not addressable".to_string()),
        }
    }

    fn build_index_target(&self, name: &str, indices: &[Expr]) -> Expr {
        let mut expr = Expr::Var(name.to_string());
        for index in indices {
            expr = Expr::Index {
                base: Box::new(expr),
                index: Box::new(index.clone()),
            };
        }
        expr
    }

    fn build_field_target(&self, name: &str, fields: &[String]) -> Expr {
        let mut expr = Expr::Var(name.to_string());
        for field in fields {
            expr = Expr::FieldAccess {
                base: Box::new(expr),
                field: field.clone(),
            };
        }
        expr
    }

    fn create_stack_slot(&mut self, name: &str, ty: &Type) -> String {
        let slot = format!("%{}.addr.{}", name, self.slot_counter);
        self.slot_counter += 1;
        self.entry_allocas
            .push(format!("{} = alloca {}", slot, llvm_type(ty)));
        slot
    }

    fn declare_local(&mut self, name: &str, ty: Type, ptr: String) -> Result<(), String> {
        let scope = self
            .scopes
            .last_mut()
            .ok_or_else(|| "internal error: missing scope".to_string())?;
        scope.insert(name.to_string(), LocalVar { ptr, ty });
        Ok(())
    }

    fn lookup_local(&self, name: &str) -> Option<&LocalVar> {
        self.scopes.iter().rev().find_map(|scope| scope.get(name))
    }

    fn emit_line(&mut self, line: String) {
        self.body_lines.push(format!("  {}", line));
    }

    fn emit_assign(&mut self, name: &str, instruction: String) {
        self.emit_line(format!("{name} = {instruction}"));
    }

    fn emit_terminator(&mut self, line: String) {
        self.emit_line(line);
        self.terminated = true;
    }

    fn start_block(&mut self, label: &str) {
        self.body_lines.push(format!("{label}:"));
        self.current_block = label.to_string();
        self.terminated = false;
    }

    fn enter_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn exit_scope(&mut self) {
        self.scopes.pop();
    }

    fn fresh_temp(&mut self, prefix: &str) -> String {
        let name = format!("%{}.{}", prefix, self.temp_counter);
        self.temp_counter += 1;
        name
    }

    fn fresh_label(&mut self, prefix: &str) -> String {
        let name = format!("{}.{}", prefix, self.label_counter);
        self.label_counter += 1;
        name
    }
}
