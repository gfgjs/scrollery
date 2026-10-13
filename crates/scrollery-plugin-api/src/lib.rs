//! 宿主凭据服务共用的命名常量。

#![forbid(unsafe_code)]

/// 存储后端密码与校对 API key 共用的系统凭据服务名。
/// 保持名称稳定，避免已保存的用户凭据变得不可访问。
pub const KEYRING_SERVICE: &str = "scrollery";
