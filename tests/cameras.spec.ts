import { test, expect, type Page } from '@playwright/test'

async function cameraFixture(page: Page, snapshot = false, delayedStart = false, realPeer = false, localFrames = false) {
  await page.addInitScript(({ snapshot, delayedStart, realPeer, localFrames }) => {
    const w = window as any
    let sequence = 0
    const events:Record<string,number>={}
    const callbacks: Record<number, (value: unknown) => void> = {}
    const entities = [1,2,3,4].map(id => ({ entityId:`camera.test${id}`,name:`摄像头${id}`,domain:'camera',state:'idle',unit:null,lastUpdated:'2026-11-10T01:00:00Z',attributes:{supported_features:2} }))
    const profiles = entities.map((entity,index) => ({entityId:entity.entityId,sourceType:snapshot?'snapshot':'go2rtc',streamName:snapshot?null:`cam${index+1}`,enabled:true}))
    const NativePeer = window.RTCPeerConnection
    const senders = new Map<string, { peer: RTCPeerConnection; stream: MediaStream; timer: number }>()
    const stats = { starts: [] as string[], stops: [] as string[], opened:0, closed:0, snapshots:0, offers:0 }
    w.__cameraTest = stats
    w.__cameraEmit=(event:string,payload:unknown)=>callbacks[events[event]]?.({event,id:events[event],payload})
    w.isTauri = true
    w.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} }
    w.__TAURI_INTERNALS__ = {
      transformCallback(callback: (value: unknown) => void) { callbacks[++sequence]=callback;return sequence },
      async invoke(command: string, args: any) {
        if (command === 'plugin:event|listen') {events[args.event]=args.handler;return args.handler}
        if (command === 'plugin:event|unlisten') return null
        const values: Record<string, unknown> = {
          settings_get:{theme:'light',requestTimeout:10,autoConnect:true,autoReconnect:true},
          desktop_get_status: {trayAvailable:true,autoStartSupported:true,autoStartEnabled:false,windowStateEnabled:true},
          settings_get_diagnostics: {version:'0.1.0',platform:'test-fixture',connectionState:'connected',retryAttempt:0,errorCode:null,entityCount:3,cameraCount:0,sceneCount:0},
          settings_get_app_info:{name:'Mi Home Desktop',version:'0.1.0',platform:'fixture'},
          ha_get_saved_connection:{id:'primary',name:'Test HA',baseUrl:'http://127.0.0.1:8123/'},
          ha_get_connection_status:{state:'connected',attempt:0,error:null},ha_get_entities:entities,
          ha_get_catalog:{areas:[],services:{},entityAreas:{},warnings:[],servicesLoaded:true},device_get_preferences:[],
          camera_get_configuration:{gateway:{baseUrl:'http://127.0.0.1:1984/',enabled:!snapshot},profiles},
        }
        if (command in values) return {ok:true,data:values[command]}
        if (command === 'camera_start_session') {stats.starts.push(args.entityId);const id=`session-${stats.starts.length}`;if(delayedStart)await new Promise(resolve=>setTimeout(resolve,500));return {ok:true,data:{sessionId:id,entityId:args.entityId}}}
        if (command === 'camera_stop_session') {stats.stops.push(args.sessionId);const sender=senders.get(args.sessionId);if(sender){sender.peer.close();sender.stream.getTracks().forEach(track=>track.stop());clearInterval(sender.timer);senders.delete(args.sessionId)}return {ok:true,data:true}}
        if (command === 'camera_keep_session') return {ok:true,data:true}
        if (command === 'camera_exchange_sdp') {
          stats.offers++
          if (!realPeer) return {ok:true,data:'v=0\r\nm=video 9 UDP/TLS/RTP/SAVPF 96\r\n'}
          const peer=new NativePeer({iceServers:[]})
          w.__cameraSender=peer
          const canvas=document.createElement('canvas');canvas.width=320;canvas.height=180
          const context=canvas.getContext('2d')!
          let frame=0
          const draw=()=>{context.fillStyle=frame++%2?'#3478f6':'#22a06b';context.fillRect(0,0,320,180)}
          draw()
          const stream=canvas.captureStream(10)
          const timer=window.setInterval(draw,100)
          senders.set(args.sessionId,{peer,stream,timer})
          stream.getTracks().forEach(track=>peer.addTrack(track,stream))
          w.__cameraStage='setRemote';await peer.setRemoteDescription({type:'offer',sdp:args.offer});w.__cameraStage='createAnswer'
          const answer=await peer.createAnswer();w.__cameraStage='setLocal';await peer.setLocalDescription(answer);w.__cameraStage='gather'
          if (peer.iceGatheringState!=='complete') await new Promise<void>((resolve,reject)=>{
            const timeout=window.setTimeout(()=>reject(new Error('Fixture ICE timeout')),8000)
            peer.addEventListener('icegatheringstatechange',()=>{if(peer.iceGatheringState==='complete'){clearTimeout(timeout);resolve()}})
          })
          w.__cameraStage='answered';return {ok:true,data:peer.localDescription!.sdp}
        }
        if (command === 'camera_get_snapshot') {stats.snapshots++;return {ok:true,data:{dataUrl:'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jG0kAAAAASUVORK5CYII='}}}
        throw new Error(`Unexpected camera fixture command: ${command}`)
      },
    }
    class FakePeer extends EventTarget {
      iceGatheringState='complete'
      connectionState='new'
      localDescription: unknown = null
      ontrack: ((event: {track: MediaStreamTrack})=>void) | null = null
      timer: number | null = null
      source: MediaStream | null = null
      onconnectionstatechange: (()=>void)|null = null
      constructor() {super();stats.opened++}
      addTransceiver() {return {setCodecPreferences() {}}}
      async createOffer() {return {type:'offer',sdp:'v=0\r\nm=video 9 UDP/TLS/RTP/SAVPF 96\r\n'}}
      async setLocalDescription(value: unknown) {this.localDescription=value}
      async setRemoteDescription() {
        this.connectionState='connected';this.onconnectionstatechange?.()
        if (localFrames) {
          const canvas=document.createElement('canvas');canvas.width=320;canvas.height=180
          const context=canvas.getContext('2d')!;let frame=0
          const draw=()=>{context.fillStyle=frame++%2?'#3478f6':'#22a06b';context.fillRect(0,0,320,180)}
          draw();this.source=canvas.captureStream(10);this.timer=window.setInterval(draw,100)
          this.ontrack?.({track:this.source.getVideoTracks()[0]!})
        }
      }
      close() {if(this.connectionState!=='closed')stats.closed++;this.connectionState='closed';if(this.timer)clearInterval(this.timer);this.source?.getTracks().forEach(track=>track.stop())}
    }
    if (!realPeer) w.RTCPeerConnection = FakePeer
    else w.RTCPeerConnection = class extends NativePeer { constructor(config: RTCConfiguration) { super(config); w.__cameraReceiver=this } }
  }, { snapshot, delayedStart, realPeer, localFrames })
}

