//! 解析后的值与取值来源。

use std::collections::HashMap;

/// 解析后的单键值。键的 kind 由注册表保证，取值时按 kind 走对应入口。
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Str(String),
    Int(i64),
    Float(f64),
    Bool(bool),
}

impl Value {
    pub(crate) fn as_str(&self) -> &str {
        match self {
            Value::Str(s) => s,
            other => panic!("配置键的 kind 不是字符串，却是 {other:?}"),
        }
    }
    pub(crate) fn as_int(&self) -> i64 {
        match self {
            Value::Int(v) => *v,
            other => panic!("配置键的 kind 不是整数，却是 {other:?}"),
        }
    }
    pub(crate) fn as_float(&self) -> f64 {
        match self {
            Value::Float(v) => *v,
            other => panic!("配置键的 kind 不是浮点数，却是 {other:?}"),
        }
    }
    pub(crate) fn as_bool(&self) -> bool {
        match self {
            Value::Bool(v) => *v,
            other => panic!("配置键的 kind 不是布尔，却是 {other:?}"),
        }
    }
}

/// 一个键的取值来自哪一层。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Default,
    Toml,
    Env,
}

impl Source {
    pub fn label(self) -> &'static str {
        match self {
            Source::Default => "默认",
            Source::Toml => "config.toml",
            Source::Env => "env",
        }
    }
}

/// 键 → 来源。随 [`Config`](crate::Config) 一起加载，供启动摘要和排障显示。
pub type Sources = HashMap<&'static str, Source>;
