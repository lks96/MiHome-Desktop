<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { isTauri } from '@tauri-apps/api/core'
import { NAlert, NButton, NCheckbox, NDrawer, NDrawerContent, NEmpty, NInput, NInputNumber, NSelect, NSpin, NTag } from 'naive-ui'
import { useMijiaStore } from '../stores/mijia'
import { regions, type MiProperty } from '../types/mijia'
defineProps<{ camerasOnly?: boolean }>()
const emit = defineEmits<{ advanced: [] }>()
const store = useMijiaStore()
const native = isTauri()
const region = ref('cn')
const remember = ref(true)
const search = ref('')
const room = ref<string | null>(null)
const selected = ref<string | null>(null)
const clock = ref(Date.now())
const drafts = ref<Record<string, number | null>>({})
const remaining = computed(() => Math.max(0, Math.ceil((store.deadline - clock.value) / 1000)))
const roomKey = (home: string, roomName: string) => `${home} / ${roomName}`
const roomOptions = computed(() => [...new Set(store.snapshot.devices.map(d => roomKey(d.home, d.room)))].sort().map(value => ({ label: value, value })))
const filtered = computed(() => store.snapshot.devices.filter(d => (!room.value || roomKey(d.home,d.room) === room.value) && `${d.name} ${d.model} ${d.room}`.toLowerCase().includes(search.value.toLowerCase())))
const device = computed(() => store.snapshot.devices.find(d => d.did === selected.value))
watch(() => store.snapshot, value => { if (value.loggedIn) region.value = value.region; if (selected.value && !value.devices.some(d => d.did === selected.value)) selected.value = null }, { immediate: true })
const propertyKey = (p: MiProperty) => `${p.siid}:${p.piid}`
watch(() => store.properties, values => { drafts.value = Object.fromEntries(values.map(p => [propertyKey(p), typeof p.value === 'number' ? p.value : null])) })
watch(selected, did => { if (did) void store.loadProperties(did); else store.clearProperties() })
let clockTimer: ReturnType<typeof setInterval>
let syncTimer: ReturnType<typeof setInterval>
onMounted(() => {
  void store.initialize()
  clockTimer = setInterval(() => { clock.value = Date.now() }, 1000)
  syncTimer = setInterval(() => { if (!document.hidden && store.snapshot.loggedIn && !store.controlling) void store.sync() }, 60000)
})
onUnmounted(() => { clearInterval(clockTimer); clearInterval(syncTimer); void store.cancelLogin(); store.clearProperties() })
</script>

