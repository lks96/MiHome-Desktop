<script setup lang="ts">
import { computed, ref, onMounted, onUnmounted, defineAsyncComponent } from 'vue'
import { NButton, NConfigProvider, NIcon, NTag, darkTheme } from 'naive-ui'
import {
  AppsOutline, VideocamOutline, HardwareChipOutline, FlashOutline,
  SettingsOutline, WifiOutline, MenuOutline,
} from '@vicons/ionicons5'

const SettingsView = defineAsyncComponent(() => import('./views/SettingsView.vue'))
const EntityControl = defineAsyncComponent(() => import('./components/EntityControl.vue'))
const ScenesView = defineAsyncComponent(() => import('./views/ScenesView.vue'))
const MonitorView = defineAsyncComponent(() => import('./views/MonitorView.vue'))
const DevicesView = defineAsyncComponent(() => import('./views/DevicesView.vue'))
import { isTauri } from '@tauri-apps/api/core'
import { useDevicesStore } from './stores/devices'
import { useConnectionStore } from './stores/connection'
import { useDesktopStore } from './stores/desktop'
import { useSettingsStore } from './stores/settings'

const settingsStore = useSettingsStore()
const desktop = useDesktopStore()
const connection = useConnectionStore()
const devices = useDevicesStore()
const favorites = computed(() => devices.entities.filter(entity => entity.preference.isFavorite && !entity.preference.isHidden).sort((a,b) => a.preference.sortOrder - b.preference.sortOrder || a.displayName.localeCompare(b.displayName)))
const hasSnapshot = computed(() => connection.status.state === 'connected' || connection.entities.length > 0)
const entityCount = computed(() => hasSnapshot.value ? connection.entities.length : '—')
const onlineCount = computed(() => hasSnapshot.value ? connection.entities.filter(entity => !['unavailable', 'unknown'].includes(entity.state)).length : '—')
const sceneCount = computed(() => hasSnapshot.value ? connection.entities.filter(entity => entity.domain === 'scene').length : '—')
const cameraCount = computed(() => hasSnapshot.value ? connection.entities.filter(entity => entity.domain === 'camera').length : '—')
const media = window.matchMedia('(prefers-color-scheme: dark)')
const systemDark = ref(media.matches)
const updateSystemTheme = (event: MediaQueryListEvent) => { systemDark.value = event.matches }
const isDark = computed(() => settingsStore.settings.theme === 'dark' || (settingsStore.settings.theme === 'system' && systemDark.value))
onMounted(() => { void settingsStore.load(); void desktop.initialize(); if (isTauri()) void devices.load(); void connection.initialize(); media.addEventListener('change', updateSystemTheme) })
onUnmounted(() => { connection.dispose(); desktop.dispose(); media.removeEventListener('change', updateSystemTheme) })

type Section = 'overview' | 'monitor' | 'devices' | 'scenes' | 'settings'
const active = ref<Section>('overview')
const collapsed = ref(false)
const nav = [
  { id: 'overview' as const, label: '总览', icon: AppsOutline },
  { id: 'monitor' as const, label: '监控中心', icon: VideocamOutline },
  { id: 'devices' as const, label: '设备中心', icon: HardwareChipOutline },
  { id: 'scenes' as const, label: '智能场景', icon: FlashOutline },
]
const title = computed(() => [...nav, { id: 'settings' as const, label: '设置' }].find(item => item.id === active.value)?.label ?? '总览')
</script>

