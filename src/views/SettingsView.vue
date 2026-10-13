<template>
  <div class="settings-view">
    <!-- 配置解析错误置顶显示；成功变更事件由 useConfigFile 清空错误。 -->
    <div v-if="configFile.lastError.value" class="settings-config-banner">
      <AlertTriangle :size="16" />
      <span>
        {{ $t('settings.configFileParseError', { message: configFile.lastError.value.message }) }}
        <template v-if="configFile.lastError.value.line != null">{{
          $t('settings.configFileErrorLine', { line: configFile.lastError.value.line })
        }}</template>
      </span>
    </div>

    <!-- 外部编辑触及需重启的设置键时，展示可关闭的信息提示。 -->
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
        <UiButton
          class="btn-sm settings-pin-all"
          :aria-pressed="allSettingsPinned"
          @click="toggleAllPinned"
        >
          <component :is="allSettingsPinned ? PinOff : Pin" :size="15" aria-hidden="true" />
          {{ $t(allSettingsPinned ? 'settings.unpinAllSettings' : 'settings.pinAllSettings') }}
        </UiButton>
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

      <main id="settings-main" ref="settingsContent" class="settings-content">
        <section v-for="section in mountedGeneralSections" :key="section.id"
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

        <section v-if="visitedSections.includes('media')" v-show="currentSection === 'media'" id="settings-media" class="settings-section">
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
                        v-if="scan.thumbGenProgress.isRunning && scan.thumbGenPhaseLabel"
                        class="thumb-gen-phase"
                        >[{{ scan.thumbGenPhaseLabel }}]</span
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
                    <span v-if="scan.thumbGenResultsLabel">{{ scan.thumbGenResultsLabel }}</span>
                    <ThumbExecutionSummary />
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
              <SettingRow v-else :setting-key="key" />
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

        <section v-if="visitedSections.includes('ai')" v-show="currentSection === 'ai'" id="settings-ai" class="settings-section">
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
          v-if="visitedSections.includes('storage')"
          v-show="currentSection === 'storage'"
          id="settings-storage"
          class="settings-section"
        >
          <div class="settings-section__heading">
            <div>
              <h2>{{ $t('settings.sectionStorage') }}</h2>
            </div>
          </div>
          <!-- 数据备份与恢复。 -->
          <BackupSection @toggle="onBackupCardToggle" />

          <!-- ── 根文件夹显隐（V21，库级排除）───────────────────── -->
          <RootFolderVisibilitySection />

          <!-- ── 网络存储（需求8 8B, §3.8）─────────────────────── -->
          <NetworkStorageSection />

          <!-- ── 已知卷（T13 §3.7 离线 UX）──────────────────────── -->
          <KnownVolumesSection />
        </section>

        <section
          v-if="visitedSections.includes('advanced')"
          v-show="currentSection === 'advanced'"
          id="settings-advanced"
          class="settings-section settings-section--advanced"
        >
          <div class="settings-section__heading">
            <div>
              <h2>{{ $t('settings.sectionAdvanced') }}</h2>
            </div>
          </div>
          <!-- 高级页也从分组表渲染，避免新设置只进入搜索与固定清单却没有页面入口。 -->
          <CollapsibleCard
            v-for="group in groupsFor('advanced')"
            :id="group.id"
            :key="group.id"
            :title="$t(group.titleKey)"
            :default-open="group.defaultOpen"
            :class="{ 'settings-card--danger': group.id === 'danger' }"
          >
            <p v-if="group.id === 'danger'" class="danger-zone__hint">{{ $t('settings.dangerZoneHint') }}</p>
            <template v-for="key in group.keys" :key="key">
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
              <!-- 配置文件展示路径，并提供外部编辑器入口。 -->
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

        </section>
      </main>
    </div>
  </div>
</template>

<script setup lang="ts">
import ThumbExecutionSummary from '../components/common/ThumbExecutionSummary.vue'
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
import { SettingsNotSavedError } from '../stores/settingsPersistence'
import { useThemeStore } from '../stores/themeStore'
import { useDerivationStore } from '../stores/derivationStore'
import { useConfigFile } from '../composables/useConfigFile'
import { useI18n } from 'vue-i18n'
import { useBackupI18n } from '../i18n/backupMessages'
import { ArrowLeft, AlertTriangle, Search, Info, Trash2, X, GraduationCap, Pin, PinOff } from '@lucide/vue'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { useRoute, useRouter } from 'vue-router'
import { IPC } from '../constants/ipc'
import { getSettingSpec, sectionSettingKeys, type SettingKey } from '../constants/settingsMap'
import { SETTINGS_SECTIONS, SETTINGS_GROUPS, type SettingsNavId } from '../constants/settingsLayout'
import { useSettingsCards } from '../composables/useSettingsCards'
import { useSettingsNavigation } from '../composables/useSettingsNavigation'
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
// 取消、Esc 或退出设置均丢弃未提交的主题草稿。
const theme = useThemeStore()
const derive = useDerivationStore()
// 复用 App.vue 注册的配置文件监听与共享状态，不重复注册监听器。
const configFile = useConfigFile()
const { t } = useI18n()
const { t: bt } = useBackupI18n()
const router = useRouter()
const route = useRoute()

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
const cards = useSettingsCards()
const { currentSection, visitedSections, settingsContent, selectSection: navigateToSection } = useSettingsNavigation()
const mountedGeneralSections = computed(() => generalSections.filter(section => visitedSections.value.includes(section.id)))

