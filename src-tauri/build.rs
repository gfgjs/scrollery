//! 构建脚本:把 app 自定义命令编译成 ACL 权限,再交给 tauri-build 生成/校验,最后随包分发。
//!
//! why 需要这层(2026-09-12,源码核实):AGENTS.md 硬约束要求「Tauri 暴露命令在 `src-tauri/capabilities/`
//! 声明权限」。而 tauri 2.11.4 的运行时门是(tauri-2.11.4/src/webview/mod.rs):
//!
//! ```text
//! if (plugin_command.is_some() || has_app_acl_manifest || !is_local) && invoke.acl.is_none() { reject }
//! ```
//!
//! 即:本地(self)来源的 app 自有命令,在没有 app 清单时**全部放行**(`has_app_acl_manifest=false`)。
//! 这解释了为什么此前 capabilities 只写插件权限也能跑——不是「core:default 覆盖了自定义命令」,
//! 而是这道门根本没查。反过来说:一旦清单存在(只为几个新命令手写清单就会如此),未列入清单的
//! 其余数百命令会被立刻拒绝。所以清单必须覆盖 registry.rs 里的全部命令,又不能靠手工维护第二份表。
//!
//! 做法:构建期解析 `src/ipc/registry.rs`(命令注册唯一真源)的 `generate_handler![...]` 条目,在 OUT_DIR
//! 生成一份权限(单条 permission,`commands.allow` = 全部命令),把它作为 tauri-build 的 app 清单来源
//! (`AppManifest::permissions_path_pattern`)。capabilities 只需引用那一个权限标识符;命令增删只改
//! registry.rs,本脚本自动跟随(靠 `rerun-if-changed` 触发)。
//!
//! 解析刻意「读不懂就报错」:handler 列表里出现非注释、非属性、非 `模块::...::命令名,` 形态的行,或
//! 一条命令都解析不出,立即构建失败——否则某天重构可能把命令悄悄漏出清单,而在运行时表现为
//! 「IPC 被 ACL 拒绝」这种离源头很远的故障。

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// 命令注册清单(单一真源)。
const REGISTRY_REL_PATH: &str = "src/ipc/registry.rs";
/// 生成的权限标识符;capabilities 里引用的就是它。
const APP_COMMAND_PERMISSION: &str = "allow-app-commands";
/// 权限文件名(置于 OUT_DIR,不进源码树)。
const PERMISSION_FILE_NAME: &str = "app-commands.toml";

fn main() {
    // registry.rs 一变就重跑本脚本,保证清单与命令注册同步。
    println!("cargo:rerun-if-changed={REGISTRY_REL_PATH}");

    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("缺 CARGO_MANIFEST_DIR"));
    let registry_path = manifest_dir.join(REGISTRY_REL_PATH);
    let registry = fs::read_to_string(&registry_path)
        .unwrap_or_else(|e| panic!("读取 {} 失败: {e}", registry_path.display()));
    let commands = parse_registered_commands(&registry);

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("缺 OUT_DIR"));
    let permission_dir = out_dir.join("app-permissions");
    fs::create_dir_all(&permission_dir).expect("创建权限输出目录失败");
    let permission_file = permission_dir.join(PERMISSION_FILE_NAME);
    write_if_changed(&permission_file, &render_permission(&commands));

    // AppManifest 只收 &'static str,故把绝对路径 glob 泄漏成静态串(build.rs 进程结束即释放)。
    // 用正斜杠:tauri-build 走 glob 0.3(已确认其把盘符前缀按 Prefix 分量处理),正斜杠跨平台稳。
    let pattern: &'static str =
        Box::leak(format!("{}/*.toml", to_forward_slashes(&permission_dir)).into_boxed_str());
    let attributes = tauri_build::Attributes::new()
        .app_manifest(tauri_build::AppManifest::new().permissions_path_pattern(pattern));
    tauri_build::try_build(attributes).expect("tauri-build 失败");
}

fn to_forward_slashes(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// 从 `generate_handler![ ... ]` 块取出命令名(路径末段),保持注册顺序并去重。
fn parse_registered_commands(source: &str) -> Vec<String> {
    const MACRO_OPEN: &str = "generate_handler![";
    // 先逐行剥注释:列表里没有字符串字面量,`//` 之后必是注释。这一步让后续配对扫描不被
    // 「注释里出现 `]`」骗到——那会把列表提前截断,表现为命令静默漏出清单(构建仍然成功,
    // 运行时才以 ACL 拒绝暴露)。
    let cleaned: String = source
        .lines()
        .map(|line| line.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n");
    let source = cleaned.as_str();

    let open = source
        .find(MACRO_OPEN)
        .unwrap_or_else(|| panic!("{REGISTRY_REL_PATH} 里找不到 `{MACRO_OPEN}`"));
    let after_open = &source[open + MACRO_OPEN.len()..];

    // 配对扫描到宏参数列表的收尾 ']':列表里只有条目与注释,不存在嵌套方括号。
    let mut depth = 1usize;
    let mut end = None;
    for (idx, ch) in after_open.char_indices() {
        match ch {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    end = Some(idx);
                    break;
                }
            }
            _ => {}
        }
    }
    let body = &after_open[..end.unwrap_or_else(|| panic!("{MACRO_OPEN} 未闭合"))];

    let mut commands: Vec<String> = Vec::new();
    for raw_line in body.lines() {
        let code = raw_line.trim();
        if code.is_empty() || code.starts_with('#') {
            // 空行 / 属性行(属性属于紧随其后的那一条目)。
            continue;
        }
        let item = code.strip_suffix(',').unwrap_or_else(|| {
            panic!("{REGISTRY_REL_PATH}:handler 列表出现非条目行 `{code}`(条目须以 `,` 结尾)")
        });
        let name = item.rsplit("::").next().unwrap_or("").trim();
        if !is_command_ident(name) {
            panic!("{REGISTRY_REL_PATH}:无法从 `{item}` 解析命令名(期望 `模块::…::命令名` 形态)");
        }
        if commands.iter().any(|known| known == name) {
            println!("cargo:warning={REGISTRY_REL_PATH}:命令 `{name}` 重复注册,已去重");
            continue;
        }
        commands.push(name.to_string());
    }
    assert!(!commands.is_empty(), "{REGISTRY_REL_PATH}:未解析出任何命令");
    commands
}

fn is_command_ident(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// 渲染权限文件。单条 permission 承载全部命令:粒度与现状(本地窗口可用全部 app 命令)一致,
/// 也不需要在 capabilities 里逐个登记;日后若要按窗口收窄,再拆成多条即可。
fn render_permission(commands: &[String]) -> String {
    let mut toml = String::from(concat!(
        "# 由 src-tauri/build.rs 依据 src/ipc/registry.rs 生成,请勿手改。\n",
        "# capabilities 引用的权限名即下方 identifier;命令增删只改 registry.rs。\n\n",
        "[[permission]]\n",
    ));
    toml.push_str(&format!("identifier = \"{APP_COMMAND_PERMISSION}\"\n"));
    toml.push_str("description = \"经 registry.rs 注册的全部 app 自有命令(构建期生成)。\"\n");
    toml.push_str("commands.allow = [\n");
    for command in commands {
        toml.push_str(&format!("  \"{command}\",\n"));
    }
    toml.push_str("]\n");
    toml
}

fn write_if_changed(path: &Path, contents: &str) {
    if fs::read_to_string(path)
        .map(|old| old == contents)
        .unwrap_or(false)
    {
        return;
    }
    fs::write(path, contents).unwrap_or_else(|e| panic!("写 {} 失败: {e}", path.display()));
}
