; Monster LLVM IR backend
target triple = "x86_64-pc-linux-gnu"

@.fmt.print_i32 = private unnamed_addr constant [3 x i8] c"%d\00"
@.fmt.print_ln_i32 = private unnamed_addr constant [4 x i8] c"%d\0A\00"
@.fmt.print_str = private unnamed_addr constant [3 x i8] c"%s\00"
@.fmt.scan_i32 = private unnamed_addr constant [3 x i8] c"%d\00"
@.str.true = private unnamed_addr constant [5 x i8] c"true\00"
@.str.false = private unnamed_addr constant [6 x i8] c"false\00"
@.str.read_i32_error = private unnamed_addr constant [42 x i8] c"Monster runtime error: expected i32 input\00"

declare i32 @printf(ptr, ...)
declare i32 @puts(ptr)
declare i32 @scanf(ptr, ...)
declare void @exit(i32)

define internal void @__monster_builtin_print_i32(i32 %value) {
entry:
  %call.0 = call i32 (ptr, ...) @printf(ptr getelementptr inbounds ([3 x i8], ptr @.fmt.print_i32, i64 0, i64 0), i32 %value)
  ret void
}

define internal void @__monster_builtin_print_ln_i32(i32 %value) {
entry:
  %call.1 = call i32 (ptr, ...) @printf(ptr getelementptr inbounds ([4 x i8], ptr @.fmt.print_ln_i32, i64 0, i64 0), i32 %value)
  ret void
}

define internal void @__monster_builtin_print_bool(i1 %value) {
entry:
  %str.0 = select i1 %value, ptr getelementptr inbounds ([5 x i8], ptr @.str.true, i64 0, i64 0), ptr getelementptr inbounds ([6 x i8], ptr @.str.false, i64 0, i64 0)
  %call.2 = call i32 (ptr, ...) @printf(ptr getelementptr inbounds ([3 x i8], ptr @.fmt.print_str, i64 0, i64 0), ptr %str.0)
  ret void
}

define internal void @__monster_builtin_print_ln_bool(i1 %value) {
entry:
  %str.1 = select i1 %value, ptr getelementptr inbounds ([5 x i8], ptr @.str.true, i64 0, i64 0), ptr getelementptr inbounds ([6 x i8], ptr @.str.false, i64 0, i64 0)
  %call.3 = call i32 @puts(ptr %str.1)
  ret void
}

define internal void @__monster_builtin_print_str(ptr %value) {
entry:
  %call.4 = call i32 (ptr, ...) @printf(ptr getelementptr inbounds ([3 x i8], ptr @.fmt.print_str, i64 0, i64 0), ptr %value)
  ret void
}

define internal void @__monster_builtin_print_ln_str(ptr %value) {
entry:
  %call.5 = call i32 @puts(ptr %value)
  ret void
}

define internal i32 @__monster_builtin_read_i32() {
entry:
  %value.addr = alloca i32
  %scan.0 = call i32 (ptr, ...) @scanf(ptr getelementptr inbounds ([3 x i8], ptr @.fmt.scan_i32, i64 0, i64 0), ptr %value.addr)
  %scan.ok = icmp eq i32 %scan.0, 1
  br i1 %scan.ok, label %read.ok, label %read.fail

read.fail:
  %call.2 = call i32 @puts(ptr getelementptr inbounds ([42 x i8], ptr @.str.read_i32_error, i64 0, i64 0))
  call void @exit(i32 1)
  unreachable

read.ok:
  %value.0 = load i32, ptr %value.addr
  ret i32 %value.0
}
%struct.VecI32 = type { ptr, i32, i32 }

declare ptr @malloc(i32)
declare ptr @realloc(ptr, i32)
declare void @free(ptr)

define %struct.VecI32 @vec_i32_new() {
entry:
  %bin.0 = mul i32 4, 4
  %call.1 = call ptr @malloc(i32 %bin.0)
  %insert.2 = insertvalue %struct.VecI32 poison, ptr %call.1, 0
  %insert.3 = insertvalue %struct.VecI32 %insert.2, i32 0, 1
  %insert.4 = insertvalue %struct.VecI32 %insert.3, i32 4, 2
  ret %struct.VecI32 %insert.4
}

