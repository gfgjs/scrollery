//! 插件发行包与 registry 的公钥验签原语。
//! 私钥不进入本 crate；发布时通过 PICASA_EXOTIC_KEYSET_FILE 注入公钥集。

#![forbid(unsafe_code)]

pub mod crypto;

pub use crypto::{CryptoError, KeyPurpose, KeyStatus, VerifyingKeyset};
