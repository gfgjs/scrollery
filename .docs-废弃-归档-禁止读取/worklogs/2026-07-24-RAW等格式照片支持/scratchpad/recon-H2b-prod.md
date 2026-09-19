---
status: active
type: working-memory
line: RAW等格式照片支持
id: recon-H2b-prod
created: 2026-07-24
---

# 摸底报告：raw-worker sidecar 打包进发布包施工面

## 1. tauri.conf.json 及所有平台变体的 bundle 节现状

**路径 + 代码片段：**

- **主配置** `src-tauri/tauri.conf.json:51-61`
  ```json
  "bundle": {
    "active": true,
    "targets": "all",
    "icon": [...]
  }
  ```
  - **现状**：无 `externalBin`、无 `resources`、无 `beforeBuildCommand`

- **perf 变体** `src-tauri/tauri.perf.conf.json:1-4`
  ```json
  {
    "$schema": "...",
    "//": "...计划追加 bundle.externalBin（ffmpeg.exe）与 bundle.resources（pdfium.dll）..."
  }
  ```
  - **现状**：空配置，仅注释说明计划（P4/P5 落地后追加）

- **direct-release 变体** `src-tauri/tauri.direct-release.conf.json:11-13`
  ```json
  "bundle": {
    "createUpdaterArtifacts": true
  }
  ```
  - **现状**：无 `externalBin`、无 `resources`

**结论**：三个配置文件的 bundle 节均无 externalBin/resources 声明，raw-worker 尚未被纳入打包策略。

---

## 2. ai-worker 先例：交付、启动、路径解析

**路径 + 代码片段：**

- **启动路径解析** `src-tauri/src/ai/worker_client.rs:599-618`
  ```rust
  pub fn ai_worker_exe() -> Result<PathBuf, String> {
      // dev/test 覆盖
      if let Ok(p) = std::env::var("PICASA_AI_WORKER_PATH") {
          if p.is_file() { return Ok(p); }
      }
      // prod 生产：current_exe 同目录
      let exe = std::env::current_exe()?;
      let dir = exe.parent()?;
      let p = dir.join(format!("ai-worker{}", std::env::consts::EXE_SUFFIX));
      if p.is_file() { Ok(p) } else { Err(...) }
  }
  ```
  
- **开发编译** `src-tauri/tauri.conf.json:6`
  ```json
  "beforeDevCommand": "cargo build -p ai-worker && npm run dev"
  ```

**结论**：ai-worker 采用"**主程序同目录加载**"策略（prod 预期 ai-worker.exe 与 Scrollery.exe 同目录）；dev 时在 beforeDevCommand 编译；但 tauri.conf.json 的 bundle 节中**无 externalBin 声明**，说明 ai-worker 在 prod 打包策略中**尚未确定**是走 externalBin 还是其他方式（可能原地编译或离线补装）。

---

## 3. resolve_worker_path：dev/prod 分支完整代码

**路径 + 代码片段：**

`src-tauri/src/exotic/installer.rs:325-365`

- **Debug（dev）分支**（行 333-354）
  ```rust
  #[cfg(debug_assertions)]
  {
      if plugin_id == PSD_PLUGIN_ID {
          if let Some(p) = std::env::var_os("EXOTIC_PSD_WORKER_PATH") {
              return Some(PathBuf::from(p));
          }
      } else if plugin_id == RAW_PLUGIN_ID {
          // dev 用专属 env
          if let Some(p) = std::env::var_os("EXOTIC_RAW_WORKER_PATH") {
              return Some(PathBuf::from(p));
          }
          // dev 未设 env：明确拒绝
          tracing::warn!(
              "{plugin_id} 是 builtin worker(RAW gnu sidecar),dev 需设 EXOTIC_RAW_WORKER_PATH \
               指向 raw-worker.exe 才能拉起;prod 打包留待 H2b-prod(tauri externalBin 跨工具链 sidecar)"
          );
          return None;
      }
  }
  ```

- **Release（prod）分支**（行 358-364）
  ```rust
  #[cfg(not(debug_assertions))]
  if plugin_id == RAW_PLUGIN_ID {
      tracing::warn!(
          "{plugin_id} 是 builtin worker,prod 打包(H2b-prod:tauri externalBin 跨工具链 sidecar)尚未就绪,拒绝拉起"
      );
      return None;
  }
  ```

**对比 PSD worker 路径**：PSD worker 采用"已安装插件"验签流程（第 367 行后的 verify_installed_integrity），raw 则因是 builtin distribution 无安装包记录，dev/prod 均走提前短路。

**结论**：raw-worker 在 prod Release 构建中**明确拒绝拉起**，日志明言"H2b-prod:tauri externalBin 跨工具链 sidecar"为施工待办。

---

## 4. raw-worker crate 及编译产物

**路径 + 代码片段：**

- **Cargo.toml** `crates/exotic-workers/raw-worker/Cargo.toml:1-21`
  ```toml
  # [[bin]]
  name = "raw-worker"
  path = "src/main.rs"
  
  # 独立 workspace（脱离父）
  [workspace]
  ```
  
