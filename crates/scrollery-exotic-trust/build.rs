// 内置信任根 keyset 的编译期装配点(部署配置注入)。
//
// 默认:原样复制 resources/exotic-keyset.json(占位公钥集,私钥已弃)到 OUT_DIR,
// crypto.rs 经 include_str!(OUT_DIR/exotic-keyset.json) 嵌入 → 行为与旧的直接
// include_str!("../resources/…") 逐位一致。
//
// 注入:构建时设 PICASA_EXOTIC_KEYSET_FILE=<绝对路径> → 以该文件为编译期信任根。
// 这就是 resources 内 _note 预告的「发布前由发布流水线替换为受控签发机公钥」的
// 替换机制,内测构建(2026-07-05)提前启用:内测 keyset 建议为「占位集 + 内测键」
// 超集,使 builtin_keyset_parses 等测试在注入态下依然成立。
//
// 安全姿态:注入只发生在**编译期**,产物二进制的信任根固定;Release 运行时没有任何
// keyset 旁路(SEC-02)。与 debug-only 的**运行时**变量 PICASA_EXOTIC_DEV_KEYSET
// (exotic/mod.rs::trusted_keyset)同名近义但互不相干,勿混淆。

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=PICASA_EXOTIC_KEYSET_FILE");
    println!("cargo:rerun-if-changed=resources/exotic-keyset.json");

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let default_path = manifest_dir.join("resources").join("exotic-keyset.json");

    let src = match env::var("PICASA_EXOTIC_KEYSET_FILE") {
        Ok(p) if !p.is_empty() => {
            let p = PathBuf::from(p);
            // fail-fast:显式注入的文件缺失属构建配置错误,绝不静默回退占位集
            // (静默回退会产出一个「以为带内测信任根、实则 fail-closed」的坏安装包)。
            assert!(
                p.is_file(),
                "PICASA_EXOTIC_KEYSET_FILE 指向的 keyset 文件不存在: {}",
                p.display()
            );
            println!("cargo:rerun-if-changed={}", p.display());
            // 构建日志留痕:注入生效与否必须可见、可审计(内测建包的 Done 证据之一)。
            println!("cargo:warning=exotic 信任根已由构建注入: {}", p.display());
            p
        }
        // dev 自取(2026-07-11 psd bad_signature 事故):无显式注入时,**debug 构建**若发现
        // 本机内测 keyset(gitignored,仅含验签公钥)则自动采用——此前 dev 构建从未被注入,
        // 信任根恒为 fail-closed 占位集,已装的内测签名插件在 dev 里永远 bad_signature,
        // 且该缺口被 07-05 彩排期的运行时旁路掩蔽、cargo clean 后暴露。Release 构建、
        // CI runner 与贡献者机(无 .internal-signing/)不受影响:仍占位集或显式注入。
        _ => {
            let dev_keyset = manifest_dir
                .parent()
                .and_then(|p| p.parent())
                .map(|root| root.join(".internal-signing").join("internal-keyset.json"));
            match dev_keyset {
                Some(p) if env::var("PROFILE").as_deref() == Ok("debug") && p.is_file() => {
                    println!("cargo:rerun-if-changed={}", p.display());
                    println!(
                        "cargo:warning=exotic 信任根(debug 自取内测集): {}",
                        p.display()
                    );
                    p
                }
                _ => default_path,
            }
        }
    };

    // 最小完整性哨兵:空文件/明显非 keyset 内容在编译期即失败,不等运行时 fail-closed。
    let content = fs::read_to_string(&src)
        .unwrap_or_else(|e| panic!("读取 keyset 失败 {}: {e}", src.display()));
    assert!(
        content.contains("\"keys\""),
        "keyset 文件缺少 \"keys\" 字段,疑似指错文件: {}",
        src.display()
    );

    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR")).join("exotic-keyset.json");
    fs::write(&out, content)
        .unwrap_or_else(|e| panic!("写入 OUT_DIR keyset 失败 {}: {e}", out.display()));
}
