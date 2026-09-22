<template>
  <div class="settings-view">
    <!-- 外置配置文件解析错误横幅(批次B):config-file-error 有值时显示,变更成功事件到达后清除
         (useConfigFile 内的 config-file-changed 处理器会清空 lastError)。置顶,不受滚动/搜索影响。 -->
    <div v-if="configFile.lastError.value" class="settings-config-banner">
      <AlertTriangle :size="16" />
      <span>
        {{ $t('settings.configFileParseError', { message: configFile.lastError.value.message }) }}
        <template v-if="configFile.lastError.value.line != null">{{
          $t('settings.configFileErrorLine', { line: configFile.lastError.value.line })
        }}</template>
      </span>
    </div>

    <!-- 外置配置文件「需重启生效」提示条(批次B深审 #7):外部编辑触及了 restart_required 键
         时展示,信息级(非 alert)、可关闭——与上方错误横幅同结构，颜色改走 --color-info。 -->
    <div
      v-if="configFile.restartRequiredKeys.value.length"
      class="settings-config-banner settings-config-banner--info"

    >
      <Info :size="16" class="settings-config-banner__info-icon" />
      <span>{{
        $t('settings.configRestartRequiredKeys', {
          keys: configFile.restartRequiredKeys.value.join('、'),
        })
      }}</span>
      <button
        type="button"
        class="settings-config-banner__dismiss"

        @click="configFile.dismissRestartRequiredNotice()"
      >
        <X :size="14" />
      </button>
    </div>

    <header class="settings-header">
      <div class="settings-header__leading">
        <button
          type="button"
          class="btn-back"
          :title="$t('settings.backToApp')"

          @click="closeSettings"
        >
          <ArrowLeft :size="18" />
        </button>
        <div class="settings-header__title-block">
          <h1 class="settings-title">{{ $t('settings.title') }}</h1>
        </div>
      </div>
      <div class="settings-search-wrap">
        <label class="settings-search">
          <Search :size="16" />
          <input
            v-model.trim="settingsQuery"
            type="search"
            :placeholder="$t('settings.searchPlaceholder')"
            :aria-label="$t('settings.searchPlaceholder')"
            @keydown.esc="settingsQuery = ''"
            @keydown.enter.prevent="searchResults[0] && selectSearchResult(searchResults[0])"
          />
        </label>
        <div
          v-if="settingsQuery.trim()"
          class="settings-search-results"
        >
          <button
            v-for="result in searchResults"
            :key="result.id"
            type="button"
            class="settings-search-result"

            @click="selectSearchResult(result)"
          >
            <span class="settings-search-result__label">{{ result.label }}</span>
            <span class="settings-search-result__meta">
              {{ $t(result.sectionLabelKey) }}
              <template v-if="result.groupLabel"> · {{ result.groupLabel }}</template>
            </span>
          </button>
          <div v-if="!searchResults.length" class="settings-search-results__empty">
            {{ $t('settings.noSearchResults') }}
          </div>
        </div>
      </div>
      <div class="settings-header__actions">
        <!-- 详细首次使用引导手册入口:与首启向导独立,随时可重开(强开,不受 guideSeen 影响)。 -->
        <button class="btn-guide" :title="$t('settings.userGuide')" @click="ui.openUserGuide()">
          <GraduationCap :size="15" />
          <span>{{ $t('settings.userGuide') }}</span>
        </button>
      </div>
    </header>

    <div class="settings-layout">
      <nav class="settings-nav">
        <button
          v-for="section in settingsSections"
          :key="section.id"
          type="button"
          class="settings-nav__item"
          :class="{ active: currentSection === section.id }"
          :aria-current="currentSection === section.id ? 'page' : undefined"

          @click="selectSection(section.id)"
        >
          {{ $t(section.labelKey) }}
        </button>
      </nav>

      <main id="settings-main" class="settings-content">
        <section v-for="section in generalSections" :key="section.id"
          v-show="currentSection === section.id" :id="`settings-${section.id}`" class="settings-section">
          <div class="settings-section__heading"><h2>{{ $t(section.labelKey) }}</h2></div>
          <CollapsibleCard v-for="group in groupsFor(section.id)" :id="group.id" :key="group.id"
            :title="$t(group.titleKey)" :default-open="group.defaultOpen"
            :summary="group.id === 'galleryTimeline' ? timelineSummary : undefined">
            <template v-for="key in group.keys" :key="key">
              <template v-if="key === 'theme'">
                <SettingRow setting-key="theme" no-control>
                  <template #extra><ThemeSettings /></template>
                </SettingRow>
              </template>
                <SettingRow v-else-if="key === 'viewerIccManager'" setting-key="viewerIccManager">
                  <template #extra>
                    <details class="settings-inline-details" :open="config.viewerColorTarget === 'custom'">
                      <summary>{{ $t('settings.viewerIccProfiles', { count: iccProfiles.length }) }}</summary>
                      <div class="icc-profile-list">
                      <div v-if="!iccProfiles.length" class="settings-card__desc">
                        {{ $t('settings.viewerIccEmpty') }}
                      </div>
                      <label v-for="p in iccProfiles" :key="p.id" class="icc-profile-item">
                        <input
                          type="radio"
                          name="viewerIccProfile"
                          :checked="
                            config.viewerColorTarget === 'custom' &&
                            config.viewerColorCustomId === p.id
                          "
                          @change="selectIccProfile(p.id)"
                        />
                        <span class="icc-profile-name" :title="p.name">{{ p.name }}</span>
                        <span class="icc-profile-size">{{ formatFileSize(p.fileSizeBytes) }}</span>
                        <button
                          type="button"
                          class="icc-profile-delete"

                          :title="$t('settings.viewerIccDeleteBtn')"
                          @click="deleteIccProfile(p.id)"
                        >
                          <Trash2 :size="14" />
                        </button>
                      </label>
                      </div>
                    </details>
                  </template>
                  <UiButton @click="importIccProfile">
                    {{ $t('settings.viewerIccImportBtn') }}
                  </UiButton>
                </SettingRow>
              <SettingRow v-else :setting-key="key" />
            </template>
          </CollapsibleCard>
          <ReaderSettingsSection v-if="section.id === 'gallery'" />
        </section>

        <section v-show="currentSection === 'media'" id="settings-media" class="settings-section">
          <div class="settings-section__heading">
            <div>
              <h2>{{ $t('settings.sectionMedia') }}</h2>
            </div>
          </div>
          <!-- ── 缩略图 ───────────────────────────────────────── -->
          <CollapsibleCard v-for="group in thumbnailGroups" :id="group.id" :key="group.id" :title="$t(group.titleKey)">
            <template v-for="key in group.keys" :key="key">
              <!-- 特例:悬停信息开关下挂信息元素多选面板 -->
              <SettingRow v-if="key === 'showThumbInfo'" setting-key="showThumbInfo">
                <DynamicSettingControl setting-key="showThumbInfo" />
                <template #extra>
                  <div v-if="ui.showThumbInfo" class="thumb-info-options">
                    <label
                      v-for="el in THUMB_INFO_ELEMENTS"
                      :key="el.value"
                      class="thumb-info-option"
                    >
                      <input
                        type="checkbox"
                        :checked="ui.thumbInfoElements.includes(el.value)"
                        @click="handleThumbInfoToggle($event, el.value)"
                      />{{ $t(el.labelKey) }}
                    </label>
                  </div>
                </template>
              </SettingRow>
              <!-- 特例:缓存目录(可点路径描述 + 换目录按钮) -->
              <SettingRow v-else-if="key === 'thumbCacheDir'" setting-key="thumbCacheDir">
                <template #desc>
                  <div
                    class="settings-card__desc clickable-path"
                    @click="openDirectory(thumbCacheDir)"
                    :title="$t('settings.openInExplorer')"
                  >
                    {{ thumbCacheDir || $t('settings.fetchingPath') }}
                  </div>
                </template>
                <UiButton @click="changeCacheDir">
                  {{ $t('settings.changeDir') }}
                </UiButton>
              </SettingRow>
              <!-- 特例:缓存占用统计(总量摘要 + 分类目明细;进设置页拉一次,按钮手动刷新) -->
              <SettingRow v-else-if="key === 'cacheStats'" setting-key="cacheStats">
                <template #desc>
                  <div class="settings-card__desc">
                    <template v-if="cacheStats">{{
                      $t('settings.cacheStatsSummary', {
                        size: formatFileSize(cacheStats.total.bytes),
                        files: cacheStats.total.files,
                        limit: formatFileSize(cacheStats.limitMb * 1024 * 1024),
                      })
                    }}</template>
                    <template v-else-if="cacheStatsLoading">{{
                      $t('settings.cacheStatsLoading')
                    }}</template>
                    <template v-else>{{ $t('settings.cacheStatsEmpty') }}</template>
                  </div>
                </template>
                <template #extra>
                  <details v-if="cacheStats" class="settings-inline-details">
                    <summary>{{ $t('settings.cacheBreakdown') }}</summary>
                    <div class="cache-stats-grid">
                    <div v-for="row in cacheStatRows" :key="row.labelKey" class="cache-stats-item">
                      <span class="cache-stats-label">{{ $t(row.labelKey) }}</span>
                      <span class="cache-stats-value"
                        >{{ formatFileSize(row.stat.bytes) }} ·
                        {{ $t('settings.cacheStatsFiles', { n: row.stat.files }) }}</span
                      >
                    </div>
                    </div>
                  </details>
                </template>
                <UiButton :disabled="cacheStatsLoading" @click="refreshCacheStats">
                  {{ $t('settings.cacheStatsRefresh') }}
                </UiButton>
              </SettingRow>
              <!-- 特例:全量缩略图生成(进度条 + 启停按钮) -->
              <SettingRow v-else-if="key === 'fullThumbGen'" setting-key="fullThumbGen">
                <template #extra>
                  <div v-if="scan.thumbGenProgress.status !== 'idle'" class="thumb-gen-status">
                    <div class="progress-bar">
                      <div
                        class="progress-bar__fill"
                        :class="{ 'progress-shimmer': scan.thumbGenProgress.isRunning }"
                        :style="{ width: thumbGenPercent + '%' }"
                      />
                    </div>
                    <div class="thumb-gen-text">
                      <span v-if="scan.thumbGenProgress.isRunning">{{
                        $t('settings.genStatusRunning', {
                          generated: scan.thumbGenProgress.generated,
                          total: scan.thumbGenProgress.total,
                        })
                      }}</span>
                      <span
                        v-if="scan.thumbGenProgress.isRunning && scan.thumbGenProgress.phase"
                        class="thumb-gen-phase"
                        >[{{ scan.thumbGenProgress.phase }}]</span
                      >
                      <span v-else-if="scan.thumbGenProgress.status === 'completed'">{{
                        $t('settings.genStatusCompleted')
                      }}</span>
                      <span v-else-if="scan.thumbGenProgress.status === 'cancelled'">{{
                        $t('settings.genStatusCancelled')
                      }}</span>
                      <span v-else-if="scan.thumbGenProgress.status === 'error'">{{
                        $t('settings.genStatusError')
                      }}</span>
                    </div>
                  </div>
                </template>
                <div class="setting-actions">
                  <UiButton
                    v-if="scan.thumbGenProgress.isRunning"
                    @click="scan.stopFullThumbnailGeneration()"
                  >
                    {{ $t('settings.stopGen') }}
                  </UiButton>
                  <UiButton v-else variant="primary" @click="scan.startFullThumbnailGeneration()">
                    {{ $t('settings.startGen') }}
                  </UiButton>
                </div>
              </SettingRow>
              <!-- 特例:缩略图尺寸档。5 段较宽,与标签并排时窄宽度下 info 列会被挤到 0,
               中文标签逐字竖排(见反馈图)。改为标签在上、分段条整行铺满、各档等宽。 -->
              <SettingRow v-else-if="key === 'thumbSize'" setting-key="thumbSize" no-control>
                <template #extra>
                  <DynamicSettingControl setting-key="thumbSize" class="thumb-size-full" />
                </template>
              </SettingRow>
              <!-- gpuEngine 行仅在解码策略=GPU 时可见(rowVisible,与原 v-if 一致) -->
              <SettingRow v-else-if="rowVisible(key)" :setting-key="key" />
            </template>
          </CollapsibleCard>

          <!-- ── 视频 ─────────────────────────────────────────── -->
          <CollapsibleCard id="video" :title="$t('settings.video')">
            <template v-for="key in sectionSettingKeys('video')" :key="key">
              <!-- 特例:视频封面/关键帧手动提取(增量/全量/停止 + 进度)。
               两个动作钮(增量/全量)均为 .btn(全局 white-space:nowrap),并排放右侧控件槽时其 min-content
               会在窄内容列下吃满整行,把 info 列(min-width:0)挤到 0,中文标签/描述逐字竖排(同 thumbSize 旧疾)。
               故不放控件槽:no-control 省右列,标签/描述占整行,按钮整行铺在描述下方(#extra)。
               是否连带关键帧由上方 enableVideoKeyframes 开关决定(单一事实源)。 -->
              <SettingRow v-if="key === 'videoDeriveGen'" setting-key="videoDeriveGen" no-control>
                <template #extra>
                  <div v-if="derive.isVideoRunning" class="thumb-gen-status">
                    <div class="progress-bar">
                      <div
                        class="progress-bar__fill progress-shimmer"
                        :style="{ width: videoDerivePercent + '%' }"
                      />
                    </div>
                    <div class="thumb-gen-text">
                      <span>{{ derive.videoFinished }} / {{ derive.videoTotal }}</span>
                    </div>
                  </div>
                  <div class="setting-actions setting-actions--stacked">
                    <UiButton v-if="derive.isVideoRunning" @click="derive.stopVideoExtraction()">
                      {{ $t('settings.videoDeriveStop') }}
                    </UiButton>
                    <template v-else>
                      <UiButton variant="primary" @click="derive.startVideoIncremental()">
                        {{ $t('settings.videoDeriveIncremental') }}
                      </UiButton>
                      <UiButton @click="derive.startVideoFull()">
                        {{ $t('settings.videoDeriveFull') }}
                      </UiButton>
                    </template>
                  </div>
                </template>
              </SettingRow>
              <SettingRow v-else :setting-key="key" />
            </template>
            <!-- 视频可播产物独立池(视频格式扩展子系统 design.md §5.4):不共用上方缩略图 10GB 池
             (GB 级转码/改封产物会把全库缩略图驱逐殆尽)。实时占用走 VIDEO_CACHE_STATS(照
             GET_CACHE_STATS 扩容先例),进设置页 onMounted 拉一次;取回前/失败时降级为静态
             上限展示(镜像后端 `DEFAULT_VIDEO_CACHE_MAX_MB` 常量,不代表实时占用)。 -->
            <div class="settings-card__item video-cache-pool-static">
              <div class="settings-card__info">
                <div class="settings-card__label">{{ $t('settings.videoCachePoolTitle') }}</div>
                <div class="settings-card__desc">
                  <template v-if="videoCacheStats">{{
                    $t('settings.videoCachePoolUsage', {
                      used: formatFileSize(videoCacheStats.bytes),
                      limit: formatFileSize(videoCacheStats.limitMb * 1024 * 1024),
                      files: videoCacheStats.files,
                    })
                  }}</template>
                  <template v-else>{{
                    $t('settings.videoCachePoolDesc', {
                      size: formatFileSize(DEFAULT_VIDEO_CACHE_MAX_BYTES),
                    })
                  }}</template>
                </div>
              </div>
            </div>
          </CollapsibleCard>
        </section>

        <section v-show="currentSection === 'ai'" id="settings-ai" class="settings-section">
          <div class="settings-section__heading">
            <div>
              <h2>{{ $t('settings.sectionAi') }}</h2>
            </div>
          </div>
          <!-- ── AI 模型配置 ──────────────────────────────────── -->
          <CollapsibleCard id="aiModels" :title="$t('settings.aiModels')">
            <template v-for="key in sectionSettingKeys('aiModels')" :key="key">
              <!-- 特例:引擎状态(设备/显存/模型加载状态描述 + 测试加载按钮) -->
              <SettingRow v-if="key === 'aiEngineStatus'" setting-key="aiEngineStatus">
                <template #desc>
                  <div class="settings-card__desc">
                    {{ ai.providerLabel }} {{ ai.status.gpuName ? `(${ai.status.gpuName})` : '' }}
                    <span v-if="ai.status.vramGb !== null">
                      [{{ $t('settings.aiVram') }}: {{ ai.status.vramGb }}GB]</span
                    >
                    <span v-if="!ai.status.clipLoaded" class="ai-status-warn">
                      {{ $t('settings.aiModelNotLoaded') }}</span
                    >
                    <span v-else class="ai-status-ok"> {{ $t('settings.aiModelLoaded') }}</span>
                  </div>
                </template>
                <UiButton @click="ai.initEngine" :disabled="ai.status.clipLoaded">
                  {{ $t('settings.aiTestLoad') }}
                </UiButton>
              </SettingRow>
              <SettingRow v-else :setting-key="key" />
            </template>
            <!-- 手动导入 / 图像·文本模型选择已移除：模型的下载与切换统一由下方「模型库」管理。 -->
          </CollapsibleCard>

          <!-- ── AI 模型库（下载与切换，Layer B）──────────────────── -->
          <ModelLibrary />

          <!-- ── 人脸模型库（F7，只读：双轨 + 安装状态）──────────────── -->
          <FaceModelLibrary />

          <!-- ── OCR 模型分节（B′ 路线 T10：预下载入口 + 档位切换）──────────────── -->
          <OcrModelSection />

          <!-- ── 影像增强模型分节（P0 批 5：降噪/超分/去伪影模型下载 + 删除）──────── -->
          <EnhanceSettingsSection />
        </section>

        <section
          v-show="currentSection === 'storage'"
          id="settings-storage"
          class="settings-section"
        >
          <div class="settings-section__heading">
            <div>
              <h2>{{ $t('settings.sectionStorage') }}</h2>
            </div>
          </div>
          <!-- ── 数据备份与恢复（方案 B §8）───────────────────── -->
          <BackupSection @toggle="onBackupCardToggle" />

          <!-- ── 根文件夹显隐（V21，库级排除）───────────────────── -->
          <RootFolderVisibilitySection />

          <!-- ── 网络存储（需求8 8B, §3.8）─────────────────────── -->
          <NetworkStorageSection />

          <!-- ── 已知卷（T13 §3.7 离线 UX）──────────────────────── -->
          <KnownVolumesSection />
        </section>

        <section
          v-show="currentSection === 'advanced'"
          id="settings-advanced"
          class="settings-section settings-section--advanced"
        >
          <div class="settings-section__heading">
            <div>
              <h2>{{ $t('settings.sectionAdvanced') }}</h2>
            </div>
          </div>
          <!-- ── 开发者工具 ─────────────────────────────────── -->
          <CollapsibleCard id="debug" :title="$t('sidebar.debugSettings')">
            <template v-for="key in sectionSettingKeys('debug')" :key="key">
              <!-- 特例:日志目录(可点路径描述 + 换目录按钮) -->
              <SettingRow v-if="key === 'logDir'" setting-key="logDir">
                <template #desc>
                  <div
                    class="settings-card__desc clickable-path"
                    @click="openDirectory(logDir)"
                    :title="$t('settings.openInExplorer')"
                  >
                    {{ logDir || $t('settings.fetchingPath') }}
                  </div>
                </template>
                <UiButton @click="changeLogDir">
                  {{ $t('settings.changeDir') }}
                </UiButton>
              </SettingRow>
              <!-- 特例:外置配置文件(config.toml,批次B) —— 说明文案 + 当前路径 + 用外部编辑器打开按钮。 -->
              <SettingRow v-else-if="key === 'configFile'" setting-key="configFile">
                <template #desc>
                  <div class="settings-card__desc">{{ $t('settings.configFileDesc') }}</div>
                  <div class="settings-card__desc configfile-path">
                    {{ configFile.status.value?.path || $t('settings.fetchingPath') }}
                  </div>
                </template>
                <UiButton @click="handleOpenConfigFile">
                  {{ $t('settings.openConfigFileBtn') }}
                </UiButton>
              </SettingRow>
              <SettingRow v-else :setting-key="key" />
            </template>
          </CollapsibleCard>

          <!-- ── 危险操作(设计 §7.2:破坏性项独立分区,不与普通设置混排;默认折叠=渐进披露)── -->
          <CollapsibleCard
            id="danger"
            :title="$t('settings.dangerZone')"
            :default-open="false"
            class="settings-card--danger"
          >
            <p class="danger-zone__hint">{{ $t('settings.dangerZoneHint') }}</p>
            <SettingRow v-for="key in sectionSettingKeys('danger')" :key="key" :setting-key="key" />
          </CollapsibleCard>
        </section>
      </main>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted, computed, onUnmounted, nextTick, watch } from 'vue'
import { invokeIpc } from '../utils/ipc'
import { logger } from '../utils/logger'
import { useUiStore } from '../stores/uiStore'
import { useToastStore } from '../stores/toastStore'
import { useScanStore } from '../stores/scanStore'
import { useMediaStore } from '../stores/mediaStore'
import { useConfirm } from '../composables/useConfirm'
import { useAiStore } from '../stores/aiStore'
import { useConfigStore } from '../stores/configStore'
import { useThemeStore } from '../stores/themeStore'
import { useDerivationStore } from '../stores/derivationStore'
import { useConfigFile } from '../composables/useConfigFile'
import { useI18n } from 'vue-i18n'
import { useBackupI18n } from '../i18n/backupMessages'
import { ArrowLeft, AlertTriangle, Search, Info, Trash2, X, GraduationCap } from '@lucide/vue'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { useRoute, useRouter } from 'vue-router'
import { IPC } from '../constants/ipc'
import { isMobilePlatform } from '../utils/platform'
import { getSettingSpec, sectionSettingKeys, type SettingKey } from '../constants/settingsMap'
import { SETTINGS_SECTIONS, SETTINGS_GROUPS, type SettingsNavId } from '../constants/settingsLayout'
import { useSettingsCards } from '../composables/useSettingsCards'
import { formatFileSize } from '../utils/format'
import { setThumbCacheDir } from '../utils/thumbCacheDir'
import SettingRow from '../components/settings/SettingRow.vue'
import DynamicSettingControl from '../components/settings/DynamicSettingControl.vue'
import ThemeSettings from '../components/settings/ThemeSettings.vue'
import ReaderSettingsSection from '../components/settings/ReaderSettingsSection.vue'
import NetworkStorageSection from '../components/settings/NetworkStorageSection.vue'
import KnownVolumesSection from '../components/settings/KnownVolumesSection.vue'
import RootFolderVisibilitySection from '../components/settings/RootFolderVisibilitySection.vue'
import BackupSection from '../components/settings/BackupSection.vue'
import ModelLibrary from '../components/settings/ModelLibrary.vue'
import FaceModelLibrary from '../components/settings/FaceModelLibrary.vue'
import OcrModelSection from '../components/settings/OcrModelSection.vue'
import EnhanceSettingsSection from '../components/settings/EnhanceSettingsSection.vue'
import CollapsibleCard from '../components/settings/CollapsibleCard.vue'
import UiButton from '../components/ui/UiButton.vue'
import { useSettingsCacheStats } from '../composables/useSettingsCacheStats'
import { useIccProfileManager } from '../composables/useIccProfileManager'

const ui = useUiStore()
const toast = useToastStore()
const scan = useScanStore()
const media = useMediaStore()
// 高级元数据警告走 app 内 ConfirmDialog(原生 window.confirm 在 Tauri webview 不可靠,警告会静默穿透)。
const { confirm } = useConfirm()
const ai = useAiStore()
const config = useConfigStore()
// 主题草稿随设置页退出丢弃:取消／Esc／退出设置都丢弃草稿,无需额外确认(方案 §2)。
const theme = useThemeStore()
const derive = useDerivationStore()
// 外置配置文件(config.toml,批次B):App.vue 已挂过一次全局监听,这里再次调用只是复用同一份
// 共享状态(status/lastError 跨调用同源,见 useConfigFile.ts 头注),不会重复注册监听。
const configFile = useConfigFile()
const { t } = useI18n()
const { t: bt } = useBackupI18n()
const route = useRoute()
const router = useRouter()

// 视频派生进度(特例行):完成数/总数,分母含四态(pending/processing/done/error)。
const videoDerivePercent = computed(
  () => (derive.videoFinished / Math.max(derive.videoTotal, 1)) * 100,
)

interface SettingsSearchResult {
  id: string
  section: SettingsNavId
  sectionLabelKey: string
  groupId: string
  groupLabel: string
  label: string
  description: string
  settingKey?: SettingKey
}

const settingsSections = SETTINGS_SECTIONS
const generalSections = settingsSections.filter(({ id }) => ['common', 'appearance', 'gallery'].includes(id))
const settingsQuery = ref('')
const defaultSection: SettingsNavId = 'appearance'
const cards = useSettingsCards()

function groupsFor(section: SettingsNavId) {
  return SETTINGS_GROUPS.filter(group => group.section === section && group.keys.length && (!group.desktopOnly || !isMobilePlatform))
}
const thumbnailGroups = groupsFor('media').filter(group => group.id !== 'video')
const timelineSummary = computed(() => t('settings.timelineSummary', {
  axis: config.timelineAxisWidth, thumb: config.timelineScrollWidth,
}))

const searchCatalog = computed<SettingsSearchResult[]>(() => SETTINGS_GROUPS
  .filter(group => !group.desktopOnly || !isMobilePlatform)
  .flatMap(group => {
    const sectionLabelKey = settingsSections.find(section => section.id === group.section)?.labelKey ?? ''
    const groupLabel = group.id === 'backup' ? bt(group.titleKey) : t(group.titleKey)
    const base = { section: group.section, sectionLabelKey, groupId: group.id, groupLabel }
    if (!group.keys.length) return [{ ...base, id: group.id, label: groupLabel,
      description: group.id === 'backup' ? bt('backup.searchTerms') : '' }]
    return group.keys.filter(rowVisible).map(key => {
      const spec = getSettingSpec(key)!
      return { ...base, id: key, settingKey: key, label: t(spec.label),
        description: [spec.descKey, spec.summaryKey, spec.searchTermsKey].filter((key): key is string => !!key).map(key => t(key)).join(' ') }
    })
  }))
const searchResults = computed(() => {
  const query = settingsQuery.value.trim().toLocaleLowerCase()
  if (!query) return []
  return searchCatalog.value.filter(result =>
    [result.label, result.description, result.groupLabel, t(result.sectionLabelKey)]
      .join(' ').toLocaleLowerCase().includes(query),
  ).slice(0, 10)
})

function normalizeSection(value: unknown): SettingsNavId {
  if (value === 'general') return 'appearance'
  return settingsSections.some(section => section.id === value) ? value as SettingsNavId : defaultSection
}
const currentSection = ref<SettingsNavId>(normalizeSection(route.params.section))
function sectionPath(section: SettingsNavId): string {
  return section === defaultSection ? '/settings' : `/settings/${section}`
}
function selectSection(section: SettingsNavId) {
  settingsQuery.value = ''
  currentSection.value = section
  void router.replace(sectionPath(section))
}
watch(() => route.params.section, section => { currentSection.value = normalizeSection(section) })
watch(currentSection, async () => {
  await nextTick()
  // 窄屏分类横向滚动；搜索跨分类后仍需露出当前分类。
  document.querySelector<HTMLElement>('.settings-nav__item.active')
    ?.scrollIntoView({ block: 'nearest', inline: 'nearest' })
})

async function selectSearchResult(result: SettingsSearchResult) {
  selectSection(result.section)
  cards.expand(result.groupId)
  await nextTick()
  const group = document.querySelector<HTMLElement>(`[data-settings-card="${result.groupId}"]`)
  const target = result.settingKey
    ? group?.querySelector<HTMLElement>(`[data-setting-key="${result.settingKey}"]`)
    : group
  if (!target) return
  // 配置文件列表按需展开；搜索命中时同时露出当前文件。
  if (result.settingKey === 'viewerIccManager') {
    const list = target.querySelector('details.settings-inline-details')
    if (list instanceof HTMLDetailsElement) list.open = true
  }
  target.scrollIntoView({ block: 'center', behavior: window.matchMedia('(prefers-reduced-motion: reduce)').matches ? 'instant' : 'smooth' })
  target.classList.remove('settings-search-highlight')
  void target.offsetWidth
  target.classList.add('settings-search-highlight')
  target.addEventListener('animationend', () => target.classList.remove('settings-search-highlight'), { once: true })
  const control = target.querySelector<HTMLElement>('input, select, .settings-card__header-toggle, button:not(.pin-btn), summary')
  control?.focus({ preventScroll: true })
}

function onBackupCardToggle(open: boolean) {
  if (!open) return
  // 备份卡片高度较大；展开时明确保持存储分区，避免动态内容改变时导航跳走。
  currentSection.value = 'storage'
}

// gpuEngine 行仅在解码策略=GPU 时显示(注册式重构前的行级 v-if 原样保留,§8)。
function rowVisible(key: string): boolean {
  return key !== 'gpuEngine' || config.thumbStrategy === 'gpu'
}

// 悬停信息元素多选面板(顺序即渲染顺序;geo 的 i18n key 为历史命名 thumbInfoLocation)。
const THUMB_INFO_ELEMENTS = [
  { value: 'status', labelKey: 'settings.thumbInfoStatus' },
  { value: 'type', labelKey: 'settings.thumbInfoType' },
  { value: 'favorite', labelKey: 'settings.thumbInfoFavorite' },
  { value: 'size', labelKey: 'settings.thumbInfoSize' },
  { value: 'resolution', labelKey: 'settings.thumbInfoResolution' },
  { value: 'date', labelKey: 'settings.thumbInfoDate' },
  { value: 'filename', labelKey: 'settings.thumbInfoFilename' },
  { value: 'path', labelKey: 'settings.thumbInfoPath' },
  { value: 'geo', labelKey: 'settings.thumbInfoLocation' },
  { value: 'camera', labelKey: 'settings.thumbInfoCamera' },
  { value: 'params', labelKey: 'settings.thumbInfoParams' },
]

// 缓存占用统计(缩略图/日志目录 + 缩略图缓存统计 + 视频可播产物独立池统计)下沉
// useSettingsCacheStats;自定义 ICC 导入/枚举/删除下沉 useIccProfileManager
// (超长文件拆分方案 tierB-2 §SettingsView.vue ②)。
const {
  thumbCacheDir,
  logDir,
  cacheStats,
  cacheStatsLoading,
  videoCacheStats,
  DEFAULT_VIDEO_CACHE_MAX_BYTES,
  refreshCacheStats,
  refreshVideoCacheStats,
  cacheStatRows,
} = useSettingsCacheStats()

const { iccProfiles, refreshIccProfiles, importIccProfile, selectIccProfile, deleteIccProfile } =
  useIccProfileManager()

const thumbGenPercent = computed(() => {
  const { generated, total } = scan.thumbGenProgress
  if (!total) return 0
  return Math.min(100, Math.round((generated / total) * 100))
})

onMounted(async () => {
  // 视频派生计数:进设置页即取一次(fetch 内含「在跑则起表轮询」),特例行状态才不落后。
  void derive.fetchVideoStatus()
  await config.loadConfig()

  try {
    thumbCacheDir.value = await invokeIpc<string>(IPC.GET_THUMB_CACHE_DIR)
  } catch (e) {
    logger.warn('Failed to fetch resolved cache dir', { error: e })
  }

  // 缓存占用统计:不 await(遍历大缓存秒级),结果就绪后行体自会响应式补上。
  void refreshCacheStats()
  void refreshVideoCacheStats()

  // 自定义 ICC 列表(方案 B §0④):移动端行本就隐藏,不发起(D-414)。
  if (!isMobilePlatform) void refreshIccProfiles()

  try {
    logDir.value = await invokeIpc<string>(IPC.GET_LOG_DIR)
  } catch (e) {
    logger.warn('Failed to fetch resolved log dir', { error: e })
  }

  try {
    await ai.fetchStatus()
  } catch (e) {
    logger.error('Failed to get ai status', { error: e })
  }

  document.addEventListener('keydown', onKeyDown)
})

onUnmounted(() => {
  document.removeEventListener('keydown', onKeyDown)
  // 路由离开(含浏览器后退)同样丢弃主题草稿:只切设置分类不触发本钩子,不会误取消。
  theme.cancelEdit()
})

function onKeyDown(e: KeyboardEvent) {
  if (e.key !== 'Escape') return
  if (settingsQuery.value.trim()) {
    settingsQuery.value = ''
    return
  }
  closeSettings()
}

async function changeLogDir() {
  try {
    const selected = await openDialog({
      directory: true,
      multiple: false,
      title: t('settings.chooseLogDir'),
    })
    if (selected && typeof selected === 'string') {
      await config.saveConfig('log_dir', selected)
      logDir.value = selected
      toast.addToast('success', t('settings.logDirChanged'))
    }
  } catch (e) {
    logger.error('Failed to select log directory', { error: e })
  }
}

async function changeCacheDir() {
  try {
    const selected = await openDialog({
      directory: true,
      multiple: false,
      title: t('settings.chooseCacheDir'),
    })
    if (selected && typeof selected === 'string') {
      await config.saveConfig('thumb_cache_dir', selected)
      thumbCacheDir.value = selected
      // 展示保持 OS 原生形态;同步 thumbCacheDir 模块缓存,防其他消费方读到旧目录。
      setThumbCacheDir(selected)
      toast.addToast('success', t('settings.cacheDirChanged'))
    }
  } catch (e) {
    logger.error('Failed to select directory', { error: e })
  }
}

// 外置配置文件(config.toml,批次B):打开系统默认编辑器。成功后无需本地状态变更——
// 后端保存时广播 config-file-changed/config-file-error,useConfigFile 的全局监听器会更新
// lastError 并刷新 config/ui store;这里只兜底展示「打开动作本身」失败(如无默认编辑器)。
async function handleOpenConfigFile() {
  try {
    await configFile.openInEditor()
  } catch (e) {
    toast.addToast('error', t('settings.openConfigFileFailed', { error: e }))
  }
}

async function openDirectory(path: string) {
  if (!path) return
  try {
    await invokeIpc(IPC.OPEN_DIRECTORY, { path })
  } catch (e) {
    toast.addToast('error', t('settings.openDirFailed', { error: e }))
  }
}

async function handleThumbInfoToggle(e: MouseEvent, val: string) {
  const el = e.target as HTMLInputElement
  const isChecked = el.checked
  const isAdvanced = ['geo', 'camera', 'params'].includes(val)

  if (isChecked && isAdvanced) {
    // 异步确认无法在 await 后 preventDefault(事件已结算);取消时手动撤销原生勾选,
    // 并跳过集合写入——:checked 绑定值仍为 false,Vue 下一帧与之对齐。
    const { confirmed } = await confirm({
      title: t('settings.advancedMetadataTitle'),
      message: t('settings.advancedMetadataWarning'),
    })
    if (!confirmed) {
      el.checked = false
      return
    }
  }

  const current = new Set(ui.thumbInfoElements)
  if (isChecked) {
    current.add(val)
  } else {
    current.delete(val)
  }
  ui.setThumbInfoElements(Array.from(current))
  media.invalidateLayout()
}

function closeSettings() {
  // 退出设置即丢弃主题草稿(取消／Esc／返回同语义,无需额外确认;方案 §2)。
  theme.cancelEdit()
  if (window.history.state?.back) router.back()
  else void router.replace('/')
}
</script>

<style scoped src="./SettingsView.styles.css"></style>
