import pluginVue from 'eslint-plugin-vue'
import { defineConfigWithVueTs, vueTsConfigs } from '@vue/eslint-config-typescript'
import skipFormatting from '@vue/eslint-config-prettier/skip-formatting'

// Vue 3 + TypeScript strict 官方推荐 flat config（create-vue 同款）。
// - flat/essential：Vue 模板/SFC 必备规则。
// - vueTsConfigs.recommended：typescript-eslint 推荐集（非类型感知，不依赖 tsconfig 类型信息）。
// - skipFormatting：关闭与 Prettier 冲突的格式类规则，把「格式」交给 Prettier，
//   ESLint 只管代码质量（避免二者打架）。
export default defineConfigWithVueTs(
  {
    name: 'app/files-to-lint',
    files: ['**/*.{ts,mts,tsx,vue}'],
  },
  {
    // Rust（src-tauri）、Python 虚拟环境（venv，含 torch/onnxmltools 打包 JS）、
    // 构建产物、压缩/生成文件不参与前端 lint —— 否则 ESLint 默认会扫到这些 .js/.mjs 噪声。
    name: 'app/files-to-ignore',
    ignores: [
      '**/dist/**',
      '**/dist-ssr/**',
      '**/coverage/**',
      'src-tauri/**',
      // vendored 第三方源码(foliate-js,钉定 commit;见 src/vendor/foliate-js/VENDOR.md):
      // 不参与本项目 lint —— 其代码风格(私有 class 字段 `#x`、生成器等)非本仓约定,
      // 且刻意保持与上游零 diff 以便升级,格式化/规则整改都会破坏这一点。
      'src/vendor/**',
      // workspace 化后 Rust target/ 在仓库根(tauri codegen 产出 .js/.mjs 资产,非前端源码)。
      'target/**',
      // 本地 git worktree 副本及外部代理脚本,非项目源码。
      '.claude/**',
      'crates/**',
      'venv/**',
      '**/site-packages/**',
      '**/*.min.js',
    ],
  },
  pluginVue.configs['flat/essential'],
  vueTsConfigs.recommended,
  {
    // R1-7 i18n 防回潮：模板内禁止裸文本 —— 一切用户可见文案必须走 t()/$t()。
    // 规则只查模板文本节点与 title/aria-label/placeholder/alt 等展示型属性；
    // allowlist 准入标准：纯符号 / 计量单位 / 品牌与协议名 / 技术徽标 / 技术占位示例 /
    // 语言自名（endonym，语言选项按惯例以其自身语言显示）——勿放行任何自然语言句子。
    name: 'app/i18n-no-bare-strings',
    files: ['src/**/*.vue'],
    rules: {
      'vue/no-bare-strings-in-template': [
        'error',
        {
          allowlist: [
            // 符号与标点
            '(', ')', ',', '.', '&', '+', '-', '=', '*', '/', '#', '%', '!', '?', ':',
            '[', ']', '{', '}', '<', '>', '·', '•', '‐', '–', '—',
            '−', '|', '×', '…', '‹', '›', '→', '↑', '↓', '✗',
            // 计量单位（紧跟插值的后缀片段）
            'px', 's', 'MB', 'GB', 'em',
            // 品牌 / 协议 / 技术徽标
            'Scrollery', 'v0.1.0', 'WebDAV', 'LIVE', 'Live', 'ORIG', 'THUMB', 'RAW', 'fp16',
            '1:1', 'API Key',
            // 图片简单编辑(方案 C):裁剪比例/输出格式,技术标识非自然语言
            '4:3', '3:4', '16:9', '9:16', 'JPEG', 'PNG',
            // 技术占位示例（placeholder 中的 URL / 模型名 / 密钥格式）
            'https://api.openai.com/v1', 'https://dav.example.com/remote.php/dav/files/me',
            'gpt-4o-mini', 'sk-...', 'photos',
            // 语言自名（endonym）
            '简体中文', 'English',
          ],
        },
      ],
    },
  },
  {
    // 审查 P1-17 防回潮：除 utils/ipc.ts(唯一 wrapper)外,禁止从 @tauri-apps/api/core 裸导入
    // `invoke` —— 一切 IPC 调用走 invokeIpc(常量强制 + 结构化 IpcError.code 分流),消除
    // 「raw invoke 错误退化为 (e as Error).message」的双轨。convertFileSrc/Channel 仍从 core 导入。
    name: 'app/no-raw-invoke',
    files: ['src/**/*.{ts,vue}'],
    ignores: ['src/utils/ipc.ts'],
    rules: {
      'no-restricted-imports': [
        'error',
        {
          paths: [
            {
              name: '@tauri-apps/api/core',
              importNames: ['invoke'],
              message:
                '请用 invokeIpc(from utils/ipc)而非裸 invoke——统一结构化错误码分流(审查 P1-17)。convertFileSrc/Channel 仍可从 core 导入。',
            },
          ],
        },
      ],
    },
  },
  {
    // 日志能力重构 S3(方案 §4/§9.4)防回潮:裸 console.* 不落盘、agent/用户查不到,一律走
    // utils/logger.ts(汇入后端 tracing,同一套 JSONL/UI/压缩治理)。三处例外真是"给浏览器
    // devtools 看"而非应用日志:logger.ts 自身(桥接点)、useGalleryPerfProbe.ts(dev 门控性能
    // 探针,localStorage 开关,产物是给开发者当场读的 console.table 风格输出)、harness/ipcFixtures.ts
    // (仅 UI harness dev 场景运行,不进生产构建的真实用户路径)。
    name: 'app/no-console',
    files: ['src/**/*.{ts,vue}'],
    ignores: [
      'src/utils/logger.ts',
      'src/composables/useGalleryPerfProbe.ts',
      'src/harness/ipcFixtures.ts',
    ],
    rules: {
      'no-console': 'error',
    },
  },
  {
    // 标准约定：以 `_` 开头的未用变量/参数/解构/catch 视为「有意保留」（接口要求但用不到
    // 的形参、占位解构等），不报 no-unused-vars —— 避免为消警而强删签名必需的参数。
    name: 'app/unused-underscore',
    rules: {
      '@typescript-eslint/no-unused-vars': [
        'error',
        {
          argsIgnorePattern: '^_',
          varsIgnorePattern: '^_',
          caughtErrorsIgnorePattern: '^_',
          destructuredArrayIgnorePattern: '^_',
        },
      ],
    },
  },
  skipFormatting,
)