<template>
  <div class="page-heading"><div><div class="eyebrow">YOUR XIAOMI HOME</div><h1>{{ camerasOnly ? '米家摄像头' : '我的米家' }}</h1><p>打开米家 App 扫一扫，连接你的家。</p></div><div v-if="store.snapshot.loggedIn" class="mi-actions"><NButton :loading="store.syncing" @click="store.sync">刷新设备</NButton><NButton :disabled="store.controlling || store.syncing" @click="store.logout">退出账号</NButton></div></div>
  <NAlert v-if="!native" type="info" class="mi-alert">请在 Windows 桌面客户端中扫码登录。浏览器预览无法连接米家账号或保存会话。</NAlert>
  <NAlert v-if="store.error" type="error" class="mi-alert" role="alert">{{ store.error.message }}<NButton v-if="!store.ready && native" size="small" @click="store.initialize(true)">重试读取登录状态</NButton></NAlert>
  <NSpin :show="store.loading">
    <section v-if="!store.snapshot.loggedIn" class="panel mi-welcome">
      <div class="mi-intro"><div class="mi-mark">米</div><h2>扫码，让家出现在桌面上。</h2><p>无需搭建服务、创建令牌或手动添加设备。登录后自动同步米家中的家庭、房间和设备。</p><ol><li>选择与米家 App 相同的服务地区</li><li>打开米家 App，用扫一扫扫描右侧二维码</li><li>在手机上确认登录，设备将自动出现</li></ol><small>登录会话保存在 Windows 凭据管理器中，可随时退出。设备控制能力取决于型号及小米云支持。</small></div>
      <div class="mi-login">
        <label for="mi-region">账号服务地区</label><NSelect input-id="mi-region" aria-label="米家服务地区" v-model:value="region" :options="regions" :disabled="store.loggingIn" />
        <NCheckbox v-model:checked="remember" :disabled="store.loggingIn">记住登录，下次自动连接</NCheckbox>
        <div v-if="store.qr" class="mi-qr"><img :src="store.qr.image" alt="米家登录二维码" /><strong>用米家 App 扫一扫</strong><span>请在手机确认 · {{ remaining }} 秒后过期</span><NButton @click="store.cancelLogin">取消登录</NButton></div>
        <div v-else class="mi-qr-placeholder"><span aria-hidden="true">▦</span><p>{{ store.loggingIn ? '正在获取登录二维码…' : '准备好手机，开始连接你的家' }}</p><NButton type="primary" size="large" :disabled="!native || store.loading" :loading="store.loggingIn" @click="store.startLogin(region, remember)">{{ store.error ? '重新获取二维码' : '用米家 App 扫码登录' }}</NButton><NButton v-if="store.loggingIn" text @click="store.cancelLogin">取消</NButton></div>
      </div>
    </section>
    <template v-else>
      <div class="panel mi-account"><div><strong>{{ store.snapshot.accountLabel }}</strong><p>{{ regions.find(r => r.value === store.snapshot.region)?.label }} · {{ store.snapshot.devices.length }} 台设备 · {{ store.snapshot.devices.filter(d => d.online).length }} 台在线</p></div><small>{{ store.snapshot.updatedAt ? `上次同步 ${new Date(store.snapshot.updatedAt * 1000).toLocaleTimeString()}` : '等待首次同步' }}</small></div>
      <NAlert v-for="warning in store.snapshot.warnings" :key="warning" type="warning" class="mi-alert">{{ warning }}</NAlert>
      <div class="mi-filters"><NInput v-model:value="search" placeholder="搜索设备、型号或房间" aria-label="搜索米家设备" clearable /><NSelect v-model:value="room" :options="roomOptions" placeholder="全部房间" clearable aria-label="筛选米家房间" /></div>
      <NEmpty v-if="!filtered.filter(d => !camerasOnly || d.camera).length" style="padding:50px" :description="store.syncing ? '正在同步设备…' : store.snapshot.devices.length ? '没有匹配的设备' : '此地区暂无设备，请确认与米家 App 中的地区一致'" />
      <div class="mi-device-grid"><article v-for="d in filtered.filter(d => !camerasOnly || d.camera)" :key="d.did" class="panel mi-device" :data-mi-device="d.did"><div class="mi-device-top"><span class="mi-device-icon">{{ d.camera ? '◉' : '⌂' }}</span><NTag :type="d.online ? 'success' : 'default'" size="small" :bordered="false">{{ d.online ? '在线' : '离线' }}</NTag></div><h3>{{ d.name }}</h3><p>{{ d.home }} {{ d.room }}</p><small>{{ d.model }}</small><p v-if="d.camera" class="mi-compatibility">已识别摄像头 · 此型号直播待适配</p><NButton secondary :disabled="!d.online" @click="selected = d.did">{{ d.online ? '查看状态与控制' : '设备离线' }}</NButton></article></div>
    </template>
  </NSpin>
  <div class="mi-advanced"><span>已有 Home Assistant 或视频网关？</span><NButton text @click="emit('advanced')">打开高级接入设置 →</NButton></div>
  <NDrawer :show="!!selected" :width="560" @update:show="show => { if (!show) selected = null }"><NDrawerContent :title="device?.name ?? '设备控制'" closable><p>{{ device?.home }} {{ device?.room }} · {{ device?.model }}</p><NAlert v-if="device?.camera" type="info">已自动识别为摄像头。直接直播需要验证型号的取流方式；当前没有可用直播，其他已支持的设备能力显示在下方。</NAlert><NButton :loading="store.propertiesLoading" :disabled="store.controlling" @click="selected && store.loadProperties(selected)">刷新设备状态</NButton><NAlert v-if="store.propertiesError" type="warning" class="mi-alert" role="alert">{{ store.propertiesError.message }}</NAlert><p v-if="store.feedback" role="status">{{ store.feedback }}</p><NSpin :show="store.propertiesLoading"><NEmpty v-if="!store.propertiesLoading && !store.properties.length && !store.propertiesError" description="此设备尚未公开受支持的控制能力" style="padding:40px" /><section v-for="p in store.properties" :key="propertyKey(p)" class="mi-property"><div><strong>{{ p.name }}</strong><p>{{ p.unavailable ? '状态暂不可读' : typeof p.value === 'boolean' ? p.value ? '开启' : '关闭' : p.value ?? '—' }}</p></div><template v-if="p.writable"><div v-if="p.format === 'bool'" class="mi-actions"><NButton :type="p.value === true ? 'primary' : 'default'" :disabled="store.controlling || store.propertiesLoading || !device?.online" @click="store.setProperty(p, true)">开启</NButton><NButton :type="p.value === false ? 'primary' : 'default'" :disabled="store.controlling || store.propertiesLoading || !device?.online" @click="store.setProperty(p, false)">关闭</NButton></div><div v-else class="mi-numeric"><NSelect v-if="p.choices.length" v-model:value="drafts[propertyKey(p)]" :options="p.choices.map(c => ({ label: c.description, value: c.value }))" :disabled="store.controlling" /><NInputNumber v-else v-model:value="drafts[propertyKey(p)]" :min="p.range?.[0]" :max="p.range?.[1]" :step="p.range?.[2]" :disabled="store.controlling" /><NButton :disabled="drafts[propertyKey(p)] == null || store.controlling || store.propertiesLoading || !device?.online" @click="store.setProperty(p, drafts[propertyKey(p)])">应用</NButton></div></template></section></NSpin><p class="mi-footnote">操作后重新读取设备报告的状态。较旧的 Wi-Fi 设备、蓝牙及网关子设备可能暂不支持云端控制。</p></NDrawerContent></NDrawer>
