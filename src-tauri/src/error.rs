use rusqlite::ErrorCode;
use serde::ser::Serializer;
use serde::Serialize;

/// 后端统一错误。
///
/// 序列化为 `"code:message"` 字符串，前端 `normalizeError` 依此映射为
/// `ApiError { code, message, retryable }`，两端错误语义保持一致。
#[derive(Debug, Clone)]
pub struct AppError {
    code: &'static str,
    message: String,
}

impl AppError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn unavailable(scope: &str) -> Self {
        Self::new(
            "unavailable",
            format!("{scope} 尚未接入 Rust 后端（第一阶段只实现前端与接口预留）"),
        )
    }

    pub fn not_found(target: impl std::fmt::Display) -> Self {
        Self::new("not_found", format!("找不到 {target}"))
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new("invalid_argument", message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new("internal", message)
    }

    /// 本地数据库比当前程序更新：拒绝启动，避免旧代码写坏新结构。
    ///
    /// 独立错误码（而不是借用 `unavailable`）：这是"需要升级应用"的确定性状态，
    /// 前端必须给出可执行的提示，而不是"服务暂时不可用，请重试"。
    pub fn database_too_new(db_version: i64, code_version: i64) -> Self {
        Self::new(
            "database_too_new",
            format!(
                "本地数据库版本（{db_version}）高于当前程序（{code_version}），请先升级 Sage"
            ),
        )
    }

    pub fn corrupt(detail: impl std::fmt::Display) -> Self {
        Self::new(
            "internal",
            format!("本地数据库已损坏，请从备份恢复（{detail}）"),
        )
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.code, self.message)
    }
}

impl std::error::Error for AppError {}

/// 数据库错误 → 统一错误。
///
/// 注意：不做任何用户输入的回显，`detail` 只带错误码与 SQLite 的原始描述。
impl From<rusqlite::Error> for AppError {
    fn from(err: rusqlite::Error) -> Self {
        use rusqlite::Error::*;
        match err {
            SqliteFailure(inner, message) => {
                let text = message.unwrap_or_else(|| format!("{:?}", inner.code));
                match inner.code {
                    ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked => Self::new(
                        "unavailable",
                        "本地数据库正忙，请稍后重试",
                    ),
                    ErrorCode::ConstraintViolation => {
                        Self::new("invalid_argument", format!("数据校验未通过：{text}"))
                    }
                    ErrorCode::ReadOnly => Self::new("unavailable", "本地数据库处于只读状态"),
                    ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase => Self::corrupt(&text),
                    _ => Self::internal(format!("数据库错误：{text}")),
                }
            }
            QueryReturnedNoRows => Self::not_found("记录"),
            other => Self::internal(format!("数据库错误：{other}")),
        }
    }
}



impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;
