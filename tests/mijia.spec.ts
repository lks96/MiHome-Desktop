import { test, expect, type Page } from '@playwright/test'
import { mkdtempSync, readFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, basename } from 'node:path'
import { execFileSync } from 'node:child_process'

// Test-only IPC. Account tokens and real QR authorization never enter these fixtures.
async function mijiaFixture(page: Page, rememberFailure = false, expiresIn = 180) {
  await page.addInitScript(({ rememberFailure, expiresIn }) => {
    const w = window as any
    const callbacks: Record<number, (value: unknown) => void> = {}
    const subscriptions: Record<string, number> = {}
    w.__mijiaEmit = (event: string, payload: unknown) => callbacks[subscriptions[event]!]?.({ event, payload })
    let sequence = 0
    const state = { authorized: false, canceled: [] as string[], writes: [] as unknown[], restoreCalls: 0, remoteDelay: false, remoteStartFinish: null as null | ((value: unknown) => void), remoteStarts: [] as string[], remoteStops: [] as string[], remote: { sessionId:'test-session',stage:'relay_request',failedStage:null,elapsedMs:1000,attempt:1,mediaPackets:0,keyframes:0,codec:null,playbackUrl:null,error:null }, cameraProbes: [] as string[], cameraProbeFinish: null as null | ((value: unknown) => void), finish: null as null | ((value: unknown) => void) }
    w.__mijiaTest = state
    const snapshot = () => ({ loggedIn: state.authorized, region: 'cn', accountLabel: state.authorized ? '小米账号 · 尾号 5678' : null, devices: state.authorized ? [
      { did: 'light-1', name: '书房灯', model: 'fixture.light.one', home: '我的家', room: '书房', online: true, camera: false },
      { did: 'camera-1', name: '门口摄像头', model: 'chuangmi.camera.039a01', home: '我的家', room: '门口', online: true, camera: true },
      { did: 'offline-1', name: '离线设备', model: 'fixture.switch.one', home: '我的家', room: '客厅', online: false, camera: false },
    ] : [], warnings: [], updatedAt: state.authorized ? 1_800_000_000 : null })
    const properties = [{ siid: 2, piid: 1, name: '灯 · 电源', format: 'bool', readable: true, writable: true, range: null, choices: [], value: false, unavailable: false }]
    w.isTauri = true
    w.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} }
    w.__TAURI_INTERNALS__ = {
      transformCallback(callback: (value: unknown) => void) { callbacks[++sequence] = callback; return sequence },
      async invoke(command: string, args: any) {
        if (command === 'plugin:event|listen') { subscriptions[args.event] = args.handler; return args.handler }
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
        if (command === 'mijia_camera_probe') { state.cameraProbes.push(args.did); return new Promise(resolve => { state.cameraProbeFinish = resolve }) }
        if (command === 'camera_remote_start') { state.remoteStarts.push(args.did); if (state.remoteDelay) return new Promise(resolve => { state.remoteStartFinish = resolve }); return {ok:true,data:{...state.remote}} }
        if (command === 'camera_remote_status') return {ok:true,data:{...state.remote}}
        if (command === 'camera_remote_stop') { state.remoteStops.push(args.sessionId);return {ok:true,data:true} }
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
  await expect(page.getByText('支持远程播放测试')).toBeVisible()
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

test('camera grant detection never claims remote playback or exposes cloud credentials', async ({ page }) => {
  await mijiaFixture(page); await page.goto('/'); await scan(page)
  await page.evaluate(() => (window as any).__mijiaTest.finish({ ok: true, data: { state: 'authorized' } }))
  await page.locator('[data-mi-device="camera-1"]').getByRole('button', { name: '查看状态与控制' }).click()
  await page.getByRole('button', { name: '检测视频授权', exact: true }).click()
  await expect.poll(() => page.evaluate(() => !!(window as any).__mijiaTest.cameraProbeFinish)).toBe(true)
  await expect(page.getByRole('button', { name: '检测视频授权', exact: true })).toBeDisabled()
  await page.evaluate(() => (window as any).__mijiaTest.cameraProbeFinish({ ok: true, data: {
    model: 'chuangmi.camera.039a01', transport: 'cs2', cloudAuthorizationGranted: true, remotePlaybackVerified: false, observedAt: 1800000000,
    token: 'private-fixture-token', sign: 'private-fixture-sign',
  } }))
  await expect(page.getByRole('alert').filter({ hasText: '云端视频授权已取得' })).toContainText('尚未建立远程视频通道')
  const report = page.locator('[data-camera-probe-result]')
  await expect(report).toContainText('"remotePlaybackVerified": false')
  await expect(report).not.toContainText('private-fixture-')
  await expect(page.locator('video')).toHaveCount(0)
  expect(await page.evaluate(() => (window as any).__mijiaTest.cameraProbes)).toEqual(['camera-1'])
})

test('camera probe response cannot leak into another drawer after it is closed', async ({ page }) => {
  await mijiaFixture(page); await page.goto('/'); await scan(page)
  await page.evaluate(() => (window as any).__mijiaTest.finish({ ok: true, data: { state: 'authorized' } }))
  await page.locator('[data-mi-device="camera-1"]').getByRole('button', { name: '查看状态与控制' }).click()
  await page.getByRole('button', { name: '检测视频授权', exact: true }).click()
  await expect.poll(() => page.evaluate(() => !!(window as any).__mijiaTest.cameraProbeFinish)).toBe(true)
  await page.keyboard.press('Escape')
  await page.locator('[data-mi-device="light-1"]').getByRole('button', { name: '查看状态与控制' }).click()
  await page.evaluate(() => (window as any).__mijiaTest.cameraProbeFinish({ ok: true, data: { model:'chuangmi.camera.039a01',transport:'cs2',cloudAuthorizationGranted:true,remotePlaybackVerified:false,observedAt:1800000000 } }))
  await expect(page.locator('[data-camera-probe-result]')).toHaveCount(0)
  await expect(page.getByRole('button', { name: '检测视频授权', exact: true })).toHaveCount(0)
})

test('remote camera waits for decoded frames and releases its session when the drawer closes', async ({ page }) => {
  await mijiaFixture(page); await page.goto('/'); await scan(page)
  await page.evaluate(() => (window as any).__mijiaTest.finish({ ok: true, data: { state: 'authorized' } }))
  await page.locator('[data-mi-device="camera-1"]').getByRole('button', { name: '查看状态与控制' }).click()
  await page.getByRole('button', { name: '远程播放', exact: true }).click()
  await expect(page.getByText('正在等待摄像头加入', { exact: true })).toBeVisible()
  await expect(page.getByRole('button', { name: '远程播放', exact: true })).toBeDisabled()
  await page.getByText('连接结果', { exact: true }).click()
  await expect(page.locator('[data-remote-camera-result]')).toContainText('"remotePlaybackVerified": false')
  await expect(page.getByText('正在播放', { exact: true })).toHaveCount(0)
  await expect(page.locator('video')).toHaveCount(0)
  await page.keyboard.press('Escape')
  await expect.poll(() => page.evaluate(() => (window as any).__mijiaTest.remoteStops)).toEqual(['test-session'])
  expect(await page.evaluate(() => (window as any).__mijiaTest.remoteStarts)).toEqual(['camera-1'])
})

test('remote failure preserves the failed stage and does not expose connection materials', async ({ page }) => {
  await mijiaFixture(page); await page.goto('/'); await scan(page)
  await page.evaluate(() => {
    const s = (window as any).__mijiaTest
    s.finish({ ok: true, data: { state: 'authorized' } })
    s.remote = { ...s.remote, stage: 'failed', failedStage:'session_linking', error:{code:'CS2_LINK_TIMEOUT',message:'中继就绪，但摄像头会话连接未完成',retryable:false},sign:'private-signature',init_string:'private-init' }
  })
  await page.locator('[data-mi-device="camera-1"]').getByRole('button', { name: '查看状态与控制' }).click()
  await page.getByRole('button', { name: '远程播放', exact: true }).click()
  await expect(page.getByText('中继就绪，但摄像头会话连接未完成', { exact: true })).toBeVisible()
  await page.getByText('连接结果', { exact: true }).click()
  await expect(page.locator('[data-remote-camera-result]')).toContainText('CS2_LINK_TIMEOUT')
  await expect(page.locator('[data-remote-camera-result]')).not.toContainText('private-')
  await expect(page.getByRole('button', { name:'远程播放', exact:true })).toBeEnabled()
})

test('remote HLS uses real decoded frames before reporting playback and stops on close', async ({ page }) => {
  const folder = mkdtempSync(join(tmpdir(), 'mihome-hls-test-'))
  try {
    execFileSync('ffmpeg', ['-hide_banner','-loglevel','error','-f','lavfi','-i','testsrc2=size=320x180:rate=20','-t','6','-c:v','libx264','-threads','1','-preset','ultrafast','-profile:v','baseline','-pix_fmt','yuv420p','-g','20','-sc_threshold','0','-f','hls','-hls_time','1','-hls_list_size','0','-hls_segment_filename',join(folder,'seg%06d.ts'),join(folder,'index.m3u8')])
    await page.route('http://127.0.0.1:18888/fixture/**', async route => {
      const name = basename(new URL(route.request().url()).pathname)
      await route.fulfill({status:200,body:readFileSync(join(folder,name)),contentType:name.endsWith('.m3u8')?'application/vnd.apple.mpegurl':'video/mp2t',headers:{'access-control-allow-origin':'http://127.0.0.1:1420'}})
    })
    await mijiaFixture(page); await page.goto('/'); await scan(page)
    await page.evaluate(() => {
      const s = (window as any).__mijiaTest
      s.finish({ok:true,data:{state:'authorized'}})
      s.remote={...s.remote,stage:'media_ready',mediaPackets:100,keyframes:5,codec:'H.264',playbackUrl:'http://127.0.0.1:18888/fixture/index.m3u8'}
    })
    await page.locator('[data-mi-device="camera-1"]').getByRole('button', { name:'查看状态与控制' }).click()
    await page.getByRole('button', {name:'远程播放',exact:true}).click()
    await expect(page.getByText('正在播放',{exact:true})).toBeVisible({timeout:15000})
    await expect.poll(() => page.locator('video').evaluate((v:HTMLVideoElement) => v.videoWidth)).toBe(320)
    await page.getByText('连接结果',{exact:true}).click()
    await expect(page.locator('[data-remote-camera-result]')).toContainText('"remotePlaybackVerified": true')
    await expect(page.locator('[data-remote-camera-result]')).not.toContainText('18888')
    await page.getByRole('button',{name:'停止播放',exact:true}).click()
    await expect.poll(() => page.evaluate(() => (window as any).__mijiaTest.remoteStops)).toEqual(['test-session'])
    await expect(page.getByText('正在播放',{exact:true})).toHaveCount(0)
  } finally { rmSync(folder,{recursive:true,force:true}) }
})

test('closing during remote start releases a late session without reopening the drawer', async ({ page }) => {
  await mijiaFixture(page); await page.goto('/'); await scan(page)
  await page.evaluate(() => { const s=(window as any).__mijiaTest; s.remoteDelay=true; s.finish({ok:true,data:{state:'authorized'}}) })
  await page.locator('[data-mi-device="camera-1"]').getByRole('button', {name:'查看状态与控制'}).click()
  await page.getByRole('button',{name:'远程播放',exact:true}).click()
  await expect.poll(() => page.evaluate(() => !!(window as any).__mijiaTest.remoteStartFinish)).toBe(true)
  await page.keyboard.press('Escape')
  await page.locator('[data-mi-device="light-1"]').getByRole('button', {name:'查看状态与控制'}).click()
  await page.evaluate(() => {const s=(window as any).__mijiaTest;s.remoteStartFinish({ok:true,data:s.remote})})
  await expect.poll(() => page.evaluate(() => (window as any).__mijiaTest.remoteStops)).toEqual(['test-session'])
  await expect(page.getByRole('button',{name:'远程播放',exact:true})).toHaveCount(0)
  await expect(page.locator('video')).toHaveCount(0)
})

test('hidden desktop stops remote video and prevents automatic reopening', async ({ page }) => {
  await mijiaFixture(page); await page.goto('/'); await scan(page)
  await page.evaluate(() => (window as any).__mijiaTest.finish({ok:true,data:{state:'authorized'}}))
  await page.locator('[data-mi-device="camera-1"]').getByRole('button', {name:'查看状态与控制'}).click()
  await page.getByRole('button',{name:'远程播放',exact:true}).click()
  await expect(page.getByText('正在等待摄像头加入',{exact:true})).toBeVisible()
  await page.evaluate(() => (window as any).__mijiaEmit('app:window_visible',false))
  await expect.poll(() => page.evaluate(() => (window as any).__mijiaTest.remoteStops)).toEqual(['test-session'])
  await expect(page.getByRole('button',{name:'远程播放',exact:true})).toBeDisabled()
  await page.evaluate(() => (window as any).__mijiaEmit('app:window_visible',true))
  await expect(page.getByRole('button',{name:'远程播放',exact:true})).toBeEnabled()
  expect(await page.evaluate(() => (window as any).__mijiaTest.remoteStarts)).toEqual(['camera-1'])
})

test('auth diagnostics expose only structure and deterministic parse failures do not auto-retry', async ({ page }) => {
  await page.clock.install()
  await mijiaFixture(page); await page.goto('/'); await scan(page)
  await page.evaluate(() => {
    const s=(window as any).__mijiaTest
    s.finish({ok:true,data:{state:'authorized'}})
    s.remote={...s.remote,stage:'failed',failedStage:'miss_authenticating',error:{code:'MISS_AUTH_JSON_INVALID',message:'摄像头认证响应格式暂不兼容，请复制连接结果',retryable:false},
      authDiagnostics:{messages:1,messageKind:'auth_response',commandByteOrder:'le',bodyBytes:128,trailingPaddingBytes:2,encrypted:false,jsonShape:'object',jsonErrorCategory:'syntax',rawResponse:'private-auth-response',sign:'private-signature'},
      transportDiagnostics:{linkRequests:2,linkNonRequests:1,commandMessages:1,mediaMessages:0,streamErrorChannel:null,relayToken:'private-relay-token'},
    }
  })
  await page.locator('[data-mi-device="camera-1"]').getByRole('button',{name:'查看状态与控制'}).click()
  await page.getByRole('button',{name:'远程播放',exact:true}).click()
  await expect(page.getByText('摄像头认证响应格式暂不兼容，请复制连接结果',{exact:true})).toBeVisible()
  await page.getByText('连接结果',{exact:true}).click()
  const report=page.locator('[data-remote-camera-result]')
  await expect(report).toContainText('"diagnosticSchema": 3')
  await expect(report).toContainText('"clientVersion": "0.3.1"')
  await expect(report).toContainText('"commandByteOrder": "le"')
  await expect(report).toContainText('"linkNonRequests": 1')
  await expect(report).not.toContainText('private-')
  await page.clock.runFor(15000)
  expect(await page.evaluate(() => (window as any).__mijiaTest.remoteStarts)).toEqual(['camera-1'])
})