<template>
  <NConfigProvider :theme="isDark ? darkTheme : null">
    <div class="shell" :class="{ 'is-collapsed': collapsed, 'theme-dark': isDark }">
      <aside class="sidebar">
        <div class="brand"><div class="brand-mark">米</div><span class="brand-name">Mi Home<span>DESKTOP</span></span></div>
        <div class="workspace-label">工作空间</div>
        <nav aria-label="主导航">
          <button v-for="item in nav" :key="item.id" class="nav-item" :class="{ active: active === item.id }" @click="active = item.id">
            <NIcon :component="item.icon" :size="19" /><span>{{ item.label }}</span>
          </button>
        </nav>
        <div class="sidebar-bottom">
          <button class="nav-item" :class="{ active: active === 'settings' }" @click="active = 'settings'"><NIcon :component="SettingsOutline" :size="19" /><span>设置</span></button>
          <div class="connection-card"><span class="connection-dot"></span><div><strong>{{ connection.saved?.name ?? 'Home Assistant' }}</strong><small>{{ connection.label }}</small></div><NIcon :component="WifiOutline" :size="17" /></div>
        </div>
      </aside>
      <main class="main-area">
        <header class="topbar"><NButton quaternary circle @click="collapsed = !collapsed"><template #icon><NIcon :component="MenuOutline" /></template></NButton><div class="breadcrumbs">Mi Home Desktop <span>/</span> <strong>{{ title }}</strong></div><div class="topbar-right"><NTag :bordered="false" round size="small" :type="connection.status.state === 'connected' ? 'success' : 'default'">{{ connection.label }}</NTag><div class="avatar">M</div></div></header>
        <section class="content">
          <template v-if="active === 'overview'">
            <div class="page-heading"><div><div class="eyebrow">SMART HOME CONTROL</div><h1>欢迎回来</h1><p>{{ connection.status.state === 'connected' ? '实时连接已建立，设备状态正在同步。' : '连接 Home Assistant，开始管理你的智能家居。' }}</p></div><NButton type="primary" size="large" @click="active = 'settings'">配置连接</NButton></div>
            <div class="hero-card"><div class="hero-copy"><div class="hero-kicker">你的智能家居，一目了然</div><h2>让每个设备<br>都触手可及。</h2><p>{{ connection.status.state === 'connected' ? '已连接你的 Home Assistant 实例，前往设备中心查看实时实体。' : '连接 Home Assistant 后，你的设备、房间和场景将在这里汇集。' }}</p><NButton secondary @click="active = 'settings'">前往连接设置 <span class="arrow">→</span></NButton></div><div class="hero-art" aria-hidden="true"><div class="orbit orbit-one"></div><div class="orbit orbit-two"></div><div class="house"><div class="roof"></div><div class="house-body"><div class="window"></div><div class="door"></div></div></div><div class="spark spark-one">✦</div><div class="spark spark-two">✧</div></div></div>
            <div class="section-heading"><div><h2>空间概览</h2><p>{{ connection.syncing ? '正在同步实体…' : connection.label }}</p></div><span class="muted">{{ connection.status.state === 'connected' ? '实时状态' : '等待连接或恢复同步' }}</span></div>
            <div class="stat-grid"><article class="stat-card"><div class="stat-icon blue"><NIcon :component="HardwareChipOutline" :size="20" /></div><div class="stat-label">实体总数</div><div class="stat-value">{{ entityCount }}</div><div class="stat-foot">{{ hasSnapshot ? 'Home Assistant 实体' : '等待同步' }}</div></article><article class="stat-card"><div class="stat-icon violet"><NIcon :component="WifiOutline" :size="20" /></div><div class="stat-label">可用实体</div><div class="stat-value">{{ onlineCount }}</div><div class="stat-foot">{{ hasSnapshot ? '不含 unavailable / unknown' : '连接 Home Assistant 后同步' }}</div></article><article class="stat-card"><div class="stat-icon amber"><NIcon :component="FlashOutline" :size="20" /></div><div class="stat-label">场景实体</div><div class="stat-value">{{ sceneCount }}</div><div class="stat-foot">复用 Home Assistant 场景</div></article><article class="stat-card"><div class="stat-icon rose"><NIcon :component="VideocamOutline" :size="20" /></div><div class="stat-label">摄像头</div><div class="stat-value">{{ cameraCount }}</div><div class="stat-foot">视频兼容性需实机验证</div></article></div>
            <section v-if="favorites.length" class="panel" style="margin-top:18px"><h3>收藏快捷控制</h3><div class="favorite-grid"><article v-for="entity in favorites" :key="entity.entityId"><strong>{{ entity.displayName }}</strong><p>{{ entity.state }} {{ entity.unit }}</p><EntityControl :entity="entity" compact /></article></div></section>
            <div class="bottom-grid"><article v-if="!connection.saved" class="panel"><div class="panel-heading"><div><h3>快速开始</h3><p>完成以下步骤以连接你的智能家居</p></div><span class="step-count">0 / 3</span></div><div class="step-row"><span class="step-number">1</span><div><strong>准备 Home Assistant</strong><small>安装 Xiaomi Miot 并接入米家设备</small></div><span class="step-state">待完成</span></div><div class="step-row"><span class="step-number">2</span><div><strong>创建长期访问令牌</strong><small>在 Home Assistant 用户资料中创建</small></div><span class="step-state">待完成</span></div><div class="step-row"><span class="step-number">3</span><div><strong>连接你的实例</strong><small>填写地址并安全保存访问令牌</small></div><span class="step-state">待完成</span></div></article><article v-else class="panel"><div class="panel-heading"><div><h3>{{ connection.saved.name }}</h3><p>{{ connection.label }}</p></div></div><p class="muted">{{ connection.saved.baseUrl }}</p><p>{{ connection.status.state === 'connected' ? '实体快照和状态事件已同步。' : '实时状态暂停更新，请检查连接或稍后重试。' }}</p><NButton secondary @click="active = 'devices'">查看实体列表</NButton></article><article class="panel tip-panel"><div class="tip-icon">✦</div><div class="eyebrow">设计原则</div><h3>真实设备，真实状态</h3><p>设备状态由 Home Assistant 提供。摄像头支持情况会依据实际型号和连接方式验证。</p><div class="tip-rule"></div><div class="tip-meta"><span>隐私优先</span><span>本地连接</span><span>安全凭据</span></div></article></div>
          </template>
          <MonitorView v-else-if="active === 'monitor'" :active="desktop.visible" />
          <DevicesView v-else-if="active === 'devices'" />
          <ScenesView v-else-if="active === 'scenes'" />
          <SettingsView v-else-if="active === 'settings'" />
          <div v-else class="empty-section"><div class="empty-icon"><NIcon :component="nav.find(item => item.id === active)?.icon ?? SettingsOutline" :size="28" /></div><div class="eyebrow">{{ title.toUpperCase() }}</div><h1>{{ title }}</h1><p>此功能将在后续开发阶段接入真实 Home Assistant 数据。</p><NButton type="primary" @click="active = 'overview'">返回总览</NButton></div>
          <footer>Mi Home Desktop <span>·</span> 本地优先的智能家居控制台</footer>
        </section>
      </main>
    </div>
  </NConfigProvider>
</template>
