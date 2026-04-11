use super::emit_program;
use crate::ast::{Expr, Function, Program, Stmt, Type};
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
fn sanitizes_namespaced_function_symbols() {
    let program = Program {
        imports: Vec::new(),
        consts: Vec::new(),
        enums: Vec::new(),
        structs: Vec::new(),
        functions: vec![
            Function {
                name: "lib.math.add".to_string(),
                params: vec![("a".to_string(), Type::I32), ("b".to_string(), Type::I32)],
                ret_type: Type::I32,
                body: Some(vec![Stmt::Return(Some(Expr::Binary {
                    op: crate::ast::BinOp::Add,
                    left: Box::new(Expr::Var("a".to_string())),
                    right: Box::new(Expr::Var("b".to_string())),
                }))]),
                is_extern: false,
            },
            Function {
                name: "main".to_string(),
                params: vec![],
                ret_type: Type::I32,
                body: Some(vec![Stmt::Return(Some(Expr::Call {
                    name: "lib.math.add".to_string(),
                    args: vec![Expr::Int(3), Expr::Int(4)],
                }))]),
                is_extern: false,
            },
        ],
    };

    analyze_program(&program).expect("semantic analysis should succeed");
    let ir = emit_program(&program).expect("llvm emission should succeed");

    assert!(ir.contains("define i32 @lib__math__add(i32 %a, i32 %b) {"));
    assert!(ir.contains("call i32 @lib__math__add(i32 3, i32 4)"));
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
fn emits_break_and_continue_in_while_loop() {
    let ir = emit_source(
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

    assert!(ir.contains("br label %while.end."));
    assert!(ir.contains("br label %while.cond."));
    assert!(ir.contains("while.body."));
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
            print_ln_i32(x);
            print_ln_bool(true);
            print_ln_str("Hello, World!");
            return 0;
        }
        "#,
    );

    assert!(ir.contains("define internal i32 @__monster_builtin_read_i32()"));
    assert!(ir.contains("call i32 @__monster_builtin_read_i32()"));
    assert!(ir.contains("call void @__monster_builtin_print_i32(i32"));
    assert!(ir.contains("call void @__monster_builtin_print_bool(i1 1)"));
    assert!(ir.contains("call void @__monster_builtin_print_ln_i32(i32"));
    assert!(ir.contains("call void @__monster_builtin_print_ln_bool(i1 1)"));
    assert!(ir.contains("define internal void @__monster_builtin_print_str(ptr %value)"));
    assert!(ir.contains("define internal void @__monster_builtin_print_ln_str(ptr %value)"));
    assert!(ir.contains("@.str.user.0 = private unnamed_addr constant"));
    assert!(ir.contains("call void @__monster_builtin_print_str(ptr getelementptr"));
    assert!(ir.contains("call void @__monster_builtin_print_ln_str(ptr getelementptr"));
}

#[test]
fn emits_file_io_builtins() {
    let ir = emit_source(
        r#"
        extern fn free(ptr: *u8) -> void;

        fn main() -> i32 {
            let mut len: usize = 0 as usize;
            let data: *u8 = read_file("exam.mnst", &len);
            write_file("target/mst/exam.copy", data, len);
            free(data);
            return len as i32;
        }
        "#,
    );

    assert!(
        ir.contains("define internal ptr @__monster_builtin_read_file(ptr %path, ptr %out_len)")
    );
    assert!(ir.contains(
        "define internal void @__monster_builtin_write_file(ptr %path, ptr %data, i64 %len)"
    ));
    assert!(ir.contains("declare ptr @fopen(ptr, ptr)"));
    assert!(ir.contains("declare i64 @fread(ptr, i64, i64, ptr)"));
    assert!(ir.contains("declare i64 @fwrite(ptr, i64, i64, ptr)"));
    assert!(ir.contains("call ptr @__monster_builtin_read_file(ptr getelementptr"));
    assert!(ir.contains("call void @__monster_builtin_write_file(ptr getelementptr"));
}

#[test]
fn emits_string_and_byte_utility_builtins() {
    let ir = emit_source(
        r#"
        extern fn calloc(count: usize, size: usize) -> *u8;
        extern fn free(ptr: *u8) -> void;

        fn main() -> i32 {
            let src: str = "Monster";
            let len: usize = strlen(src);
            let buf: *u8 = calloc(len + (1 as usize), sizeof(u8));
            memcpy(buf, src as *u8, len + (1 as usize));

            if str_eq(src, buf as str) && memcmp(buf, src as *u8, len) == 0 {
                free(buf);
                return len as i32;
            }

            free(buf);
            return 0;
        }
        "#,
    );

    assert!(ir.contains("define internal i64 @__monster_builtin_strlen(ptr %value)"));
    assert!(
        ir.contains("define internal i32 @__monster_builtin_memcmp(ptr %lhs, ptr %rhs, i64 %len)")
    );
    assert!(
        ir.contains("define internal void @__monster_builtin_memcpy(ptr %dst, ptr %src, i64 %len)")
    );
    assert!(ir.contains("define internal i1 @__monster_builtin_str_eq(ptr %lhs, ptr %rhs)"));
    assert!(ir.contains("call i64 @__monster_builtin_strlen(ptr"));
    assert!(ir.contains("call void @__monster_builtin_memcpy(ptr"));
    assert!(ir.contains("call i32 @__monster_builtin_memcmp(ptr"));
    assert!(ir.contains("call i1 @__monster_builtin_str_eq(ptr"));
}

