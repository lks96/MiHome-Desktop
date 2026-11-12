import { test, expect, type Page } from '@playwright/test'

// Test-only IPC. Account tokens and real QR authorization never enter these fixtures.
async function mijiaFixture(page: Page, rememberFailure = false, expiresIn = 180) {
  await page.addInitScript(({ rememberFailure, expiresIn }) => {
    const w = window as any
    const callbacks: Record<number, (value: unknown) => void> = {}
    let sequence = 0
    const state = { authorized: false, canceled: [] as string[], writes: [] as unknown[], restoreCalls: 0, finish: null as null | ((value: unknown) => void) }
    w.__mijiaTest = state
    const snapshot = () => ({ loggedIn: state.authorized, region: 'cn', accountLabel: state.authorized ? '小米账号 · 尾号 5678' : null, devices: state.authorized ? [
      { did: 'light-1', name: '书房灯', model: 'fixture.light.one', home: '我的家', room: '书房', online: true, camera: false },
      { did: 'camera-1', name: '门口摄像头', model: 'fixture.camera.one', home: '我的家', room: '门口', online: true, camera: true },
      { did: 'offline-1', name: '离线设备', model: 'fixture.switch.one', home: '我的家', room: '客厅', online: false, camera: false },
    ] : [], warnings: [], updatedAt: state.authorized ? 1_800_000_000 : null })
    const properties = [{ siid: 2, piid: 1, name: '灯 · 电源', format: 'bool', readable: true, writable: true, range: null, choices: [], value: false, unavailable: false }]
    w.isTauri = true
    w.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} }
    w.__TAURI_INTERNALS__ = {
      transformCallback(callback: (value: unknown) => void) { callbacks[++sequence] = callback; return sequence },
      async invoke(command: string, args: any) {
        if (command === 'plugin:event|listen') return args.handler
        if (command === 'plugin:event|unlisten') return null
        if (command === 'mijia_restore') { state.restoreCalls++; return { ok: true, data: snapshot() } }
        if (command === 'mijia_start_login') return { ok: true, data: { id: 'fixture-qr', expiresIn, image: 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+j9WQAAAAASUVORK5CYII=' } }
        if (command === 'mijia_poll_login') return new Promise(resolve => { state.finish = value => {
          if (rememberFailure) { resolve({ ok: false, error: { code: 'CREDENTIAL_STORE_FAILED', message: '无法访问 Windows 凭据管理器，请检查系统凭据服务', retryable: false } }); return }
          if (!state.canceled.includes(args.id)) state.authorized = (value as any).data.state === 'authorized'
          resolve(value)
        } })
        if (command === 'mijia_cancel_login') { state.canceled.push(args.id); return { ok: true, data: true } }
        if (command === 'mijia_sync') return { ok: true, data: snapshot() }
        if (command === 'mijia_logout') { state.authorized = false; return { ok: true, data: true } }
        if (command === 'mijia_get_properties') return { ok: true, data: properties }
        if (command === 'mijia_set_property') { state.writes.push(args); return { ok: true, data: properties } }
        const values: Record<string, unknown> = {
          settings_get: { theme: 'light', requestTimeout: 10, autoConnect: true, autoReconnect: true, closeToTray: false, autoStart: false },
          settings_get_app_info: { name: 'Mi Home Desktop', version: '0.1.0', platform: 'test-fixture' },
          desktop_get_status: { trayAvailable: false, autoStartSupported: false, autoStartEnabled: false, windowStateEnabled: false },
          ha_get_saved_connection: null, ha_get_connection_status: { state: 'disconnected', attempt: 0, error: null }, ha_get_entities: [],
          ha_get_catalog: { areas: [], services: {}, entityAreas: {}, warnings: [], servicesLoaded: false }, device_get_preferences: [],
        }
        if (command in values) return { ok: true, data: values[command] }
        throw new Error(`Unexpected command ${command}`)
      },
    }
  }, { rememberFailure, expiresIn })
}
async function scan(page: Page) {
  await page.getByRole('button', { name: '用米家 App 扫码登录', exact: true }).click()
  await expect(page.getByAltText('米家登录二维码')).toBeVisible()
  await expect.poll(() => page.evaluate(() => !!(window as any).__mijiaTest.finish)).toBe(true)
}

