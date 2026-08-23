// theseus/error.rs — theseus 错误类型
//
// 提供 theseus 整合包下载生态所需的错误类型：
//   Error      — 统一错误类型（包装 ErrorKind，raw 字段兼容 theseus 调用约定）
//   ErrorKind  — 错误种类枚举（涵盖 theseus 全部变体）
//   LabrinthError — theseus 原生 API 错误（保留命名 + 结构体字段）
//   SharedInstanceUnavailableReason — 共享实例不可用原因（Copy 枚举）
//   Result     — theseus 生态统一 Result 别名

use std::fmt;

/// theseus 生态统一 Result 别名
pub type Result<T> = std::result::Result<T, Error>;

/// 共享实例不可用原因（theseus 兼容枚举，Copy 以支持解引用）
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SharedInstanceUnavailableReason {
    NotReady,
    Missing,
    Disabled,
    Unknown,
}

/// theseus 原生 API 错误结构体（保留 theseus 命名以兼容代码引用）
///
/// model.rs 中 `ErrorKind::LabrinthError(error)` 解构出此结构体，
/// 访问其 `error`/`status`/`method`/`url`/`route` 字段。
#[derive(Debug, Clone)]
pub struct LabrinthError {
    pub error: String,
    pub status: Option<u16>,
    pub method: Option<String>,
    pub url: Option<String>,
    pub route: Option<String>,
}

impl fmt::Display for LabrinthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(status) = self.status {
            write!(f, "[{}] {}", status, self.error)
        } else {
            write!(f, "{}", self.error)
        }
    }
}

impl std::error::Error for LabrinthError {}

/// theseus 错误种类（涵盖 theseus 生态全部变体）
#[derive(Debug)]
pub enum ErrorKind {
    /// 输入参数错误
    InputError(String),
    /// 被限流
    Ratelimited { retry_in_seconds: u64 },
    /// IO 错误（tokio/std IO 错误包装）
    IoError(std::io::Error),
    /// HTTP 请求错误
    HttpError(String),
    /// ZIP 处理错误
    ZipError(String),
    /// JSON 解析错误（theseus 命名风格）
    JsonError(String),
    /// JSON 解析错误（theseus 代码用 JSONError 引用）
    JSONError(String),
    /// 文件未找到
    FileNotFoundError(String),
    /// 校验失败（SHA1/大小不匹配）
    HashMismatch { expected: String, actual: String },
    /// 哈希错误（theseus 用 HashError(expected, actual)）
    HashError(String, String),
    /// 取消信号
    Cancelled,
    /// 其他错误（简短描述）
    Other(String),
    /// 其他错误（theseus 用 OtherError 引用）
    OtherError(String),
    /// 任意错误（包装 eyre::Report 的字符串形式）
    Any(String),

    // ===== theseus 兼容变体（runner.rs / diagnostics.rs 等引用）=====
    /// 共享实例不可用
    SharedInstanceUnavailable(SharedInstanceUnavailableReason),
    /// 启动器错误
    LauncherError(String),
    /// Java 运行时错误
    JREError(String),
    /// 缺少必要值
    NoValueFor(String),
    /// 元数据错误
    MetadataError(String),
    /// 下载错误
    FetchError(String),
    /// API 不可用
    ApiIsDownError(String),
    /// WebSocket 错误
    WSError(String),
    /// WebSocket 关闭错误
    WSClosedError(String),
    /// Labrinth API 错误
    LabrinthError(LabrinthError),
    /// 反序列化错误
    DeserializationError(String),
    /// 路径前缀剥离错误
    StripPrefixError(String),
    /// 文件系统错误
    FSError(String),
    /// 标准 IO 错误
    StdIOError(std::io::Error),
    /// UTF-8 编码错误
    UTFError(String),
    /// INI 解析错误
    INIError(String),
    /// SQLx 数据库错误
    Sqlx(String),
    /// SQLx 迁移错误
    SqlxMigrate(String),
    /// 任务 join 错误
    JoinError(String),
    /// 通道接收错误
    RecvError(String),
    /// 信号量获取错误
    AcquireError(String),
    /// 事件错误
    EventError(String),
}

impl ErrorKind {
    pub fn as_input_error(s: impl Into<String>) -> Self {
        ErrorKind::InputError(s.into())
    }
}

/// theseus 统一错误类型
///
/// `raw` 字段使用 `Box<ErrorKind>` 以兼容 theseus 代码中
/// `error.raw.as_ref()` 的调用约定（返回 `&ErrorKind`）。
#[derive(Debug)]
pub struct Error {
    pub raw: Box<ErrorKind>,
    pub context: Option<String>,
}

impl Error {
    pub fn from(kind: ErrorKind) -> Self {
        Self {
            raw: Box::new(kind),
            context: None,
        }
    }

    pub fn with_context(mut self, ctx: impl Into<String>) -> Self {
        self.context = Some(ctx.into());
        self
    }

