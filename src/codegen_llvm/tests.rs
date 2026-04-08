use super::emit_program;
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::semantic::analyze_program;

fn emit_source(source: &str) -> String {
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize().expect("tokenize should succeed");
    let mut parser = Parser::new(tokens);
    let program = parser.parse_program().expect("parse should succeed");
    analyze_program(&program).expect("semantic analysis should succeed");
    emit_program(&program).expect("llvm emission should succeed")
}

#[test]
fn emits_function_and_arithmetic_ir() {
    let ir = emit_source(
        r#"
        fn add(a: i32, b: i32) -> i32 {
            return a + b;
        }

        fn main() -> i32 {
            return add(3, 4);
        }
        "#,
    );

    assert!(ir.contains("define i32 @add(i32 %a, i32 %b) {"));
    assert!(ir.contains("store i32 %a, ptr %a.addr.0"));
    assert!(ir.contains("call i32 @add(i32 3, i32 4)"));
}

#[test]
fn emits_control_flow_and_phi_for_logic() {
    let ir = emit_source(
        r#"
        fn main() -> i32 {
            let x: i32 = 1;
            if x == 1 || x == 2 {
                return 10;
            } else {
                return 20;
            }
        }
        "#,
    );

    assert!(ir.contains("phi i1"));
    assert!(ir.contains("br i1 %"));
    assert!(ir.contains("define i32 @main() {"));
}

#[test]
fn emits_builtin_io_calls() {
    let ir = emit_source(
        r#"
        fn main() -> i32 {
            let x: i32 = read_i32();
            print_i32(x);
            print_bool(true);
            print_str("Hello, World!");
            return 0;
        }
        "#,
    );

    assert!(ir.contains("define internal i32 @__monster_builtin_read_i32()"));
    assert!(ir.contains("call i32 @__monster_builtin_read_i32()"));
    assert!(ir.contains("call void @__monster_builtin_print_i32(i32"));
    assert!(ir.contains("call void @__monster_builtin_print_bool(i1 1)"));
    assert!(ir.contains("define internal void @__monster_builtin_print_str(ptr %value)"));
    assert!(ir.contains("@.str.user.0 = private unnamed_addr constant"));
    assert!(ir.contains("call void @__monster_builtin_print_str(ptr getelementptr"));
}

#[test]
fn emits_void_function_and_bare_return() {
    let ir = emit_source(
        r#"
        fn log_message() -> void {
            print_str("Hello");
            return;
        }

        fn main() -> i32 {
            log_message();
            return 0;
        }
        "#,
    );

    assert!(ir.contains("define void @log_message() {"));
    assert!(ir.contains("call void @log_message()"));
    assert!(ir.contains("ret void"));
}

#[test]
fn emits_extern_declaration_and_call() {
    let ir = emit_source(
        r#"
        extern fn abs(value: i32) -> i32;

        fn main() -> i32 {
            return abs(-7);
        }
        "#,
    );

    assert!(ir.contains("declare i32 @abs(i32)"));
    assert!(ir.contains("call i32 @abs(i32"));
    assert!(!ir.contains("define i32 @abs("));
}