test('first launch leads with Mijia QR and keeps HA in advanced settings', async ({ page }) => {
  await page.goto('/')
  await expect(page.getByRole('heading', { name: '我的米家', exact: true })).toBeVisible()
  await expect(page.getByRole('button', { name: '用米家 App 扫码登录', exact: true })).toBeDisabled()
  await expect(page.getByRole('button', { name: '配置连接', exact: true })).toHaveCount(0)
  await page.screenshot({ path: '/workspace/.environment/mijia-first-launch.png', fullPage: true })
  await page.getByRole('button', { name: 'Home Assistant（高级）', exact: true }).click()
  await expect(page.getByRole('button', { name: '保存并连接', exact: true })).toBeDisabled()
  await page.getByRole('button', { name: '返回米家扫码接入', exact: true }).click()
  await expect(page.getByRole('heading', { name: '我的米家', exact: true })).toBeVisible()
})

test('QR confirmation discovers rooms and controls only report read-back state', async ({ page }) => {
  await mijiaFixture(page); await page.goto('/'); await scan(page)
  await page.evaluate(() => (window as any).__mijiaTest.finish({ ok: true, data: { state: 'authorized' } }))
  await expect(page.locator('[data-mi-device]')).toHaveCount(3)
  const light = page.locator('[data-mi-device="light-1"]')
  await expect(light.getByText('我的家 书房')).toBeVisible()
  await expect(page.locator('[data-mi-device="offline-1"]').getByRole('button')).toBeDisabled()
  await page.screenshot({ path: '/workspace/.environment/mijia-devices.png', fullPage: true })
  await light.getByRole('button', { name: '查看状态与控制' }).click()
  await expect(page.getByText('灯 · 电源', { exact: true })).toBeVisible()
  await page.getByRole('button', { name: '开启', exact: true }).click()
  await expect(page.getByRole('status')).toContainText('已读取设备报告的状态')
  await expect(page.getByText('关闭', { exact: true }).first()).toBeVisible()
  expect(await page.evaluate(() => (window as any).__mijiaTest.writes)).toEqual([{ did: 'light-1', siid: 2, piid: 1, value: true }])
  await page.keyboard.press('Escape')
  await page.getByRole('button', { name: '监控中心', exact: true }).click()
  await expect(page.locator('[data-mi-device]')).toHaveCount(1)
  await expect(page.getByText('已识别摄像头 · 此型号直播待适配')).toBeVisible()
  await page.getByRole('button', { name: '退出账号', exact: true }).click()
  await expect(page.getByRole('button', { name: '用米家 App 扫码登录', exact: true })).toBeVisible()
  await expect(page.locator('[data-mi-device]')).toHaveCount(0)
})

test('leaving QR login cancels it and a late success cannot reopen the account', async ({ page }) => {
  await mijiaFixture(page); await page.goto('/'); await scan(page)
  await page.getByRole('button', { name: '取消登录', exact: true }).click()
  await page.evaluate(() => (window as any).__mijiaTest.finish({ ok: true, data: { state: 'authorized' } }))
  await expect(page.getByAltText('米家登录二维码')).toHaveCount(0)
  await expect(page.locator('[data-mi-device]')).toHaveCount(0)
  expect(await page.evaluate(() => (window as any).__mijiaTest.restoreCalls)).toBe(1)
})

test('credential failures do not appear as a successful remembered login', async ({ page }) => {
  await mijiaFixture(page, true); await page.goto('/'); await scan(page)
  await page.evaluate(() => (window as any).__mijiaTest.finish({ ok: true, data: { state: 'authorized' } }))
  await expect(page.getByRole('alert')).toContainText('无法访问 Windows 凭据管理器')
  await expect(page.locator('[data-mi-device]')).toHaveCount(0)
  await expect(page.getByRole('button', { name: '重新获取二维码', exact: true })).toBeVisible()
})

test('QR expires while a poll is pending and late confirmation stays canceled', async ({ page }) => {
  await page.clock.install()
  await mijiaFixture(page, false, 1); await page.goto('/'); await scan(page)
  await page.clock.runFor(1200)
  await expect(page.getByAltText('米家登录二维码')).toHaveCount(0)
  await expect(page.getByRole('alert')).toContainText('二维码已过期')
  await page.evaluate(() => (window as any).__mijiaTest.finish({ ok: true, data: { state: 'authorized' } }))
  await expect(page.locator('[data-mi-device]')).toHaveCount(0)
  expect(await page.evaluate(() => (window as any).__mijiaTest.restoreCalls)).toBe(1)
})