// 与页面可用设置共用分组表；保留用户既有顺序，批量取消只移除这些设置项。
const pinnableSettingKeys = [...new Set(SETTINGS_GROUPS
  .flatMap(group => group.keys))]
const allSettingsPinned = computed(() => pinnableSettingKeys.every(key => ui.pinnedSettings.includes(key)))
function toggleAllPinned() {
  // 设置服务统一回滚并提示写入失败。
  void ui.setPinnedSettings(pinnableSettingKeys, !allSettingsPinned.value).catch(() => {})
}

function groupsFor(section: SettingsNavId) {
  return SETTINGS_GROUPS.filter(group => group.section === section && group.keys.length)
}
const thumbnailGroups = groupsFor('media').filter(group => group.id !== 'video')
const timelineSummary = computed(() => t('settings.timelineSummary', {
  axis: config.timelineAxisWidth, thumb: config.timelineScrollWidth,
}))

const searchCatalog = computed<SettingsSearchResult[]>(() => SETTINGS_GROUPS
  .flatMap(group => {
    const sectionLabelKey = settingsSections.find(section => section.id === group.section)?.labelKey ?? ''
    const groupLabel = group.id === 'backup' ? bt(group.titleKey) : t(group.titleKey)
    const base = { section: group.section, sectionLabelKey, groupId: group.id, groupLabel }
    if (!group.keys.length) return [{ ...base, id: group.id, label: groupLabel,
      description: group.id === 'backup' ? bt('backup.searchTerms') : '' }]
    return group.keys.map(key => {
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

function selectSection(section: SettingsNavId) {
  settingsQuery.value = ''
  navigateToSection(section)
}

async function selectSearchResult(result: SettingsSearchResult) {
  selectSection(result.section)
  await revealSettingsResult(result)
}

// 下载引导与设置搜索共用展开、滚动和聚焦，深链进入时也能露出原本折叠的卡片。
watch(
  () => route.fullPath,
  () => {
    const result = searchCatalog.value.find(
      (item) => item.groupId === route.hash.slice(1)
        && item.section === currentSection.value && !item.settingKey,
    )
    if (result) void revealSettingsResult(result)
  },
  { immediate: true, flush: 'post' },
)

async function revealSettingsResult(result: SettingsSearchResult) {
  // 新分类须先登记卡片的默认折叠态，再展开命中的分组。
  await nextTick()
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

// 缓存统计与 ICC 管理分别由 useSettingsCacheStats、useIccProfileManager 负责。
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

async function loadThumbnailSettings() {
  try {
    thumbCacheDir.value = await invokeIpc<string>(IPC.GET_THUMB_CACHE_DIR)
  } catch (e) {
    logger.warn('Failed to fetch resolved cache dir', { error: e })
  }

  // 先取得目录，再查询其占用；不让未打开媒体分类的用户等待缓存遍历。
  void refreshCacheStats()
}

async function loadLogDirectory() {
  try {
    logDir.value = await invokeIpc<string>(IPC.GET_LOG_DIR)
  } catch (e) {
    logger.warn('Failed to fetch resolved log dir', { error: e })
  }

}

// 只加载本次新访问的分类；已有组件和数据保留，切回来不重复初始化。
watch(visitedSections, (sections, previous = []) => {
  for (const section of sections.slice(previous.length)) {
    if (section === 'media') {
      void derive.fetchVideoStatus()
      void loadThumbnailSettings()
      void refreshVideoCacheStats()
    } else if (section === 'gallery') {
      void refreshIccProfiles()
    } else if (section === 'advanced') {
      void loadLogDirectory()
    } else if (section === 'ai') {
      void ai.fetchStatus().catch(error => logger.error('Failed to get ai status', { error }))
    }
  }
}, { immediate: true })

onMounted(() => {
  void config.loadConfig()
  // 绑定不等待异步请求，确保快速离开时卸载钩子能完整清理。
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
    if (e instanceof SettingsNotSavedError) return
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
    if (e instanceof SettingsNotSavedError) return
    logger.error('Failed to select directory', { error: e })
  }
}

// 保存后的配置事件由全局监听器更新状态；此处仅展示打开编辑器失败。
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
  // 退出设置即丢弃未提交的主题草稿。
  theme.cancelEdit()
  if (window.history.state?.back) router.back()
  else void router.replace('/')
}
</script>

<style scoped src="./SettingsView.styles.css"></style>
