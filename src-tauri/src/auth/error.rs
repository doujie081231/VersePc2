// auth/error.rs — 认证错误类型与 XErr 错误码映射

use serde::{ser::SerializeStruct, Serialize, Serializer};

/// 认证错误
#[derive(Debug)]
pub enum AuthError {
    /// 网络请求失败
    Network(String),
    /// OAuth 设备码过期
    DeviceCodeExpired,
    /// 用户尚未授权（轮询中）
    AuthorizationPending,
    /// 用户拒绝授权
    AuthorizationDeclined,
    /// 速率限制，需等待 N 秒
    RateLimit(u64),
    /// Token 过期或无效，需要重新登录
    TokenExpired,
    /// XSTS 返回 XErr 错误码
    XErr(u64, String),
    /// 没有购买 Minecraft
    NeedPurchase,
    /// 没有创建 Minecraft 档案
    NeedCreateProfile,
    /// 其他错误
    Other(String),
}

impl AuthError {
    /// 错误代码字符串（供前端判断）
    pub fn code(&self) -> &'static str {
        match self {
            Self::Network(_) => "network",
            Self::DeviceCodeExpired => "device_code_expired",
            Self::AuthorizationPending => "authorization_pending",
            Self::AuthorizationDeclined => "authorization_declined",
            Self::RateLimit(_) => "rate_limit",
            Self::TokenExpired => "invalid_grant",
            Self::XErr(_, _) => "xerr",
            Self::NeedPurchase => "need_purchase",
            Self::NeedCreateProfile => "need_create_profile",
            Self::Other(_) => "other",
        }
    }

    /// 是否需要重新登录
    pub fn need_relogin(&self) -> bool {
        matches!(self, Self::TokenExpired | Self::XErr(_, _))
    }

    /// 用户可读的错误消息
    pub fn message(&self) -> String {
        match self {
            Self::Network(e) => format!("网络请求失败：{}", e),
            Self::DeviceCodeExpired => "设备码已过期，请重新获取".to_string(),
            Self::AuthorizationPending => "等待用户授权中".to_string(),
            Self::AuthorizationDeclined => "用户拒绝了授权".to_string(),
            Self::RateLimit(secs) => format!("请求过于频繁，请等待 {} 秒后重试", secs),
            Self::TokenExpired => "登录已过期，请重新登录".to_string(),
            Self::XErr(code, msg) => format!("Xbox Live 错误（{}）：{}", code, msg),
            Self::NeedPurchase => "您尚未购买 Minecraft，请先购买游戏".to_string(),
            Self::NeedCreateProfile => "您的账号尚未创建 Minecraft 档案，请先在 Minecraft 官网创建档案".to_string(),
            Self::Other(e) => format!("未知错误：{}", e),
        }
    }
}

impl Serialize for AuthError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut s = serializer.serialize_struct("AuthError", 3)?;
        s.serialize_field("code", self.code())?;
        s.serialize_field("message", &self.message())?;
        s.serialize_field("needRelogin", &self.need_relogin())?;
        s.end()
    }
}

/// XErr 错误码 → 中文提示
pub fn xerr_message(xerr: u64) -> &'static str {
    match xerr {
        2148916233 => "该微软账号没有关联 Xbox 账号，请先在 Xbox 官网关联",
        2148916234 => "该地区不可用，请尝试更换网络或使用代理",
        2148916235 => "Xbox Live 服务暂时不可用，请稍后重试",
        2148916236 => "该账号需要成人验证，请先完成 Xbox 成人验证",
        2148916237 => "该账号已被封禁",
        2148916238 => "该账号是儿童账号，需要成人关联才能使用",
        _ => "未知 Xbox 错误",
    }
}

/// 判断是否是速率限制（HTTP 429）
pub fn parse_rate_limit_retry_after(retry_after_header: &str) -> u64 {
    retry_after_header
        .parse::<u64>()
        .unwrap_or(5)
        .max(1)
}