    /// 获取错误种类引用（theseus 兼容接口）
    pub fn kind(&self) -> &ErrorKind {
        &self.raw
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.raw.as_ref() {
            ErrorKind::InputError(s) => write!(f, "输入错误: {}", s),
            ErrorKind::Ratelimited { retry_in_seconds } => {
                write!(f, "被限流，{} 秒后重试", retry_in_seconds)
            }
            ErrorKind::IoError(e) => write!(f, "IO 错误: {}", e),
            ErrorKind::HttpError(s) => write!(f, "HTTP 错误: {}", s),
            ErrorKind::ZipError(s) => write!(f, "ZIP 错误: {}", s),
            ErrorKind::JsonError(s) => write!(f, "JSON 错误: {}", s),
            ErrorKind::JSONError(s) => write!(f, "JSON 错误: {}", s),
            ErrorKind::FileNotFoundError(s) => write!(f, "文件未找到: {}", s),
            ErrorKind::HashMismatch { expected, actual } => {
                write!(f, "校验失败: 期望 {}, 实际 {}", expected, actual)
            }
            ErrorKind::HashError(expected, actual) => {
                write!(f, "哈希错误: 期望 {}, 实际 {}", expected, actual)
            }
            ErrorKind::Cancelled => write!(f, "操作已取消"),
            ErrorKind::Other(s) => write!(f, "{}", s),
            ErrorKind::OtherError(s) => write!(f, "{}", s),
            ErrorKind::Any(s) => write!(f, "{}", s),
            ErrorKind::SharedInstanceUnavailable(reason) => {
                write!(f, "共享实例不可用: {:?}", reason)
            }
            ErrorKind::LauncherError(s) => write!(f, "启动器错误: {}", s),
            ErrorKind::JREError(s) => write!(f, "Java 错误: {}", s),
            ErrorKind::NoValueFor(s) => write!(f, "缺少值: {}", s),
            ErrorKind::MetadataError(s) => write!(f, "元数据错误: {}", s),
            ErrorKind::FetchError(s) => write!(f, "下载错误: {}", s),
            ErrorKind::ApiIsDownError(s) => write!(f, "API 不可用: {}", s),
            ErrorKind::WSError(s) => write!(f, "WebSocket 错误: {}", s),
            ErrorKind::WSClosedError(s) => {
                write!(f, "WebSocket 已关闭: {}", s)
            }
            ErrorKind::LabrinthError(e) => write!(f, "Labrinth 错误: {}", e),
            ErrorKind::DeserializationError(s) => {
                write!(f, "反序列化错误: {}", s)
            }
            ErrorKind::StripPrefixError(s) => {
                write!(f, "路径前缀错误: {}", s)
            }
            ErrorKind::FSError(s) => write!(f, "文件系统错误: {}", s),
            ErrorKind::StdIOError(e) => write!(f, "标准 IO 错误: {}", e),
            ErrorKind::UTFError(s) => write!(f, "UTF-8 编码错误: {}", s),
            ErrorKind::INIError(s) => write!(f, "INI 解析错误: {}", s),
            ErrorKind::Sqlx(s) => write!(f, "数据库错误: {}", s),
            ErrorKind::SqlxMigrate(s) => write!(f, "数据库迁移错误: {}", s),
            ErrorKind::JoinError(s) => write!(f, "任务 join 错误: {}", s),
            ErrorKind::RecvError(s) => write!(f, "通道接收错误: {}", s),
            ErrorKind::AcquireError(s) => write!(f, "信号量获取错误: {}", s),
            ErrorKind::EventError(s) => write!(f, "事件错误: {}", s),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::from(ErrorKind::IoError(e))
    }
}

impl From<ErrorKind> for Error {
    fn from(kind: ErrorKind) -> Self {
        Self::from(kind)
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Self::from(ErrorKind::JsonError(e.to_string()))
    }
}

impl From<reqwest::Error> for Error {
    fn from(e: reqwest::Error) -> Self {
        Self::from(ErrorKind::HttpError(e.to_string()))
    }
}

impl From<async_zip::error::ZipError> for Error {
    fn from(e: async_zip::error::ZipError) -> Self {
        Self::from(ErrorKind::ZipError(e.to_string()))
    }
}

impl From<eyre::Report> for Error {
    fn from(e: eyre::Report) -> Self {
        Self::from(ErrorKind::Any(e.to_string()))
    }
}

impl From<LabrinthError> for Error {
    fn from(e: LabrinthError) -> Self {
        Self::from(ErrorKind::LabrinthError(e))
    }
}

impl From<tauri::Error> for Error {
    fn from(e: tauri::Error) -> Self {
        Self::from(ErrorKind::EventError(e.to_string()))
    }
}

// 修复 E0277：补充 theseus 代码用到的 ? 转换 From impl
// 这些 impl 让 ? 操作符能将对应错误自动转换为 theseus::error::Error

/// 信号量获取错误转换（修复 install_mrpack.rs / runner.rs 中 acquire().await? 调用）
impl From<tokio::sync::AcquireError> for Error {
    fn from(e: tokio::sync::AcquireError) -> Self {
        Self::from(ErrorKind::AcquireError(e.to_string()))
    }
}

/// 路径前缀剥离错误转换（修复 recovery.rs 中 strip_prefix().await? 调用）
impl From<std::path::StripPrefixError> for Error {
    fn from(e: std::path::StripPrefixError) -> Self {
        Self::from(ErrorKind::StripPrefixError(e.to_string()))
    }
}

/// 任务 join 错误转换（修复 shared_instance.rs 中 spawn_blocking().await?? 调用）
impl From<tokio::task::JoinError> for Error {
    fn from(e: tokio::task::JoinError) -> Self {
        Self::from(ErrorKind::JoinError(e.to_string()))
    }
}
