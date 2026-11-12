import { ref } from 'vue'
import { defineStore } from 'pinia'
import { isTauri } from '@tauri-apps/api/core'
import { call } from '../utils/ipc'
import type { AppError } from '../types/settings'
import type { MiSnapshot, QrLogin, MiProperty } from '../types/mijia'

const empty = (): MiSnapshot => ({ loggedIn: false, region: 'cn', accountLabel: null, devices: [], warnings: [], updatedAt: null })
export const useMijiaStore = defineStore('mijia', () => {
  const snapshot = ref<MiSnapshot>(empty())
  const qr = ref<QrLogin | null>(null)
  const deadline = ref(0)
  const error = ref<AppError | null>(null)
  const ready = ref(false)
  const loading = ref(false)
  const syncing = ref(false)
  const loggingIn = ref(false)
  const properties = ref<MiProperty[]>([])
  const propertiesFor = ref('')
  const propertiesLoading = ref(false)
  const propertiesError = ref<AppError | null>(null)
  const controlling = ref(false)
  const feedback = ref('')
  let timer: ReturnType<typeof setTimeout> | undefined
  let expiryTimer: ReturnType<typeof setTimeout> | undefined
  let generation = 0
  let detailGeneration = 0
  let revision = 0
  async function initialize(force = false) {
    if (loading.value || ready.value && !force) return
    if (!isTauri()) { ready.value = true; return }
    loading.value = true; error.value = null
    const current = revision
    const result = await call<MiSnapshot>('mijia_restore')
    if (current === revision) {
      if (result.ok) snapshot.value = result.data; else error.value = result.error
      ready.value = result.ok
    }
    loading.value = false
    if (result.ok && result.data.loggedIn && current === revision) await sync()
  }
  async function sync() {
    if (syncing.value || !snapshot.value.loggedIn) return
    syncing.value = true; error.value = null
    const current = revision
    const result = await call<MiSnapshot>('mijia_sync')
    if (current === revision) {
      if (result.ok) snapshot.value = result.data
      else {
        error.value = result.error
        if (result.error.code === 'MIJIA_AUTH_EXPIRED') { snapshot.value = empty(); clearProperties() }
      }
    }
    syncing.value = false
  }
  async function cancelLogin() {
    generation++
    clearTimeout(timer)
    clearTimeout(expiryTimer)
    const id = qr.value?.id
    qr.value = null; loggingIn.value = false
    if (id) await call<boolean>('mijia_cancel_login', { id })
  }
  async function endLogin(failure: AppError) {
    const canceledGeneration = generation + 1
    await cancelLogin()
    if (generation === canceledGeneration) error.value = failure
  }
  async function poll(id: string, current: number) {
    if (current !== generation) return
    if (Date.now() >= deadline.value) {
      await endLogin({ code: 'MIJIA_QR_EXPIRED', message: '二维码已过期，请重新获取', retryable: true }); return
    }
    const result = await call<{ state: 'waiting' | 'authorized' | 'expired' }>('mijia_poll_login', { id })
    if (current !== generation) return
    if (!result.ok || result.data.state === 'expired') {
      await endLogin(result.ok ? { code: 'MIJIA_QR_EXPIRED', message: '二维码已过期，请重新获取', retryable: true } : result.error)
      return
    }
    if (result.data.state === 'authorized') {
      clearTimeout(expiryTimer)
      qr.value = null; loggingIn.value = false; revision++
      await initialize(true)
    } else timer = setTimeout(() => { void poll(id, current) }, 700)
  }
  async function startLogin(region: string, remember: boolean) {
    if (loggingIn.value) return
    await cancelLogin()
    const current = generation
    loggingIn.value = true; error.value = null
    const result = await call<QrLogin>('mijia_start_login', { region, remember })
    if (current !== generation) {
      if (result.ok) await call<boolean>('mijia_cancel_login', { id: result.data.id })
      return
    }
    if (!result.ok) { error.value = result.error; loggingIn.value = false; return }
    qr.value = result.data; deadline.value = Date.now() + result.data.expiresIn * 1000
    expiryTimer = setTimeout(() => {
      if (current === generation) void endLogin({ code: 'MIJIA_QR_EXPIRED', message: '二维码已过期，请重新获取', retryable: true })
    }, result.data.expiresIn * 1000)
    void poll(result.data.id, current)
  }
  async function logout() {
    revision++; await cancelLogin()
    const result = await call<boolean>('mijia_logout')
    if (result.ok) { snapshot.value = empty(); error.value = null; clearProperties() }
    else error.value = result.error
  }
  function clearProperties() { detailGeneration++; properties.value = []; propertiesFor.value = ''; propertiesError.value = null; feedback.value = ''; propertiesLoading.value = false }
  async function loadProperties(did: string) {
    clearProperties()
    const current = detailGeneration
    propertiesFor.value = did; propertiesLoading.value = true
    const result = await call<MiProperty[]>('mijia_get_properties', { did })
    if (current !== detailGeneration) return
    if (result.ok) properties.value = result.data; else propertiesError.value = result.error
    propertiesLoading.value = false
  }
  async function setProperty(property: MiProperty, value: unknown) {
    if (controlling.value || !propertiesFor.value) return
    const current = detailGeneration
    const did = propertiesFor.value
    controlling.value = true; propertiesError.value = null; feedback.value = ''
    const result = await call<MiProperty[]>('mijia_set_property', { did, siid: property.siid, piid: property.piid, value })
    if (current === detailGeneration) {
      if (result.ok) { properties.value = result.data; feedback.value = '指令已接受，已读取设备报告的状态。' }
      else propertiesError.value = result.error
    }
    controlling.value = false
  }
  return { snapshot, qr, deadline, error, ready, loading, syncing, loggingIn, properties, propertiesFor, propertiesLoading, propertiesError, controlling, feedback,
    initialize, sync, startLogin, cancelLogin, logout, loadProperties, clearProperties, setProperty }
})
