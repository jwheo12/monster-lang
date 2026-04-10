#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    I32,
    U8,
    USize,
    Bool,
    Str,
    Void,
    Named(String),
    Array(Box<Type>, usize),
    Slice(Box<Type>),
    Ptr(Box<Type>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BinOp {
    Or,
    And,
    Add,
    Sub,
    Mul,
    Div,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
    AddrOf,
    Deref,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    Int(i32),
    Bool(bool),
    Str(String),
    Var(String),
    SizeOf(Type),
    ArrayLiteral(Vec<Expr>),
    StructLiteral {
        name: String,
        fields: Vec<(String, Expr)>,
    },
    FieldAccess {
        base: Box<Expr>,
        field: String,
    },
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
    },
    Call {
        name: String,
        args: Vec<Expr>,
    },
    Binary {
        op: BinOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
    },
    Cast {
        expr: Box<Expr>,
        ty: Type,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stmt {
    Let {
        name: String,
        ty: Type,
        mutable: bool,
        value: Expr,
    },
    Assign {
        name: String,
        value: Expr,
    },
    AssignIndex {
        name: String,
        indices: Vec<Expr>,
        value: Expr,
    },
    AssignField {
        name: String,
        fields: Vec<String>,
        value: Expr,
    },
    AssignDeref {
        target: Expr,
        value: Expr,
    },
    Expr(Expr),
    If {
        condition: Expr,
        then_body: Vec<Stmt>,
        else_body: Option<Vec<Stmt>>,
    },
    While {
        condition: Expr,
        body: Vec<Stmt>,
    },
    Break,
    Continue,
    Return(Option<Expr>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumVariant {
    pub name: String,
    pub payload: Option<Type>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportDecl {
    pub path: String,
    pub alias: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructDef {
    pub name: String,
    pub fields: Vec<(String, Type)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumDef {
    pub name: String,
    pub variants: Vec<EnumVariant>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Function {
    pub name: String,
    pub params: Vec<(String, Type)>,
    pub ret_type: Type,
    pub body: Option<Vec<Stmt>>,
    pub is_extern: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
    pub imports: Vec<ImportDecl>,
    pub enums: Vec<EnumDef>,
    pub structs: Vec<StructDef>,
    pub functions: Vec<Function>,
}