- **关键约束**（行 5-9）
  ```
  🔴 rsraw 的 build.rs 在 MSVC panic，故本 crate 只能用 --target x86_64-pc-windows-gnu 编译
  🔴 不加入根 Cargo.toml 的 workspace members，用空 [workspace] 表脱离
  ```

- **编译产物**（已存在）
  ```
  crates/exotic-workers/raw-worker/target/x86_64-pc-windows-gnu/debug/raw-worker.exe
  ```

**结论**：raw-worker 必须用 `cargo build -p raw-worker --target x86_64-pc-windows-gnu` 独立编译（gnu-only），产物为 raw-worker.exe；独立 workspace 设计避免 msvc 编译错误传导到主项目。

---

## 5. CI 中 tauri build / bundle 工作流

**路径 + 代码片段：**

- **.github/workflows/ci.yml**（第 31-100 行）
  - 仅 `cargo check --workspace`（第 69 行）
  - 仅 `cargo test --workspace`（第 75 行）
  - 仅 `cargo clippy --workspace`（第 72 行）
  - **无 tauri build，无 bundle 打包**

- **.github/workflows/release.yml**（第 85-86 行，仅公开仓）
  ```yaml
  - name: tauri build(免费版,含 MSI + NSIS 安装包)
    run: npx tauri build
  ```
  - 仅在 tag 发布或 workflow_dispatch 触发
  - 执行 `npx tauri build`（默认配置，无 worker 编译前置）

**结论**：CI 只做编译检查/测试，无 tauri build；release workflow 中 tauri build 前**无 raw-worker 编译 job**，prod 打包链路暂无 raw-worker 集成。

---

## 6. worker spawn 机制与 capabilities

**路径 + 代码片段：**

- **spawn_worker_process** `src-tauri/src/exotic/worker.rs:85-92`
  ```rust
  pub fn spawn_worker_process(spec: &WorkerSpec) -> std::io::Result<Child> {
      let mut cmd = Command::new(&spec.exe_path);  // std::process::Command
      cmd.stdin(Stdio::piped())
         .stdout(Stdio::piped())
         .stderr(Stdio::piped());
      apply_low_priority(&mut cmd);
      cmd.spawn()
  }
  ```
  - **采用**：`std::process::Command`（原始 Rust 标准库），**不是** tauri-plugin-shell sidecar

- **capabilities 权限** `src-tauri/capabilities/default.json:10-11`
  ```json
  "shell:default",
  "shell:allow-open"
  ```
  - 有 shell 相关权限（第 10-11 行），但仅供 open/explorer 调用（system_commands.rs 第 78/85/92 行）

**结论**：worker 启动用 raw `std::process::Command`，非 tauri sidecar，因此**无需额外 capabilities 改动**；worker exe 必须在系统路径或主程序附近（可直接定位文件路径）。

---

## 7. 构建脚本中 worker 相关编译

**路径 + 代码片段：**

- **package.json scripts** `package.json:17-19`
  ```json
  "tauri:build:lite": "tauri build",
  "tauri:build:perf": "tauri build --features perf -c src-tauri/tauri.perf.conf.json",
  "tauri:build:direct-release": "tauri build -c src-tauri/tauri.direct-release.conf.json"
  ```
  - 三个 tauri build 脚本，**无 raw-worker 或 ai-worker 编译前置**

- **build.rs** `src-tauri/build.rs:1-3`
  ```rust
  fn main() {
      tauri_build::build()
  }
  ```
  - 仅调 `tauri_build::build()`，无自定义逻辑

- **tauri.conf.json** `src-tauri/tauri.conf.json:6,9`
  ```
  beforeDevCommand: "cargo build -p ai-worker && npm run dev"
  beforeBuildCommand: "npm run build"
  ```
  - 只有 ai-worker 在 beforeDevCommand 编译（dev only）
  - beforeBuildCommand 仅前端 vite build

**结论**：构建脚本中**无 raw-worker 编译命令**，prod release 打包时既无前置编译脚本，tauri.conf.json bundle 节也无 externalBin 声明，raw-worker 集成尚属施工空白。

---

## 汇总

| 层级 | 现状 | 缺口 |
|------|------|------|
| **tauri.conf.json** | bundle 节无 externalBin | 需添加 `externalBin: [...]` 指向 raw-worker.exe |
| **ai-worker** | main 同目录策略 + beforeDevCommand 编译 | bundle 打包策略未见（可能原地编译或设计遗漏） |
| **resolve_worker_path** | dev env 加载 + prod warn+None | prod 需落地 H2b-prod 打包策略 |
| **raw-worker crate** | gnu-only 独立编译，产物已存在 | 缺"编译后复制进 tauri bundle" 流程 |
| **CI 与 scripts** | release.yml 无 raw-worker 编译 job | 需在 tauri build 前加 cargo build gnu target 编译 |
| **spawn 机制** | std::process::Command + 同目录查找 | 路径解析策略可参考 ai_worker_exe（当前 raw 拒绝） |