</template>

<style scoped>
.mi-alert{margin:16px 0}.mi-actions{display:flex;gap:10px}.mi-welcome{display:grid;grid-template-columns:1.2fr 1fr;gap:48px;padding:40px;max-width:1050px}.mi-mark{width:50px;height:50px;border-radius:16px;background:#ff6900;color:white;display:grid;place-items:center;font-size:26px;margin-bottom:24px}.mi-intro h2{font-size:26px;letter-spacing:-1px}.mi-intro p,.mi-intro li{color:#758197;line-height:1.9;font-size:14px}.mi-intro ol{padding-left:20px;margin:24px 0}.mi-intro small,.mi-footnote{font-size:12px;color:#8792a5;line-height:1.8}.mi-login{display:flex;flex-direction:column;gap:15px;max-width:360px;width:100%;justify-self:center}.mi-login label{font-size:13px}.mi-qr,.mi-qr-placeholder{border:1px solid #8792a533;border-radius:18px;min-height:310px;display:flex;flex-direction:column;align-items:center;justify-content:center;padding:20px;gap:12px}.mi-qr img{width:210px;height:210px;max-width:100%;background:white}.mi-qr span{font-size:12px;color:#8792a5}.mi-qr-placeholder>span{font-size:80px;line-height:1;color:#8792a5}.mi-qr-placeholder p{font-size:13px;color:#8792a5}.mi-account{display:flex;justify-content:space-between;align-items:center;margin-bottom:20px}.mi-account p,.mi-account small{color:#8792a5;font-size:12px}.mi-filters{display:grid;grid-template-columns:2fr 1fr;gap:16px;max-width:700px;margin:20px 0}.mi-device-grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(250px,1fr));gap:18px}.mi-device-top{display:flex;justify-content:space-between;align-items:center}.mi-device-icon{font-size:30px;color:#ff6900}.mi-device h3{margin-bottom:6px}.mi-device p{font-size:13px;color:#8792a5}.mi-device small{font-size:11px;color:#8792a5;display:block;margin-bottom:16px;overflow-wrap:anywhere}.mi-device .n-button{margin-top:10px}.mi-compatibility{line-height:1.7}.mi-advanced{display:flex;gap:12px;align-items:center;margin:28px 0;font-size:12px;color:#8792a5}.mi-property{padding:20px 0;border-bottom:1px solid #8792a533}.mi-property strong{font-size:14px}.mi-property p{font-size:13px;color:#8792a5}.mi-numeric{display:flex;gap:12px}.mi-numeric>.n-select,.mi-numeric>.n-input-number{flex:1}@media(max-width:1150px){.mi-welcome{gap:24px;padding:24px}.mi-intro h2{font-size:22px}}
</style>