test('single/four layout reuses visible peer and releases hidden/unmounted sessions', async ({ page }) => {
  const errors: string[]=[]
  page.on('pageerror', error=>errors.push(error.message))
  await cameraFixture(page)
  await page.goto('/')
  await page.getByRole('button',{name:'监控中心',exact:true}).click()
  await expect.poll(()=>page.evaluate(()=>(window as any).__cameraTest.offers)).toBe(1)
  await expect(page.locator('[data-camera-id]')).toHaveCount(1)
  // Fake SDP/connected peer does not contain decoded frames and must not claim playback.
  await expect(page.getByText('正在播放',{exact:true})).toHaveCount(0)
  await page.getByRole('button',{name:'四宫格',exact:true}).click()
  await expect.poll(()=>page.evaluate(()=>(window as any).__cameraTest.offers)).toBe(4)
  await expect.poll(()=>page.evaluate(()=>(window as any).__cameraTest.starts.length)).toBe(4)
  await expect(page.locator('[data-camera-id]')).toHaveCount(4)
  await page.getByRole('button',{name:'单画面',exact:true}).click()
  await expect.poll(()=>page.evaluate(()=>(window as any).__cameraTest.stops.length)).toBe(3)
  await page.getByRole('button',{name:'设备中心',exact:true}).click()
  await expect.poll(()=>page.evaluate(()=>(window as any).__cameraTest.stops.length)).toBe(4)
  await expect.poll(()=>page.evaluate(()=>(window as any).__cameraTest.closed)).toBe(4)
  expect(errors).toEqual([])
})

test('snapshot is explicit and never presented as live video', async ({ page }) => {
  await cameraFixture(page,true)
  await page.goto('/')
  await page.getByRole('button',{name:'监控中心',exact:true}).click()
  await expect(page.getByRole('button',{name:'获取 HA 截图',exact:true})).toBeEnabled()
  expect(await page.evaluate(()=>(window as any).__cameraTest.snapshots)).toBe(0)
  await page.getByRole('button',{name:'获取 HA 截图',exact:true}).click()
  await expect(page.getByRole('img',{name:'摄像头截图（不是实时视频）'})).toBeVisible()
  await expect(page.getByText('截图（非实时）',{exact:true})).toBeVisible()
  await expect(page.getByText('正在播放',{exact:true})).toHaveCount(0)
  expect(await page.evaluate(()=>(window as any).__cameraTest.starts.length)).toBe(0)
})

test('a session created after leaving the page is immediately released', async ({ page }) => {
  await cameraFixture(page,false,true)
  await page.goto('/')
  await page.getByRole('button',{name:'监控中心',exact:true}).click()
  await expect.poll(()=>page.evaluate(()=>(window as any).__cameraTest.starts.length)).toBe(1)
  await page.getByRole('button',{name:'设备中心',exact:true}).click()
  await expect.poll(()=>page.evaluate(()=>(window as any).__cameraTest.stops.length)).toBe(1)
  expect(await page.evaluate(()=>(window as any).__cameraTest.opened)).toBe(0)
})

