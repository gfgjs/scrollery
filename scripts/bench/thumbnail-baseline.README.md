# Windows 缩略图基准采集

`thumbnail-baseline.ps1` 记录运行配置、二进制、Git 和素材清单；传入 `-ProcessId` 时低频采集指定宿主与对应图片 worker。参数示例见脚本开头。脚本不启动应用或触发缩略图生成。

## 进程范围与身份

`-ProcessId` 必须是 `-ExePath` 对应的应用宿主。当前 Windows 实现由宿主直接启动同目录的 `native-thumbnail-worker.exe`，参数包含 `--threads` 与 `--memory-bytes`，没有中间启动器。依据：`src-tauri/src/thumbnail/native_worker.rs` 的 `Settings::new` 和 `Process::spawn`。生产启动路径没有输出 worker PID 日志；`last_spawned_pid` 仅存在于测试构建。

采样通过 `Win32_Process` 查询宿主和父 PID 等于该宿主的同名 worker，再核对完整映像路径及创建时间。worker 必须位于宿主同目录，且创建时间不早于本次宿主。宿主在发现子进程后再次核对身份；退出、PID 复用或映像变化时停止，丢弃该轮记录。worker 每轮重新发现，以 PID、创建时间和映像路径区分生命周期，不保留旧 PID。其他应用实例、其他目录同名进程、孙进程均不纳入。

查询失败或候选进程身份不可读时，该轮计入 `errorCount`，不输出缺项合计；运行配置保留 `lastError`。要求当前账户能读取目标进程的映像路径和创建时间。

## 输出口径（schemaVersion 2）

- `memory-samples.csv`：每个进程每轮一行，含 `sampleIndex`、UTC 采样时间、角色、PID、父 PID、创建时间、映像路径和资源指标。生命周期峰值与累计 CPU 秒归属于该进程身份。
- `memory-totals.csv`：每轮一行，合计当次工作集、私有提交、句柄和线程，并记录 `workerCount`。通过 `sampleIndex` 关联逐进程行。
- `run-config.json`：`sampleCount` 是成功采样轮数，`processRowCount` 是进程记录数；同时保存两个 CSV 路径、进程范围、宿主初始身份和结束原因。

工作集求和可能重复计算共享页；各进程生命周期峰值不相加。合计序列的最大值只是低频采样峰值下界。`workerCount=0` 表示该轮未观察到 worker；采样间隔内已启动并结束的 worker 可能漏采。范围不含 WebView、其他 sidecar 和 GPU 显存。这里的多进程快照不是同一时刻的原子快照。

内存字段统一输出字节；`PeakWorkingSetSize`、`PeakPageFileUsage` 从 KiB 换算，CPU 时间从 100ns 换算为秒。身份和指标均来自同一条 CIM 记录，避免在枚举 PID 后用另一 API 读取已换代进程的指标。字段依据：[Microsoft Win32_Process 文档](https://learn.microsoft.com/en-us/windows/win32/cimwin32prov/win32-process)。

## 定向自检

在仓库根运行：

```powershell
node scripts/bench/thumbnail-baseline-selftest.mjs
```

自检从 PowerShell AST 加载生产函数和采样循环，使用合成记录验证过滤、身份、聚合和 CSV；另启动一个本次自有、隐藏的空闲 PowerShell 子进程核验真实 CIM 父子关系，结束时清理该进程。它不运行应用、native worker 或真实基准。合成 CSV/JSON 留在 `target/T5/selftest-<随机标识>/`。

六个具名字面量 `testCase` 使用现有测试预算收集规则，计入 `script` 池共 **6 项**；不能仅按一个启动命令计为 1 项。逐项理由见本轮 [T5 验证证据](../../docs/tasks/2026-10-11-持续改进实施/references/T5/result.md)。
