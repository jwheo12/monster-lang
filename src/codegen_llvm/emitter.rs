use std::collections::HashMap;

use crate::ast::{BinOp, Expr, Function, MatchArm, Stmt, Type, UnaryOp};

use super::{
    ConstInfo, EnumLayout, EnumVariantInfo, FunctionSig, StringLiteralData, StructLayout,
    runtime::llvm_function_name,
    util::{integer_bit_width, is_signed_integer_type, llvm_type},
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

#[derive(Clone)]
struct LoopLabels {
    continue_label: String,
    break_label: String,
}

pub(super) struct FunctionEmitter<'a> {
    function: &'a Function,
    function_sigs: &'a HashMap<String, FunctionSig>,
    struct_layouts: &'a HashMap<String, StructLayout>,
    enum_layouts: &'a HashMap<String, EnumLayout>,
    enum_variants: &'a HashMap<String, EnumVariantInfo>,
    consts: &'a HashMap<String, ConstInfo>,
    string_literals: &'a HashMap<String, StringLiteralData>,
    entry_allocas: Vec<String>,
    body_lines: Vec<String>,
    scopes: Vec<HashMap<String, LocalVar>>,
    loop_stack: Vec<LoopLabels>,
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
        enum_layouts: &'a HashMap<String, EnumLayout>,
        enum_variants: &'a HashMap<String, EnumVariantInfo>,
        consts: &'a HashMap<String, ConstInfo>,
        string_literals: &'a HashMap<String, StringLiteralData>,
    ) -> Self {
        Self {
            function,
            function_sigs,
            struct_layouts,
            enum_layouts,
            enum_variants,
            consts,
            string_literals,
            entry_allocas: Vec::new(),
            body_lines: Vec::new(),
            scopes: Vec::new(),
            loop_stack: Vec::new(),
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
            self.emit_line(format!(
                "store {} %{}, ptr {}",
                self.llvm_type(ty),
                name,
                ptr
            ));
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
            .map(|(name, ty)| format!("{} %{}", self.llvm_type(ty), name))
            .collect::<Vec<_>>()
            .join(", ");

        let mut out = String::new();
        out.push_str(&format!(
            "define {} @{}({}) {{\n",
            self.llvm_type(&self.function.ret_type),
            llvm_function_name(&self.function.name).trim_start_matches('@'),
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

    fn llvm_type(&self, ty: &Type) -> String {
        llvm_type(ty, self.enum_layouts)
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
            Stmt::Break => {
                let labels = self
                    .loop_stack
                    .last()
                    .cloned()
                    .ok_or_else(|| "internal error: break used outside of loop".to_string())?;
                self.emit_terminator(format!("br label %{}", labels.break_label));
                Ok(())
            }
            Stmt::Continue => {
                let labels =
                    self.loop_stack.last().cloned().ok_or_else(|| {
                        "internal error: continue used outside of loop".to_string()
                    })?;
                self.emit_terminator(format!("br label %{}", labels.continue_label));
                Ok(())
            }
            Stmt::Return(Some(expr)) => {
                let value = self.emit_expr(expr)?;
                self.emit_terminator(format!("ret {} {}", self.llvm_type(&value.ty), value.repr));
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
        self.loop_stack.push(LoopLabels {
            continue_label: cond_label.clone(),
            break_label: end_label.clone(),
        });
        self.emit_stmts(body, true)?;
        self.loop_stack.pop();
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
            Expr::SizeOf(ty) => self.emit_sizeof(ty),
            Expr::Cast { expr, ty } => self.emit_cast(expr, ty),
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
                if let Some(local) = self.lookup_local(name).cloned() {
                    let temp = self.fresh_temp("load");
                    self.emit_assign(
                        &temp,
                        format!("load {}, ptr {}", self.llvm_type(&local.ty), local.ptr),
                    );
                    Ok(Value {
                        repr: temp,
                        ty: local.ty,
                    })
                } else if let Some(const_info) = self.consts.get(name) {
                    let value = self.emit_expr(&const_info.value)?;
                    if value.ty == const_info.ty {
                        Ok(value)
                    } else {
                        Err(format!(
                            "internal error: constant '{}' emitted {} but was declared {}",
                            name,
                            self.llvm_type(&value.ty),
                            self.llvm_type(&const_info.ty)
                        ))
                    }
                } else if let Some(variant) = self.enum_variants.get(name) {
                    if variant.payload_ty.is_some() {
                        Err(format!(
                            "internal error: enum variant '{}' requires payload construction",
                            name
                        ))
                    } else {
                        self.emit_enum_variant_value(variant, None)
                    }
                } else {
                    Err(format!("internal error: unknown variable '{name}'"))
                }
            }
            Expr::Match { value, arms } => self.emit_match_expr(value, arms),
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

    fn emit_sizeof(&mut self, ty: &Type) -> Result<Value, String> {
        if *ty == Type::Void {
            return Err("internal error: sizeof(void) is not supported".to_string());
        }

        let sizeof_ptr = self.fresh_temp("sizeof.ptr");
        self.emit_assign(
            &sizeof_ptr,
            format!("getelementptr {}, ptr null, i64 1", self.llvm_type(ty)),
        );

        let sizeof = self.fresh_temp("sizeof");
        self.emit_assign(&sizeof, format!("ptrtoint ptr {} to i64", sizeof_ptr));

        Ok(Value {
            repr: sizeof,
            ty: Type::USize,
        })
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
                    self.llvm_type(&struct_ty),
                    current,
                    self.llvm_type(&value.ty),
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
                self.llvm_type(&array_ty),
                current,
                self.llvm_type(&first_value.ty),
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
                    self.llvm_type(&array_ty),
                    current,
                    self.llvm_type(&value.ty),
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
                self.llvm_type(&base.ty),
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
        let index_i64 = self.emit_index_value_as_i64(&index)?;

        match &base.ty {
            Type::Array(element_ty, _) => {
                let spill_ptr = self.create_stack_slot("array.tmp", &base.ty);
                self.emit_store(&base, &spill_ptr);

                let element_ptr = self.fresh_temp("elem.ptr");
                self.emit_assign(
                    &element_ptr,
                    format!(
                        "getelementptr inbounds {}, ptr {}, i64 0, i64 {}",
                        self.llvm_type(&base.ty),
                        spill_ptr,
                        index_i64
                    ),
                );

                let loaded = self.fresh_temp("elem");
                self.emit_assign(
                    &loaded,
                    format!("load {}, ptr {}", self.llvm_type(element_ty), element_ptr),
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
                    format!("extractvalue {} {}, 0", self.llvm_type(&base.ty), base.repr),
                );

                let element_ptr = self.fresh_temp("elem.ptr");
                self.emit_assign(
                    &element_ptr,
                    format!(
                        "getelementptr inbounds {}, ptr {}, i64 {}",
                        self.llvm_type(element_ty),
                        data_ptr,
                        index_i64
                    ),
                );

                let loaded = self.fresh_temp("elem");
                self.emit_assign(
                    &loaded,
                    format!("load {}, ptr {}", self.llvm_type(element_ty), element_ptr),
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
                        self.llvm_type(element_ty),
                        base.repr,
                        index_i64
                    ),
                );

                let loaded = self.fresh_temp("elem");
                self.emit_assign(
                    &loaded,
                    format!("load {}, ptr {}", self.llvm_type(element_ty), element_ptr),
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
        if name == "is" {
            return self.emit_is_call(args);
        }
        if name == "payload" {
            return self.emit_payload_call(args);
        }
        if let Some(variant) = self.enum_variants.get(name).cloned() {
            let payload =
                if variant.payload_ty.is_some() {
                    Some(self.emit_expr(args.first().ok_or_else(|| {
                        format!("internal error: missing payload for '{name}'")
                    })?)?)
                } else {
                    None
                };
            return self.emit_enum_variant_value(&variant, payload.as_ref());
        }

        let sig = self
            .function_sigs
            .get(name)
            .cloned()
            .ok_or_else(|| format!("internal error: unknown function '{name}'"))?;

        let mut rendered_args = Vec::new();
        for (arg, ty) in args.iter().zip(sig.params.iter()) {
            let value = self.emit_expr(arg)?;
            rendered_args.push(format!("{} {}", self.llvm_type(ty), value.repr));
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
                    self.llvm_type(&sig.ret_type),
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
                ty: Type::USize,
            }),
            Type::Slice(_) => {
                let len = self.fresh_temp("slice.len");
                self.emit_assign(
                    &len,
                    format!(
                        "extractvalue {} {}, 1",
                        self.llvm_type(&value.ty),
                        value.repr
                    ),
                );
                Ok(Value {
                    repr: len,
                    ty: Type::USize,
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

    fn emit_is_call(&mut self, args: &[Expr]) -> Result<Value, String> {
        if args.len() != 2 {
            return Err(format!(
                "internal error: is() expects 2 args, got {}",
                args.len()
            ));
        }

        let value = self.emit_expr(&args[0])?;
        let variant = self.extract_variant_designator(&args[1], "is")?;
        let tag = self.emit_enum_tag(&value)?;
        let result = self.fresh_temp("is");
        self.emit_assign(
            &result,
            format!("icmp eq i32 {}, {}", tag, variant.discriminant),
        );
        Ok(Value {
            repr: result,
            ty: Type::Bool,
        })
    }

    fn emit_payload_call(&mut self, args: &[Expr]) -> Result<Value, String> {
        if args.len() != 2 {
            return Err(format!(
                "internal error: payload() expects 2 args, got {}",
                args.len()
            ));
        }

        let value = self.emit_expr(&args[0])?;
        let variant = self.extract_variant_designator(&args[1], "payload")?;
        let variant_name = match &args[1] {
            Expr::Var(name) => name.clone(),
            _ => unreachable!("extract_variant_designator validated shape"),
        };
        let payload_ty = variant
            .payload_ty
            .clone()
            .ok_or_else(|| format!("internal error: variant '{}' has no payload", variant_name))?;
        let enum_ty = value.ty.clone();

        let enum_ptr = self.create_stack_slot("enum.payload.tmp", &enum_ty);
        self.emit_store(&value, &enum_ptr);

        let tag_ptr = self.fresh_temp("enum.tag.ptr");
        self.emit_assign(
            &tag_ptr,
            format!(
                "getelementptr inbounds {}, ptr {}, i32 0, i32 0",
                self.llvm_type(&enum_ty),
                enum_ptr
            ),
        );
        let tag = self.fresh_temp("enum.tag");
        self.emit_assign(&tag, format!("load i32, ptr {}", tag_ptr));

        let ok_label = self.fresh_label("payload.ok");
        let fail_label = self.fresh_label("payload.fail");
        let end_label = self.fresh_label("payload.end");

        let is_expected = self.fresh_temp("payload.is_expected");
        self.emit_assign(
            &is_expected,
            format!("icmp eq i32 {}, {}", tag, variant.discriminant),
        );
        self.emit_terminator(format!(
            "br i1 {}, label %{}, label %{}",
            is_expected, ok_label, fail_label
        ));

        self.start_block(&fail_label);
        self.emit_line("call i32 @puts(ptr getelementptr inbounds ([51 x i8], ptr @.str.enum_payload_error, i64 0, i64 0))".to_string());
        self.emit_line("call void @exit(i32 1)".to_string());
        self.emit_terminator("unreachable".to_string());

        self.start_block(&ok_label);
        let payload = self.emit_payload_from_ptr(&enum_ptr, &enum_ty, &payload_ty)?;
        self.emit_terminator(format!("br label %{}", end_label));

        self.start_block(&end_label);
        Ok(Value {
            repr: payload.repr,
            ty: payload.ty,
        })
    }

    fn emit_match_expr(&mut self, value_expr: &Expr, arms: &[MatchArm]) -> Result<Value, String> {
        if arms.is_empty() {
            return Err("internal error: match expression has no arms".to_string());
        }

        let value = self.emit_expr(value_expr)?;
        let tag = self.emit_enum_tag(&value)?;
        let value_ptr = self.create_stack_slot("match.tmp", &value.ty);
        self.emit_store(&value, &value_ptr);

        let end_label = self.fresh_label("match.end");
        let unreachable_label = self.fresh_label("match.unreachable");
        let mut incoming_values = Vec::new();
        let mut result_ty = None;
        let mut next_label: Option<String> = None;

        for (index, arm) in arms.iter().enumerate() {
            if let Some(label) = next_label.take() {
                self.start_block(&label);
            }

            let variant = self
                .enum_variants
                .get(&arm.pattern.variant)
                .cloned()
                .ok_or_else(|| {
                    format!(
                        "internal error: unknown enum variant '{}' in match arm",
                        arm.pattern.variant
                    )
                })?;

            let arm_label = self.fresh_label("match.arm");
            let miss_label = if index + 1 == arms.len() {
                unreachable_label.clone()
            } else {
                self.fresh_label("match.next")
            };
            let is_expected = self.fresh_temp("match.is");
            self.emit_assign(
                &is_expected,
                format!("icmp eq i32 {}, {}", tag, variant.discriminant),
            );
            self.emit_terminator(format!(
                "br i1 {}, label %{}, label %{}",
                is_expected, arm_label, miss_label
            ));

            self.start_block(&arm_label);
            self.enter_scope();

            if let (Some(payload_ty), Some(binding)) =
                (variant.payload_ty.clone(), arm.pattern.binding.as_ref())
            {
                if binding != "_" {
                    let payload = self.emit_payload_from_ptr(&value_ptr, &value.ty, &payload_ty)?;
                    let ptr = self.create_stack_slot(binding, &payload_ty);
                    self.declare_local(binding, payload_ty.clone(), ptr.clone())?;
                    self.emit_store(&payload, &ptr);
                }
            }

            let arm_value = self.emit_expr(&arm.expr)?;
            self.exit_scope();

            if let Some(expected_ty) = &result_ty {
                if arm_value.ty != *expected_ty {
                    return Err("internal error: match arm type mismatch".to_string());
                }
            } else {
                result_ty = Some(arm_value.ty.clone());
            }

            let incoming_block = self.current_block.clone();
            if arm_value.ty != Type::Void {
                incoming_values.push((arm_value.repr.clone(), incoming_block));
            }
            self.emit_terminator(format!("br label %{}", end_label));
            next_label = Some(miss_label);
        }

        if let Some(label) = next_label {
            self.start_block(&label);
            self.emit_terminator("unreachable".to_string());
        }

        self.start_block(&end_label);
        let result_ty = result_ty.expect("match expression has at least one arm");
        if result_ty == Type::Void {
            Ok(Value {
                repr: String::new(),
                ty: Type::Void,
            })
        } else {
            let result = self.fresh_temp("match");
            let incoming = incoming_values
                .into_iter()
                .map(|(value, label)| format!("[ {}, %{} ]", value, label))
                .collect::<Vec<_>>()
                .join(", ");
            self.emit_assign(
                &result,
                format!("phi {} {}", self.llvm_type(&result_ty), incoming),
            );
            Ok(Value {
                repr: result,
                ty: result_ty,
            })
        }
    }

    fn emit_payload_from_ptr(
        &mut self,
        enum_ptr: &str,
        enum_ty: &Type,
        payload_ty: &Type,
    ) -> Result<Value, String> {
        let payload_field_ptr = self.fresh_temp("enum.payload.ptr");
        self.emit_assign(
            &payload_field_ptr,
            format!(
                "getelementptr inbounds {}, ptr {}, i32 0, i32 2",
                self.llvm_type(enum_ty),
                enum_ptr
            ),
        );
        let payload = self.fresh_temp("enum.payload");
        self.emit_assign(
            &payload,
            format!(
                "load {}, ptr {}",
                self.llvm_type(payload_ty),
                payload_field_ptr
            ),
        );
        Ok(Value {
            repr: payload,
            ty: payload_ty.clone(),
        })
    }

    fn emit_enum_variant_value(
        &mut self,
        variant: &EnumVariantInfo,
        payload: Option<&Value>,
    ) -> Result<Value, String> {
        let enum_ty = Type::Named(variant.enum_name.clone());
        let layout = self
            .enum_layouts
            .get(&variant.enum_name)
            .ok_or_else(|| format!("internal error: unknown enum '{}'", variant.enum_name))?;

        if !layout.has_payload {
            return Ok(Value {
                repr: variant.discriminant.to_string(),
                ty: enum_ty,
            });
        }

        let enum_ptr = self.create_stack_slot("enum.variant.tmp", &enum_ty);
        self.emit_line(format!(
            "store {} zeroinitializer, ptr {}",
            self.llvm_type(&enum_ty),
            enum_ptr
        ));

        let tag_ptr = self.fresh_temp("enum.tag.ptr");
        self.emit_assign(
            &tag_ptr,
            format!(
                "getelementptr inbounds {}, ptr {}, i32 0, i32 0",
                self.llvm_type(&enum_ty),
                enum_ptr
            ),
        );
        self.emit_line(format!(
            "store i32 {}, ptr {}",
            variant.discriminant, tag_ptr
        ));

        if let Some(payload) = payload {
            let payload_field_ptr = self.fresh_temp("enum.payload.ptr");
            self.emit_assign(
                &payload_field_ptr,
                format!(
                    "getelementptr inbounds {}, ptr {}, i32 0, i32 2",
                    self.llvm_type(&enum_ty),
                    enum_ptr
                ),
            );
            self.emit_line(format!(
                "store {} {}, ptr {}",
                self.llvm_type(&payload.ty),
                payload.repr,
                payload_field_ptr
            ));
        }

        let loaded = self.fresh_temp("enum");
        self.emit_assign(
            &loaded,
            format!("load {}, ptr {}", self.llvm_type(&enum_ty), enum_ptr),
        );
        Ok(Value {
            repr: loaded,
            ty: enum_ty,
        })
    }

    fn emit_enum_tag(&mut self, value: &Value) -> Result<String, String> {
        let Type::Named(enum_name) = &value.ty else {
            return Err("internal error: enum tag requested for non-enum value".to_string());
        };

        let layout = self
            .enum_layouts
            .get(enum_name)
            .ok_or_else(|| format!("internal error: unknown enum '{}'", enum_name))?;

        if !layout.has_payload {
            return Ok(value.repr.clone());
        }

        let enum_ptr = self.create_stack_slot("enum.tag.tmp", &value.ty);
        self.emit_store(value, &enum_ptr);
        let tag_ptr = self.fresh_temp("enum.tag.ptr");
        self.emit_assign(
            &tag_ptr,
            format!(
                "getelementptr inbounds {}, ptr {}, i32 0, i32 0",
                self.llvm_type(&value.ty),
                enum_ptr
            ),
        );
        let tag = self.fresh_temp("enum.tag");
        self.emit_assign(&tag, format!("load i32, ptr {}", tag_ptr));
        Ok(tag)
    }

    fn extract_variant_designator(
        &self,
        expr: &Expr,
        context: &str,
    ) -> Result<EnumVariantInfo, String> {
        let Expr::Var(name) = expr else {
            return Err(format!(
                "internal error: {} expects enum variant designator",
                context
            ));
        };

        self.enum_variants.get(name).cloned().ok_or_else(|| {
            format!(
                "internal error: {} expects enum variant designator",
                context
            )
        })
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
                self.llvm_type(&array_ty),
                array_ptr
            ),
        );

        let slice_ty = Type::Slice(Box::new(element_ty.clone()));
        let with_ptr = self.fresh_temp("slice");
        self.emit_assign(
            &with_ptr,
            format!(
                "insertvalue {} poison, ptr {}, 0",
                self.llvm_type(&slice_ty),
                data_ptr
            ),
        );

        let with_len = self.fresh_temp("slice");
        self.emit_assign(
            &with_len,
            format!(
                "insertvalue {} {}, i64 {}, 1",
                self.llvm_type(&slice_ty),
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
        let operand_ty = left.ty.clone();
        let llvm_operand_ty = self.llvm_type(&operand_ty);

        let instruction = match op {
            BinOp::Add => format!("add {} {}, {}", llvm_operand_ty, left.repr, right.repr),
            BinOp::Sub => format!("sub {} {}, {}", llvm_operand_ty, left.repr, right.repr),
            BinOp::Mul => format!("mul {} {}, {}", llvm_operand_ty, left.repr, right.repr),
            BinOp::Div => {
                let opcode = if is_signed_integer_type(&operand_ty) {
                    "sdiv"
                } else {
                    "udiv"
                };
                format!("{opcode} {} {}, {}", llvm_operand_ty, left.repr, right.repr)
            }
            BinOp::Eq => format!(
                "icmp eq {} {}, {}",
                self.llvm_type(&left.ty),
                left.repr,
                right.repr
            ),
            BinOp::Ne => format!(
                "icmp ne {} {}, {}",
                self.llvm_type(&left.ty),
                left.repr,
                right.repr
            ),
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                let predicate = match (op, is_signed_integer_type(&operand_ty)) {
                    (BinOp::Lt, true) => "slt",
                    (BinOp::Le, true) => "sle",
                    (BinOp::Gt, true) => "sgt",
                    (BinOp::Ge, true) => "sge",
                    (BinOp::Lt, false) => "ult",
                    (BinOp::Le, false) => "ule",
                    (BinOp::Gt, false) => "ugt",
                    (BinOp::Ge, false) => "uge",
                    _ => unreachable!("logical ops handled separately"),
                };
                format!(
                    "icmp {} {} {}, {}",
                    predicate, llvm_operand_ty, left.repr, right.repr
                )
            }
            BinOp::Or | BinOp::And => unreachable!("handled separately"),
        };

        self.emit_assign(&temp, instruction);

        let ty = match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => operand_ty,
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
                self.emit_assign(
                    &temp,
                    format!("sub {} 0, {}", self.llvm_type(&expr.ty), expr.repr),
                );
                Ok(Value {
                    repr: temp,
                    ty: expr.ty,
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
                    format!("load {}, ptr {}", self.llvm_type(inner_ty), expr.repr),
                );
                Ok(Value {
                    repr: temp,
                    ty: (**inner_ty).clone(),
                })
            }
        }
    }

    fn emit_cast(&mut self, expr: &Expr, target_ty: &Type) -> Result<Value, String> {
        let value = self.emit_expr(expr)?;
        if value.ty == *target_ty {
            return Ok(value);
        }

        match (&value.ty, target_ty) {
            (Type::Str, Type::Ptr(inner_ty)) if **inner_ty == Type::U8 => Ok(Value {
                repr: value.repr,
                ty: target_ty.clone(),
            }),
            (Type::Ptr(inner_ty), Type::Str) if **inner_ty == Type::U8 => Ok(Value {
                repr: value.repr,
                ty: Type::Str,
            }),
            (Type::Ptr(_), Type::Ptr(_)) => Ok(Value {
                repr: value.repr,
                ty: target_ty.clone(),
            }),
            (Type::Ptr(_), Type::USize) => {
                let temp = self.fresh_temp("cast");
                self.emit_assign(&temp, format!("ptrtoint ptr {} to i64", value.repr));
                Ok(Value {
                    repr: temp,
                    ty: Type::USize,
                })
            }
            (Type::USize, Type::Ptr(_)) => {
                let temp = self.fresh_temp("cast");
                self.emit_assign(&temp, format!("inttoptr i64 {} to ptr", value.repr));
                Ok(Value {
                    repr: temp,
                    ty: target_ty.clone(),
                })
            }
            _ => self.emit_integer_cast(value, target_ty),
        }
    }

    fn emit_integer_cast(&mut self, value: Value, target_ty: &Type) -> Result<Value, String> {
        let Some(from_bits) = integer_bit_width(&value.ty) else {
            return Err(format!(
                "internal error: unsupported cast from {}",
                self.llvm_type(&value.ty)
            ));
        };
        let Some(to_bits) = integer_bit_width(target_ty) else {
            return Err(format!(
                "internal error: unsupported cast to {}",
                self.llvm_type(target_ty)
            ));
        };

        if value.ty == *target_ty {
            return Ok(value);
        }

        if *target_ty == Type::Bool {
            let temp = self.fresh_temp("cast");
            self.emit_assign(
                &temp,
                format!("icmp ne {} {}, 0", self.llvm_type(&value.ty), value.repr),
            );
            return Ok(Value {
                repr: temp,
                ty: Type::Bool,
            });
        }

        if value.ty == Type::Bool {
            let temp = self.fresh_temp("cast");
            self.emit_assign(
                &temp,
                format!("zext i1 {} to {}", value.repr, self.llvm_type(target_ty)),
            );
            return Ok(Value {
                repr: temp,
                ty: target_ty.clone(),
            });
        }

        if from_bits == to_bits {
            return Ok(Value {
                repr: value.repr,
                ty: target_ty.clone(),
            });
        }

        let temp = self.fresh_temp("cast");
        let instruction = if from_bits < to_bits {
            let opcode = if is_signed_integer_type(&value.ty) {
                "sext"
            } else {
                "zext"
            };
            format!(
                "{} {} {} to {}",
                opcode,
                self.llvm_type(&value.ty),
                value.repr,
                self.llvm_type(target_ty)
            )
        } else {
            format!(
                "trunc {} {} to {}",
                self.llvm_type(&value.ty),
                value.repr,
                self.llvm_type(target_ty)
            )
        };
        self.emit_assign(&temp, instruction);
        Ok(Value {
            repr: temp,
            ty: target_ty.clone(),
        })
    }

    fn emit_store(&mut self, value: &Value, ptr: &str) {
        self.emit_line(format!(
            "store {} {}, ptr {}",
            self.llvm_type(&value.ty),
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
            Type::U8 => self.emit_terminator("ret i8 0".to_string()),
            Type::USize => self.emit_terminator("ret i64 0".to_string()),
            Type::Bool => self.emit_terminator("ret i1 0".to_string()),
            Type::Str => self.emit_terminator("ret ptr null".to_string()),
            Type::Ptr(_) => self.emit_terminator("ret ptr null".to_string()),
            Type::Void => self.emit_terminator("ret void".to_string()),
            Type::Named(name)
                if self
                    .enum_layouts
                    .get(name)
                    .map(|layout| !layout.has_payload)
                    .unwrap_or(false) =>
            {
                self.emit_terminator("ret i32 0".to_string())
            }
            Type::Named(_) | Type::Array(_, _) | Type::Slice(_) => self.emit_terminator(format!(
                "ret {} zeroinitializer",
                self.llvm_type(&self.function.ret_type)
            )),
        }
    }

    fn emit_place(&mut self, expr: &Expr) -> Result<Place, String> {
        match expr {
            Expr::Var(name) => {
                let local = self.lookup_local(name).cloned().ok_or_else(|| {
                    if self.consts.contains_key(name) {
                        format!("internal error: constant '{name}' is not addressable")
                    } else {
                        format!("internal error: unknown variable '{name}'")
                    }
                })?;
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
                        self.llvm_type(&base.ty),
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
                let index_i64 = self.emit_index_value_as_i64(&index)?;

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
                                self.llvm_type(&base_value.ty),
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
                                self.llvm_type(&base_value.ty),
                                base_value.repr
                            ),
                        );

                        let element_ptr = self.fresh_temp("elem.ptr");
                        self.emit_assign(
                            &element_ptr,
                            format!(
                                "getelementptr inbounds {}, ptr {}, i64 {}",
                                self.llvm_type(element_ty),
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
                                self.llvm_type(element_ty),
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

    fn emit_index_value_as_i64(&mut self, index: &Value) -> Result<String, String> {
        match index.ty {
            Type::I32 => {
                let index_i64 = self.fresh_temp("idx");
                self.emit_assign(&index_i64, format!("sext i32 {} to i64", index.repr));
                Ok(index_i64)
            }
            Type::USize => Ok(index.repr.clone()),
            _ => Err("internal error: array index must be i32 or usize".to_string()),
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
            .push(format!("{} = alloca {}", slot, self.llvm_type(ty)));
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
