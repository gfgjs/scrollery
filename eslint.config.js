import pluginVue from 'eslint-plugin-vue'
import { defineConfigWithVueTs, vueTsConfigs } from '@vue/eslint-config-typescript'
import skipFormatting from '@vue/eslint-config-prettier/skip-formatting'

// Vue 与 TypeScript 使用推荐规则；关闭格式类规则以避免与 Prettier 冲突。
export default defineConfigWithVueTs(
  {
    name: 'app/files-to-lint',
    files: ['**/*.{ts,mts,tsx,vue}'],
  },
  {
    // 仅检查第一方前端源码，排除后端、依赖与生成文件。
    name: 'app/files-to-ignore',
    ignores: [
      '**/dist/**',
      '**/dist-ssr/**',
      '**/coverage/**',
      'src-tauri/**',
      // 第三方源码保持上游格式，以便升级时对照。
      'src/vendor/**',
      // Rust 构建目录也可能包含生成的 JS 资产。
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
    // 模板中的用户可见文案使用 t()/$t()。
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
            // 裁剪比例与输出格式
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
    // IPC 统一经 invokeIpc 校验命令并处理结构化错误码；仅该封装可导入 invoke。
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
    // 应用日志经 logger.ts 汇入后端 tracing 以落盘。
    // 桥接封装、开发性能探针与 UI harness 保留浏览器控制台输出。
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
    // `_` 前缀表示有意保留的未用值，如接口要求的形参。
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