test('missing decoded video frames triggers bounded retries instead of false playback', async ({ page }) => {
  await cameraFixture(page)
  await page.clock.install()
  await page.goto('/')
  await page.getByRole('button',{name:'监控中心',exact:true}).click()
  await expect.poll(()=>page.evaluate(()=>(window as any).__cameraTest.offers)).toBe(1)
  for (const [index,delay] of [1000,2000,5000,10000,30000].entries()) {
    await page.clock.fastForward(30001)
    await expect(page.getByText(`视频重连中（${index+1}/5）`,{exact:true}).first()).toBeVisible()
    await page.clock.fastForward(delay+1)
    await expect.poll(()=>page.evaluate(()=>(window as any).__cameraTest.offers)).toBe(index+2)
  }
  await page.clock.fastForward(30001)
  await expect(page.getByText('视频连接失败',{exact:true}).first()).toBeVisible()
  await page.clock.fastForward(60000)
  expect(await page.evaluate(()=>(window as any).__cameraTest.starts.length)).toBe(6)
  await expect(page.getByText('正在播放',{exact:true})).toHaveCount(0)
})

test('single view can select a camera in a hidden slot without creating duplicates', async ({ page }) => {
  await cameraFixture(page)
  await page.goto('/')
  await page.getByRole('button',{name:'监控中心',exact:true}).click()
  await expect.poll(()=>page.evaluate(()=>(window as any).__cameraTest.offers)).toBe(1)
  await page.locator('.monitor-slot .n-select').click()
  await page.getByText('摄像头3',{exact:true}).click()
  await expect(page.locator('[data-camera-id="camera.test3"]')).toBeVisible()
  await expect.poll(()=>page.evaluate(()=>(window as any).__cameraTest.starts.length)).toBe(2)
  await page.getByRole('button',{name:'四宫格',exact:true}).click()
  await expect.poll(()=>page.evaluate(()=>(window as any).__cameraTest.offers)).toBe(5)
  const ids=await page.locator('[data-camera-id]').evaluateAll(elements=>elements.map(element=>element.getAttribute('data-camera-id')))
  expect(ids).toHaveLength(4)
  expect(new Set(ids).size).toBe(4)
})


test('real local WebRTC test frames decode before playback is reported', async ({ page }) => {
  await cameraFixture(page,false,false,true)
  await page.goto('/')
  await page.getByRole('button',{name:'监控中心',exact:true}).click()
  await expect.poll(()=>page.evaluate(()=>(window as any).__cameraStage)).toBe('answered')
  const media = await page.evaluate(()=>{
    const w=window as any
    const offer=w.__cameraReceiver.localDescription.sdp as string
    const answer=w.__cameraSender.localDescription.sdp as string
    return { accepted: /m=video [1-9]/.test(offer) && /m=video [1-9]/.test(answer), candidates: offer.includes('a=candidate:') || answer.includes('a=candidate:') }
  })
  expect(media.accepted).toBe(true)
  test.skip(!media.candidates,'Managed Chromium gathered no local ICE candidates; native media connectivity requires a supported environment.')
  await expect(page.getByText('正在播放',{exact:true})).toBeVisible({timeout:20000})
  await expect.poll(()=>page.locator('[data-camera-id] video').evaluate((video: HTMLVideoElement)=>video.videoWidth)).toBe(320)
  await page.getByRole('button',{name:'设备中心',exact:true}).click()
  await expect.poll(()=>page.evaluate(()=>(window as any).__cameraTest.stops.length)).toBe(1)
})


test('real local media tracks trigger frame-based playback and are released', async ({ page }) => {
  await cameraFixture(page,false,false,false,true)
  await page.goto('/')
  await page.getByRole('button',{name:'监控中心',exact:true}).click()
  await expect(page.getByText('正在播放',{exact:true})).toBeVisible()
  await expect.poll(()=>page.locator('[data-camera-id] video').evaluate((video: HTMLVideoElement)=>video.videoWidth)).toBe(320)
  await page.getByRole('button',{name:'设备中心',exact:true}).click()
  await expect.poll(()=>page.evaluate(()=>(window as any).__cameraTest.closed)).toBe(1)
})


test('hiding to tray releases camera peers and showing the window restores the visible view', async ({ page }) => {
  await cameraFixture(page)
  await page.goto('/')
  await page.getByRole('button',{name:'监控中心',exact:true}).click()
  await expect.poll(()=>page.evaluate(()=>(window as any).__cameraTest.offers)).toBe(1)
  await page.evaluate(()=>(window as any).__cameraEmit('app:window_visible',false))
  await expect.poll(()=>page.evaluate(()=>(window as any).__cameraTest.stops.length)).toBe(1)
  await expect(page.getByText('窗口隐藏，播放已暂停',{exact:true})).toBeVisible()
  await page.evaluate(()=>(window as any).__cameraEmit('app:window_visible',true))
  await expect.poll(()=>page.evaluate(()=>(window as any).__cameraTest.offers)).toBe(2)
})