#[test]
fn emits_match_for_payload_enum() {
    let ir = emit_source(
        r#"
        enum Token {
            Int(i32),
            Name(str),
            Eof,
        }

        fn unwrap(token: Token) -> i32 {
            return match token {
                Int(value) => value,
                Name(_) => 0,
                Eof => 0,
            };
        }

        fn main() -> i32 {
            return unwrap(Int(42));
        }
        "#,
    );

    assert!(ir.contains("match.arm."));
    assert!(ir.contains("match.end."));
    assert!(ir.contains("phi i32"));
    assert!(ir.contains("icmp eq i32"));
    assert!(ir.contains("store i32 42, ptr %enum.payload.ptr."));
}

#[test]
fn emits_void_function_and_bare_return() {
    let ir = emit_source(
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
            return values[0] + (len(values) as i32);
        }

        fn main() -> i32 {
            let values: [i32; 3] = [10, 20, 30];
            return head(slice(values));
        }
        "#,
    );

    assert!(ir.contains("define i32 @head({ ptr, i64 } %values) {"));
    assert!(ir.contains("insertvalue { ptr, i64 } poison, ptr"));
    assert!(ir.contains("insertvalue { ptr, i64 } %slice."));
    assert!(ir.contains("extractvalue { ptr, i64 } %"));
    assert!(ir.contains("call i32 @head({ ptr, i64 }"));
    assert!(ir.contains("trunc i64 %slice.len."));
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
            print_ln_i32(len(values) as i32);
            return values[1];
        }
        "#,
    );

    assert!(ir.contains("getelementptr inbounds [3 x i32], ptr %values.addr.0, i64 0, i64 %idx."));
    assert!(ir.contains("store i32 99, ptr %elem.ptr."));
    assert!(ir.contains("trunc i64 3 to i32"));
    assert!(ir.contains("call void @__monster_builtin_print_ln_i32(i32 %cast."));
}

#[test]
fn emits_u8_usize_and_as_casts() {
    let ir = emit_source(
        r#"
        fn main() -> i32 {
            let byte: u8 = 255 as u8;
            let size: usize = len([1, 2, 3]);
            let total: usize = (byte as usize) + size;
            return total as i32;
        }
        "#,
    );

    assert!(ir.contains("trunc i32 255 to i8"));
    assert!(ir.contains("zext i8 %"));
    assert!(ir.contains("add i64 %"));
    assert!(ir.contains("trunc i64 %"));
}

#[test]
fn emits_global_const_values_inline() {
    let ir = emit_source(
        r#"
        const LIMIT: usize = 64 as usize;
        const GREETING: str = "const works";

        fn main() -> i32 {
            print_ln_str(GREETING);
            return LIMIT as i32;
        }
        "#,
    );

    assert!(ir.contains("@.str.user."));
    assert!(ir.contains("const works"));
    assert!(ir.contains("call void @__monster_builtin_print_ln_str(ptr getelementptr"));
    assert!(ir.contains("sext i32 64 to i64"));
    assert!(ir.contains("trunc i64 %cast."));
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

#[test]
fn emits_main_with_argc_and_argv() {
    let ir = emit_source(
        r#"
        fn main(argc: i32, argv: **u8) -> i32 {
            let first: *u8 = argv[0];
            print_ln_str(first as str);
            return argc;
        }
        "#,
    );

    assert!(ir.contains("define i32 @main(i32 %argc, ptr %argv) {"));
    assert!(ir.contains("getelementptr inbounds ptr, ptr %"));
    assert!(ir.contains("call void @__monster_builtin_print_ln_str(ptr %"));
}

#[test]
fn emits_c_like_enum_values_and_comparison() {
    let ir = emit_source(
        r#"
        enum Color {
            Red,
            Green,
            Blue,
        }

        fn is_red(color: Color) -> bool {
            return color == Red;
        }

        fn main() -> i32 {
            let color: Color = Green;

            if is_red(color) {
                return 1;
            }

            return 0;
        }
        "#,
    );

    assert!(ir.contains("define i1 @is_red(i32 %color) {"));
    assert!(ir.contains("store i32 1, ptr %color.addr."));
    assert!(ir.contains("icmp eq i32 %"));
    assert!(!ir.contains("%struct.Color = type"));
}

#[test]
fn emits_payload_enum_construction_and_access() {
    let ir = emit_source(
        r#"
        enum Token {
            Int(i32),
            Eof,
        }

        fn main() -> i32 {
            let token: Token = Int(42);

            if is(token, Int) {
                return payload(token, Int);
            } else {
                return 0;
            }
        }
        "#,
    );

    assert!(ir.contains("%enum.Token = type { i32, i32, [1 x i64] }"));
    assert!(ir.contains("store %enum.Token"));
    assert!(ir.contains("store i32 0, ptr %enum.tag.ptr."));
    assert!(ir.contains("store i32 42, ptr %enum.payload.ptr."));
    assert!(ir.contains("load i32, ptr %enum.payload.ptr."));
}

#[test]
fn emits_sizeof_for_scalars_and_structs() {
    let ir = emit_source(
        r#"
        struct Pair {
            left: i32,
            right: i32,
        }

        fn main() -> i32 {
            let a: usize = sizeof(i32);
            let b: usize = sizeof(Pair);
            return (a + b) as i32;
        }
        "#,
    );

    assert!(ir.contains("getelementptr i32, ptr null, i64 1"));
    assert!(ir.contains("getelementptr %struct.Pair, ptr null, i64 1"));
    assert!(ir.contains("ptrtoint ptr %sizeof.ptr."));
}