#[test]
fn emits_struct_type_and_field_access() {
    let ir = emit_source(
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

    assert!(ir.contains("%struct.Pair = type { i32, i32 }"));
    assert!(ir.contains("insertvalue %struct.Pair poison, i32 10, 0"));
    assert!(ir.contains("extractvalue %struct.Pair"));
}

#[test]
fn emits_nested_struct_field_assignment() {
    let ir = emit_source(
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

    assert!(ir.contains("getelementptr inbounds %struct.Pair, ptr %pair.addr.0, i32 0, i32 0"));
    assert!(ir.contains("getelementptr inbounds %struct.Inner, ptr %field.ptr."));
    assert!(ir.contains("store i32 42, ptr %field.ptr."));
}

#[test]
fn emits_array_literal_and_runtime_indexing() {
    let ir = emit_source(
        r#"
        fn main() -> i32 {
            let values: [i32; 3] = [10, 20, 30];
            let index: i32 = 1;
            return values[index];
        }
        "#,
    );

    assert!(ir.contains("insertvalue [3 x i32] poison, i32 10, 0"));
    assert!(ir.contains("sext i32"));
    assert!(ir.contains("getelementptr inbounds [3 x i32]"));
    assert!(ir.contains("load i32, ptr"));
}

#[test]
fn emits_slice_creation_and_indexing() {
    let ir = emit_source(
        r#"
        fn head(values: [i32]) -> i32 {
            return values[0] + len(values);
        }

        fn main() -> i32 {
            let values: [i32; 3] = [10, 20, 30];
            return head(slice(values));
        }
        "#,
    );

    assert!(ir.contains("define i32 @head({ ptr, i32 } %values) {"));
    assert!(ir.contains("insertvalue { ptr, i32 } poison, ptr"));
    assert!(ir.contains("insertvalue { ptr, i32 } %slice."));
    assert!(ir.contains("extractvalue { ptr, i32 } %"));
    assert!(ir.contains("call i32 @head({ ptr, i32 }"));
}

#[test]
fn emits_pointer_address_deref_and_indexing() {
    let ir = emit_source(
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

    assert!(ir.contains("store ptr %elem.ptr."));
    assert!(ir.contains("load ptr, ptr %p.addr."));
    assert!(ir.contains("getelementptr inbounds i32, ptr %"));
    assert!(ir.contains("store i32 99, ptr %elem.ptr."));
    assert!(ir.contains("store i32 42, ptr %"));
}

#[test]
fn emits_manual_vec_i32_with_libc_allocation() {
    let ir = emit_source(
        r#"
        struct VecI32 {
            data: *i32,
            len: i32,
            cap: i32,
        }

        extern fn malloc(size: i32) -> *i32;
        extern fn realloc(ptr: *i32, size: i32) -> *i32;
        extern fn free(ptr: *i32) -> void;

        fn vec_i32_new() -> VecI32 {
            return VecI32 { data: malloc(4 * 4), len: 0, cap: 4 };
        }

        fn vec_i32_push(vec: *VecI32, value: i32) -> void {
            let mut current: VecI32 = *vec;

            if current.len == current.cap {
                let new_cap: i32 = current.cap * 2;
                current.data = realloc(current.data, new_cap * 4);
                current.cap = new_cap;
            }

            let mut data: *i32 = current.data;
            data[current.len] = value;
            current.len = current.len + 1;
            *vec = current;
            return;
        }

        fn vec_i32_free(vec: VecI32) -> void {
            free(vec.data);
            return;
        }

        fn main() -> i32 {
            let mut vec: VecI32 = vec_i32_new();
            vec_i32_push(&vec, 10);
            vec_i32_push(&vec, 20);
            vec_i32_free(vec);
            return vec.len;
        }
        "#,
    );

    assert!(ir.contains("declare ptr @malloc(i32)"));
    assert!(ir.contains("declare ptr @realloc(ptr, i32)"));
    assert!(ir.contains("declare void @free(ptr)"));
    assert!(ir.contains("call ptr @malloc(i32 "));
    assert!(ir.contains("call ptr @realloc(ptr"));
    assert!(ir.contains("call void @free(ptr"));
}

#[test]
fn emits_array_index_assignment_and_len_builtin() {
    let ir = emit_source(
        r#"
        fn main() -> i32 {
            let mut values: [i32; 3] = [10, 20, 30];
            values[1] = 99;
            print_i32(len(values));
            return values[1];
        }
        "#,
    );

    assert!(ir.contains("getelementptr inbounds [3 x i32], ptr %values.addr.0, i64 0, i64 %idx."));
    assert!(ir.contains("store i32 99, ptr %elem.ptr."));
    assert!(ir.contains("call void @__monster_builtin_print_i32(i32 3)"));
}

#[test]
fn emits_nested_array_index_assignment() {
    let ir = emit_source(
        r#"
        fn main() -> i32 {
            let mut matrix: [[i32; 2]; 2] = [[1, 2], [3, 4]];
            matrix[1][0] = 99;
            return matrix[1][0];
        }
        "#,
    );

    assert!(
        ir.contains("getelementptr inbounds [2 x [2 x i32]], ptr %matrix.addr.0, i64 0, i64 %idx.")
    );
    assert!(ir.contains("getelementptr inbounds [2 x i32], ptr %elem.ptr."));
    assert!(ir.contains("store i32 99, ptr %elem.ptr."));
}
