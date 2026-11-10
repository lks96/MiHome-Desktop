import { test, expect, type Page } from '@playwright/test'

// This fixture only exercises the UI/IPC boundary. Rust tests use real local
// HTTP/WebSocket servers and SQLite; no fixture data is shipped in the app.
async function desktopFixture(page: Page) {
  await page.addInitScript(() => {
    const w = window as any
    const callbacks: Record<number, (value: unknown) => void> = {}
    const events: Record<string, number> = {}
    let sequence = 0
    const entities = [
      { entityId: 'light.office', name: '书房灯', domain: 'light', state: 'off', unit: null, lastUpdated: '2026-11-10T01:00:00Z', attributes: { brightness: 128, supported_color_modes: ['brightness'] } },
      { entityId: 'switch.offline', name: '离线插座', domain: 'switch', state: 'unavailable', unit: null, lastUpdated: '2026-11-10T01:00:00Z', attributes: {} },
      { entityId: 'sensor.temperature', name: '温度传感器', domain: 'sensor', state: '22.5', unit: '°C', lastUpdated: '2026-11-10T01:00:00Z', attributes: {} },
    ]
    const preferences: Record<string, unknown> = {}
    const emit = (event: string, payload: unknown) => callbacks[events[event]]?.({ event, id: events[event], payload })
    w.__deviceTest = { calls: [], emit, entities }
    w.isTauri = true
    w.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} }
    w.__TAURI_INTERNALS__ = {
      transformCallback(callback: (value: unknown) => void) { callbacks[++sequence] = callback; return sequence },
      async invoke(command: string, args: any) {
        if (command === 'plugin:event|listen') { events[args.event] = args.handler; return args.handler }
        if (command === 'plugin:event|unlisten') return null
        const values: Record<string, unknown> = {
          settings_get: { theme: 'light', requestTimeout: 10, autoConnect: true, autoReconnect: true },
          desktop_get_status: {trayAvailable:true,autoStartSupported:true,autoStartEnabled:false,windowStateEnabled:true},
          settings_get_diagnostics: {version:'0.1.0',platform:'test-fixture',connectionState:'connected',retryAttempt:0,errorCode:null,entityCount:3,cameraCount:0,sceneCount:0},
          settings_get_app_info: { name: 'Mi Home Desktop', version: '0.1.0', platform: 'test-fixture' },
          ha_get_saved_connection: { id: 'primary', name: 'Test HA', baseUrl: 'http://127.0.0.1:8123/' },
          ha_get_connection_status: { state: 'connected', attempt: 0, error: null },
          ha_get_entities: entities,
          ha_get_catalog: { areas: [{ areaId: 'office', name: '书房' }, { areaId: 'living', name: '客厅' }], entityAreas: { 'light.office': 'office', 'sensor.temperature': 'living' }, services: { light: ['turn_on', 'turn_off'], switch: ['turn_on', 'turn_off'] }, servicesLoaded: true, warnings: [] },
          device_get_preferences: Object.values(preferences),
          camera_get_configuration: { gateway: { baseUrl: '', enabled: false }, profiles: [] },
        }
        if (command in values) return { ok: true, data: values[command] }
        if (command === 'device_update_preferences') { preferences[args.preference.entityId] = args.preference; return { ok: true, data: args.preference } }
        if (command === 'device_call_service') {
          w.__deviceTest.calls.push(args.request)
          return { ok: true, data: { accepted: true, message: 'HA 已处理服务请求；实际设备状态以实时同步为准' } }
        }
        throw new Error(`Unexpected fixture command: ${command}`)
      },
    }
  })
}

test('service acceptance does not change entity state before an HA event', async ({ page }) => {
  const errors: string[] = []
  page.on('pageerror', error => errors.push(error.message))
  await desktopFixture(page)
  await page.goto('/')
  await page.getByRole('button', { name: '设备中心', exact: true }).click()
  const light = page.locator('[data-entity-id="light.office"]')
  await expect(light.locator('.entity-state')).toHaveText('off')
  await light.getByRole('button', { name: '开启', exact: true }).click()
  await expect(light.getByRole('status')).toContainText('HA 已处理服务请求')
  await expect(light.locator('.entity-state')).toHaveText('off')
  await expect.poll(() => page.evaluate(() => (window as any).__deviceTest.calls)).toEqual([{ entityId: 'light.office', domain: 'light', service: 'turn_on', serviceData: {} }])
  await page.evaluate(() => {
    const fixture = (window as any).__deviceTest
    fixture.emit('device:state_changed', { ...fixture.entities[0], state: 'on', lastUpdated: '2026-11-10T01:00:01Z' })
  })
  await expect(light.locator('.entity-state')).toHaveText('on')
  await expect(page.locator('[data-entity-id="switch.offline"]').getByRole('button', { name: '开启', exact: true })).toBeDisabled()
  await light.getByRole('button', { name: '详情', exact: true }).click()
  await expect(page.getByRole('button', { name: '应用亮度', exact: true })).toBeVisible()
  await expect(page.getByRole('button', { name: '应用色温', exact: true })).toHaveCount(0)
  expect(errors).toEqual([])
})

test('favorites, local names, hidden recovery and search use client preferences', async ({ page }) => {
  await desktopFixture(page)
  await page.goto('/')
  await page.getByRole('button', { name: '设备中心', exact: true }).click()
  const light = page.locator('[data-entity-id="light.office"]')
  await light.getByRole('button', { name: '收藏', exact: true }).click()
  await expect(light.getByRole('button', { name: '取消收藏', exact: true })).toBeVisible()
  await light.getByRole('button', { name: '详情', exact: true }).click()
  await page.getByRole('textbox', { name: '自定义显示名称' }).fill('我的台灯')
  await page.getByRole('button', { name: '保存显示偏好', exact: true }).click()
  await expect(page.getByText('本地显示偏好已保存。')).toBeVisible()
  await page.getByRole('button', { name: '隐藏实体', exact: true }).click()
  // Dismiss drawer using Escape, then recover the hidden entity.
  await page.keyboard.press('Escape')
  await expect(light).toHaveCount(0)
  await page.getByRole('switch', { name: '显示隐藏' }).click()
  await expect(light.getByText('我的台灯', { exact: true })).toBeVisible()
  await page.getByRole('textbox', { name: '搜索实体' }).fill('温度')
  await expect(page.locator('[data-entity-id]')).toHaveCount(1)
  await expect(page.locator('[data-entity-id="sensor.temperature"]')).toBeVisible()
})

test('browser preview clearly disables saving credentials', async ({ page }) => {
  await page.goto('/')
  await page.getByRole('button', { name: '配置连接', exact: true }).click()
  await expect(page.getByRole('button', { name: '保存并连接', exact: true })).toBeDisabled()
  await expect(page.getByText('请在 Windows 桌面客户端中测试和保存连接，浏览器预览不会存储令牌。')).toBeVisible()
})
