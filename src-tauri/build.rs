//! 从 `src/ipc/registry.rs` 生成全部自定义命令的 ACL 权限，交由 tauri-build 校验与打包。
//!
//! 注册表是命令清单的唯一来源；capabilities 引用生成的 `allow-app-commands` 权限。
//! 无法解析条目或清单为空时立即构建失败，避免遗漏命令后才在运行时遭 ACL 拒绝。

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
/// 自定义 permissions_path_pattern 会替代默认目录，因此手写权限也复制到同一输出目录。
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

    // AppManifest 要求静态字符串；内存在构建脚本退出时释放，glob 路径统一使用正斜杠。
    let pattern: &'static str =
        Box::leak(format!("{}/*.toml", to_forward_slashes(&permission_dir)).into_boxed_str());
    let mut attributes = tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new()
            .commands(&[
                "copy_directory",
                "cancel_directory_copy",
                "cancel_viewport_thumbnail_request",
                "get_feature_activation",
                "activate_feature",
                "restore_discard",
                "resolve_document_link",
            ])
            .permissions_path_pattern(pattern),
    );
    // MSVC 仅由 /MANIFESTINPUT 提供清单，避免重复 RT_MANIFEST 资源引发 CVT1100/LNK1123。
    if is_windows_msvc() {
        attributes = attributes
            .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest());
    }
    tauri_build::try_build(attributes).expect("tauri-build 失败");

    link_common_controls_manifest();
}

/// CommonControls v6 依赖清单,内容与 tauri-build 2.6.3 的 windows-app-manifest.xml 相同。
///
/// Windows/MSVC 下全部链接目标共用此清单，tauri-build 的默认清单在 main 中关闭。
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
/// GUI 与库单测都依赖 comctl32 v6 的 TaskDialogIndirect；缺少清单会在启动时找不到导出。
/// rustc-link-arg 覆盖库单测及 bin，rustc-link-arg-tests 仅覆盖独立 [[test]] 目标。
/// 清单嵌入可执行文件，避免依赖测试运行目录中的旁置文件。
fn link_common_controls_manifest() {
    if !is_windows_msvc() {
        // GNU 工具链不吃 link.exe 参数:退回默认行为,由该工具链自行处理(其默认清单也未关闭)。
        return;
    }
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("缺 OUT_DIR"));
    let manifest = out_dir.join("scrollery-common-controls.manifest");
    write_if_changed(&manifest, COMMON_CONTROLS_MANIFEST_XML);
    // link.exe 要求 /MANIFESTINPUT 与 /MANIFEST:EMBED 同时使用。
    println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
}

/// 目标是否为 Windows/MSVC —— 清单与 link.exe 参数只在这时才有意义。
///
/// 关闭默认清单与提供 /MANIFESTINPUT 使用同一判定，避免缺失或重复清单。
fn is_windows_msvc() -> bool {
    env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
}

fn to_forward_slashes(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// 把源码树 permissions/ 下的手写权限文件复制到生成目录(见 SOURCE_PERMISSIONS_REL_DIR 说明)。
///
/// 仅复制 .toml；与生成文件同名时拒绝覆盖，避免权限丢失。
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
    // 注册列表没有字符串字面量；先剥除行注释，避免注释中的括号干扰列表边界。
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

    // 配对方括号，找到宏参数列表的收尾。
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

/// 生成包含全部注册命令的单条权限，供 capabilities 引用。
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