define void @vec_i32_push(ptr %vec, i32 %value) {
entry:
  %vec.addr.0 = alloca ptr
  %value.addr.1 = alloca i32
  %current.addr.2 = alloca %struct.VecI32
  %new_cap.addr.3 = alloca i32
  %data.addr.4 = alloca ptr
  store ptr %vec, ptr %vec.addr.0
  store i32 %value, ptr %value.addr.1
  %load.0 = load ptr, ptr %vec.addr.0
  %deref.1 = load %struct.VecI32, ptr %load.0
  store %struct.VecI32 %deref.1, ptr %current.addr.2
  %load.2 = load %struct.VecI32, ptr %current.addr.2
  %field.3 = extractvalue %struct.VecI32 %load.2, 1
  %load.4 = load %struct.VecI32, ptr %current.addr.2
  %field.5 = extractvalue %struct.VecI32 %load.4, 2
  %bin.6 = icmp eq i32 %field.3, %field.5
  br i1 %bin.6, label %if.then.0, label %if.end.1
if.then.0:
  %load.7 = load %struct.VecI32, ptr %current.addr.2
  %field.8 = extractvalue %struct.VecI32 %load.7, 2
  %bin.9 = mul i32 %field.8, 2
  store i32 %bin.9, ptr %new_cap.addr.3
  %field.ptr.10 = getelementptr inbounds %struct.VecI32, ptr %current.addr.2, i32 0, i32 0
  %load.11 = load %struct.VecI32, ptr %current.addr.2
  %field.12 = extractvalue %struct.VecI32 %load.11, 0
  %load.13 = load i32, ptr %new_cap.addr.3
  %bin.14 = mul i32 %load.13, 4
  %call.15 = call ptr @realloc(ptr %field.12, i32 %bin.14)
  store ptr %call.15, ptr %field.ptr.10
  %field.ptr.16 = getelementptr inbounds %struct.VecI32, ptr %current.addr.2, i32 0, i32 2
  %load.17 = load i32, ptr %new_cap.addr.3
  store i32 %load.17, ptr %field.ptr.16
  br label %if.end.1
if.end.1:
  %load.18 = load %struct.VecI32, ptr %current.addr.2
  %field.19 = extractvalue %struct.VecI32 %load.18, 0
  store ptr %field.19, ptr %data.addr.4
  %load.20 = load %struct.VecI32, ptr %current.addr.2
  %field.21 = extractvalue %struct.VecI32 %load.20, 1
  %idx.22 = sext i32 %field.21 to i64
  %load.23 = load ptr, ptr %data.addr.4
  %elem.ptr.24 = getelementptr inbounds i32, ptr %load.23, i64 %idx.22
  %load.25 = load i32, ptr %value.addr.1
  store i32 %load.25, ptr %elem.ptr.24
  %field.ptr.26 = getelementptr inbounds %struct.VecI32, ptr %current.addr.2, i32 0, i32 1
  %load.27 = load %struct.VecI32, ptr %current.addr.2
  %field.28 = extractvalue %struct.VecI32 %load.27, 1
  %bin.29 = add i32 %field.28, 1
  store i32 %bin.29, ptr %field.ptr.26
  %load.30 = load ptr, ptr %vec.addr.0
  %load.31 = load %struct.VecI32, ptr %current.addr.2
  store %struct.VecI32 %load.31, ptr %load.30
  ret void
}

define i32 @vec_i32_get(%struct.VecI32 %vec, i32 %index) {
entry:
  %vec.addr.0 = alloca %struct.VecI32
  %index.addr.1 = alloca i32
  store %struct.VecI32 %vec, ptr %vec.addr.0
  store i32 %index, ptr %index.addr.1
  %load.0 = load %struct.VecI32, ptr %vec.addr.0
  %field.1 = extractvalue %struct.VecI32 %load.0, 0
  %load.2 = load i32, ptr %index.addr.1
  %idx.3 = sext i32 %load.2 to i64
  %elem.ptr.4 = getelementptr inbounds i32, ptr %field.1, i64 %idx.3
  %elem.5 = load i32, ptr %elem.ptr.4
  ret i32 %elem.5
}

define void @vec_i32_free(%struct.VecI32 %vec) {
entry:
  %vec.addr.0 = alloca %struct.VecI32
  store %struct.VecI32 %vec, ptr %vec.addr.0
  %load.0 = load %struct.VecI32, ptr %vec.addr.0
  %field.1 = extractvalue %struct.VecI32 %load.0, 0
  call void @free(ptr %field.1)
  ret void
}

define i32 @main() {
entry:
  %vec.addr.0 = alloca %struct.VecI32
  %result.addr.1 = alloca i32
  %call.0 = call %struct.VecI32 @vec_i32_new()
  store %struct.VecI32 %call.0, ptr %vec.addr.0
  call void @vec_i32_push(ptr %vec.addr.0, i32 10)
  call void @vec_i32_push(ptr %vec.addr.0, i32 20)
  call void @vec_i32_push(ptr %vec.addr.0, i32 30)
  call void @vec_i32_push(ptr %vec.addr.0, i32 40)
  call void @vec_i32_push(ptr %vec.addr.0, i32 50)
  %load.1 = load %struct.VecI32, ptr %vec.addr.0
  %field.2 = extractvalue %struct.VecI32 %load.1, 1
  call void @__monster_builtin_print_ln_i32(i32 %field.2)
  %load.3 = load %struct.VecI32, ptr %vec.addr.0
  %call.4 = call i32 @vec_i32_get(%struct.VecI32 %load.3, i32 0)
  call void @__monster_builtin_print_ln_i32(i32 %call.4)
  %load.5 = load %struct.VecI32, ptr %vec.addr.0
  %call.6 = call i32 @vec_i32_get(%struct.VecI32 %load.5, i32 4)
  call void @__monster_builtin_print_ln_i32(i32 %call.6)
  %load.7 = load %struct.VecI32, ptr %vec.addr.0
  %call.8 = call i32 @vec_i32_get(%struct.VecI32 %load.7, i32 0)
  %load.9 = load %struct.VecI32, ptr %vec.addr.0
  %call.10 = call i32 @vec_i32_get(%struct.VecI32 %load.9, i32 4)
  %bin.11 = add i32 %call.8, %call.10
  store i32 %bin.11, ptr %result.addr.1
  %load.12 = load %struct.VecI32, ptr %vec.addr.0
  call void @vec_i32_free(%struct.VecI32 %load.12)
  %load.13 = load i32, ptr %result.addr.1
  ret i32 %load.13
}

