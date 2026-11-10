<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { NButton, NInputNumber, NSelect, NColorPicker } from 'naive-ui'
import type { EntitySummary } from '../types/connection'
import { useDevicesStore } from '../stores/devices'
import { numberAttribute as num, modes, feature, brightnessSupported, colorSupported } from '../utils/capabilities'
const props = defineProps<{ entity: EntitySummary; compact?: boolean }>()
const devices = useDevicesStore()
const supportedSwitch = computed(() => ['light','switch','fan'].includes(props.entity.domain))
const disabled = computed(() => !devices.canControl(props.entity))
const has = (service: string) => devices.hasService(props.entity, service)
const brightness = ref<number | null>(null)
const kelvin = ref<number | null>(null)
const temperature = ref<number | null>(null)
const percentage = ref<number | null>(null)
const hvacMode = ref<string | null>(null)
const fanMode = ref<string | null>(null)
const presetMode = ref<string | null>(null)
const color = ref<string | null>(null)
watch(() => [props.entity.entityId, props.entity.attributes.brightness], () => { brightness.value = num(props.entity, 'brightness') }, { immediate: true })
watch(() => [props.entity.entityId, props.entity.attributes.color_temp_kelvin], () => { kelvin.value = num(props.entity, 'color_temp_kelvin') }, { immediate: true })
watch(() => [props.entity.entityId, props.entity.attributes.temperature], () => { temperature.value = num(props.entity, 'temperature') }, { immediate: true })
watch(() => [props.entity.entityId, props.entity.attributes.percentage], () => { percentage.value = num(props.entity, 'percentage') }, { immediate: true })
watch(() => [props.entity.entityId, props.entity.state], () => { hvacMode.value = modes(props.entity, 'hvac_modes').includes(props.entity.state) ? props.entity.state : null }, { immediate: true })
watch(() => [props.entity.entityId, props.entity.attributes.fan_mode], () => { fanMode.value = typeof props.entity.attributes.fan_mode === 'string' ? props.entity.attributes.fan_mode : null }, { immediate: true })
watch(() => [props.entity.entityId, props.entity.attributes.preset_mode], () => { presetMode.value = typeof props.entity.attributes.preset_mode === 'string' ? props.entity.attributes.preset_mode : null }, { immediate: true })
watch(() => props.entity.entityId, () => { color.value = null })
const options = (key: string) => modes(props.entity, key).map(value => ({ label: value, value }))
function applyColor() {
  if (!color.value || !/^#[0-9a-f]{6}$/i.test(color.value)) return
  const rgb = [1,3,5].map(index => parseInt(color.value!.slice(index,index+2),16))
  void devices.action(props.entity, 'turn_on', { rgb_color: rgb })
}
const temperatureSupported = computed(() => feature(props.entity, 1) && 'temperature' in props.entity.attributes && num(props.entity, 'min_temp') !== null && num(props.entity, 'max_temp') !== null)
</script>
<template>
  <div class="entity-controls">
    <div v-if="supportedSwitch" class="switch-actions">
      <NButton v-if="has('turn_on')" size="small" :disabled="disabled" @click.stop="devices.action(entity, 'turn_on')">开启</NButton>
      <NButton v-if="has('turn_off')" size="small" :disabled="disabled" @click.stop="devices.action(entity, 'turn_off')">关闭</NButton>
    </div>
    <template v-if="!compact">
      <template v-if="entity.domain === 'light' && has('turn_on')">
        <div v-if="brightnessSupported(entity)" class="control-row"><label>亮度（0–255）</label><NInputNumber v-model:value="brightness" :min="0" :max="255" :precision="0" :disabled="disabled" /><NButton :disabled="disabled || brightness === null" @click="devices.action(entity, 'turn_on', { brightness })">应用亮度</NButton></div>
        <div v-if="modes(entity, 'supported_color_modes').includes('color_temp') && num(entity, 'min_color_temp_kelvin') !== null && num(entity, 'max_color_temp_kelvin') !== null" class="control-row"><label>色温（K）</label><NInputNumber v-model:value="kelvin" :min="num(entity, 'min_color_temp_kelvin')!" :max="num(entity, 'max_color_temp_kelvin')!" :precision="0" :disabled="disabled" /><NButton :disabled="disabled || kelvin === null" @click="devices.action(entity, 'turn_on', { color_temp_kelvin: kelvin })">应用色温</NButton></div>
        <div v-if="colorSupported(entity)" class="control-row"><label>设置颜色</label><NColorPicker v-model:value="color" :modes="['hex']" :show-alpha="false" :disabled="disabled" /><NButton :disabled="disabled || !color" @click="applyColor">应用颜色</NButton></div>
      </template>
      <template v-if="entity.domain === 'climate'">
        <div v-if="has('set_temperature') && temperatureSupported" class="control-row"><label>目标温度</label><NInputNumber v-model:value="temperature" :min="num(entity,'min_temp')!" :max="num(entity,'max_temp')!" :step="num(entity,'target_temp_step') ?? 1" :disabled="disabled" /><NButton :disabled="disabled || temperature === null" @click="devices.action(entity, 'set_temperature', { temperature })">设置温度</NButton></div>
        <div v-if="has('set_hvac_mode') && modes(entity,'hvac_modes').length" class="control-row"><label>运行模式</label><NSelect v-model:value="hvacMode" :options="options('hvac_modes')" :disabled="disabled" /><NButton :disabled="disabled || !hvacMode" @click="devices.action(entity, 'set_hvac_mode', { hvac_mode: hvacMode })">设置模式</NButton></div>
        <div v-if="has('set_fan_mode') && feature(entity,8) && modes(entity,'fan_modes').length" class="control-row"><label>风速模式</label><NSelect v-model:value="fanMode" :options="options('fan_modes')" :disabled="disabled" /><NButton :disabled="disabled || !fanMode" @click="devices.action(entity, 'set_fan_mode', { fan_mode: fanMode })">设置风速</NButton></div>
      </template>
      <template v-if="entity.domain === 'fan'">
        <div v-if="has('set_percentage') && feature(entity,1) && 'percentage' in entity.attributes" class="control-row"><label>风速百分比</label><NInputNumber v-model:value="percentage" :min="0" :max="100" :step="num(entity,'percentage_step') ?? 1" :disabled="disabled" /><NButton :disabled="disabled || percentage === null" @click="devices.action(entity, 'set_percentage', { percentage })">设置风速</NButton></div>
        <div v-if="has('set_preset_mode') && feature(entity,8) && modes(entity,'preset_modes').length" class="control-row"><label>预设模式</label><NSelect v-model:value="presetMode" :options="options('preset_modes')" :disabled="disabled" /><NButton :disabled="disabled || !presetMode" @click="devices.action(entity, 'set_preset_mode', { preset_mode: presetMode })">设置预设</NButton></div>
      </template>
      <p v-if="!['light','switch','climate','fan'].includes(entity.domain)" class="control-note">此类实体当前支持状态查看。</p>
      <p v-else class="control-note">仅显示 HA 已声明的服务和能力；操作后的设备状态由实时同步确认。</p>
    </template>
    <p v-if="devices.pending[entity.entityId]" class="control-note" role="status">正在调用服务…</p>
    <p v-if="devices.feedback[entity.entityId]" class="control-feedback" :class="{ failed: !devices.feedback[entity.entityId].success }" role="status">{{ devices.feedback[entity.entityId].message }}</p>
  </div>
</template>
<style scoped>
.switch-actions{display:flex;gap:8px;margin:10px 0}.control-row{display:grid;grid-template-columns:110px minmax(90px,1fr) auto;gap:10px;align-items:center;margin:16px 0}.control-row label{font-size:12px;color:#8792a5}.control-note,.control-feedback{font-size:11px;line-height:1.7;color:#8792a5;overflow-wrap:anywhere}.control-feedback{color:#22a06b}.control-feedback.failed{color:#e05252}
</style>
