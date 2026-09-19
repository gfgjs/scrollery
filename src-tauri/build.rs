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
/// 手写权限文件所在目录(相对 CARGO_MANIFEST_DIR)。
///
/// 为什么要搬进 OUT_DIR:tauri-build 在设置了 permissions_path_pattern 时**只**读那一个 glob
/// (见 tauri-build src/acl.rs::app_manifest_permissions),默认的 permissions/** 分支被跳过。
/// 本仓的 pattern 指向 OUT_DIR 的生成文件,故源码树里的 permissions/ 会被静默忽略——一份「看起来
/// 生效、实际没进 ACL」的权限文件比没有更危险。这里把源码树的手写权限文件原样复制到生成目录,
/// 使两条来源共用同一个 glob,也仍然只有一个 pattern。
const SOURCE_PERMISSIONS_REL_DIR: &str = "permissions";

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
    copy_source_permissions(&manifest_dir, &permission_dir);

    // AppManifest 只收 &'static str,故把绝对路径 glob 泄漏成静态串(build.rs 进程结束即释放)。
    // 用正斜杠:tauri-build 走 glob 0.3(已确认其把盘符前缀按 Prefix 分量处理),正斜杠跨平台稳。
    let pattern: &'static str =
        Box::leak(format!("{}/*.toml", to_forward_slashes(&permission_dir)).into_boxed_str());
    let mut attributes = tauri_build::Attributes::new()
        .app_manifest(tauri_build::AppManifest::new().permissions_path_pattern(pattern));
    // Windows/MSVC 关掉 tauri-build 的默认应用清单(它经 resource.rc 的 `1 24` 编进 resource.lib),
    // 让本脚本的 /MANIFESTINPUT 成为**唯一**清单来源。原因见 link_common_controls_manifest:
    // 同一个 RT_MANIFEST 资源有两个来源时 link.exe 不去重,直接 CVT1100 + LNK1123。其余平台保持
    // 默认清单行为(非 Windows 无 winres 资源,Windows-GNU 不走 link.exe)。
    if is_windows_msvc() {
        attributes = attributes
            .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest());
    }
    tauri_build::try_build(attributes).expect("tauri-build 失败");

    link_common_controls_manifest();
}

/// CommonControls v6 依赖清单,内容与 tauri-build 2.6.3 的 windows-app-manifest.xml 相同。
///
/// 这是 Windows/MSVC 下本 crate 全部目标(UI bin/cdylib 与 lib 单测)唯一的清单来源:tauri-build 的默认清单已按
/// 目标平台关闭(见 main 里的 new_without_app_manifest),故此处内容必须自带 bin 目标需要的一切
/// ——也就是 CommonControls v6 依赖本身。
const COMMON_CONTROLS_MANIFEST_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <dependency>
    <dependentAssembly>
      <assemblyIdentity
        type="win32"
        name="Microsoft.Windows.Common-Controls"
        version="6.0.0.0"
        processorArchitecture="*"
        publicKeyToken="6595b64144ccf1df"
        language="*" />
    </dependentAssembly>
  </dependency>
</assembly>
"#;

/// 给本 crate 的 Windows/MSVC 链接步骤提供 CommonControls v6 清单(全目标唯一清单来源)。
///
/// 为什么要这一步:测试 harness 与 GUI 进程共用同一份依赖树,链接进了 tauri 的对话框实现,而
/// TaskDialogIndirect 只存在于 comctl32 v6。库单测的可执行文件没有清单时,加载器按默认解析到
/// System32 的 v5 comctl32,该导出不存在,进程启动即 0xC0000139 STATUS_ENTRYPOINT_NOT_FOUND ——
/// 测试一条都跑不了,表现为「cargo test 直接崩」而不是断言失败。
///
/// 为什么用 rustc-link-arg(而不是 rustc-link-arg-tests):库单测属于 lib 目标,不是 Cargo 的
/// [[test]] 目标(本包也没有该目标,发 -tests 形式会被 Cargo 直接拒绝),只有不带后缀的形式能覆盖
/// 到它。它同样作用于本 crate 的 bin/cdylib,其中 bin 还会链接 tauri-build 的默认
/// 清单(winres 资源 resource.lib 里的 `1 24` = RT_MANIFEST id 1)。link.exe 不会因为两份清单
/// 内容相同就去重,而是直接报「CVT1100 duplicate resource type MANIFEST name 1 language
/// 0x0409」并连锁 LNK1123(2026-09-16 开发启动日志);所以这里不能靠「相同即去重」,
/// 必须把来源收敛成一个:main 里用 WindowsAttributes::new_without_app_manifest 关掉 tauri-build
/// 那份,本函数的 /MANIFESTINPUT 成为唯一来源。
///
/// 用 /MANIFESTINPUT 而不是外置 .manifest 文件或旁置系统 DLL:后者都要在可执行文件同目录额外放
/// 东西(测试 exe 会换目录,旁置 DLL 也容易被误当系统库干扰),只有嵌进可执行文件是自洽的。
fn link_common_controls_manifest() {
    if !is_windows_msvc() {
        // GNU 工具链不吃 link.exe 参数:退回默认行为,由该工具链自行处理(其默认清单也未关闭)。
        return;
    }
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("缺 OUT_DIR"));
    let manifest = out_dir.join("scrollery-common-controls.manifest");
    write_if_changed(&manifest, COMMON_CONTROLS_MANIFEST_XML);
    // /MANIFESTINPUT 必须与 /MANIFEST:EMBED 同时给出(link.exe 的硬要求:前者只是「并入哪些清单」,
    // 后者才是「把结果嵌进可执行文件」)。
    println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
}

/// 目标是否为 Windows/MSVC —— 清单与 link.exe 参数只在这时才有意义。
///
/// 用于两处同一判定:(1) 是否关闭 tauri-build 的默认应用清单,(2) 是否发 /MANIFESTINPUT。
/// 判定必须同源,否则会出现「关了一份却没人补上」或「两份都在」的中间态。
fn is_windows_msvc() -> bool {
    env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
}

fn to_forward_slashes(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// 把源码树 permissions/ 下的手写权限文件复制到生成目录(见 SOURCE_PERMISSIONS_REL_DIR 说明)。
///
/// 只认 .toml,且**拒绝覆盖** build.rs 自己生成的文件:同名冲突是笔误,应当立刻失败,而不是让其中
/// 一份静默消失(权限文件静默丢失会表现为运行期「命令被 ACL 拒绝」,离源头很远)。
fn copy_source_permissions(manifest_dir: &Path, permission_dir: &Path) {
    let source_dir = manifest_dir.join(SOURCE_PERMISSIONS_REL_DIR);
    if !source_dir.exists() {
        return;
    }
    println!("cargo:rerun-if-changed={SOURCE_PERMISSIONS_REL_DIR}");

    let entries = fs::read_dir(&source_dir)
        .unwrap_or_else(|e| panic!("读取 {} 失败: {e}", source_dir.display()));
    for entry in entries {
        let path = entry.expect("读取权限目录条目失败").path();
        if path.extension().and_then(std::ffi::OsStr::to_str) != Some("toml") {
            continue;
        }
        let name = path
            .file_name()
            .expect("权限文件名")
            .to_string_lossy()
            .to_string();
        assert!(
            name != PERMISSION_FILE_NAME,
            "{} 与 build.rs 生成的文件同名,请改名",
            path.display()
        );
        println!("cargo:rerun-if-changed={}", path.display());
        let contents = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("读取 {} 失败: {e}", path.display()));
        write_if_changed(&permission_dir.join(&name), &contents);
    }
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
