import { test, expect, type Page } from '@playwright/test'
async function phaseFiveFixture(page: Page) {
  await page.addInitScript(()=>{
    localStorage.setItem('mihome.source', 'ha')
    const w=window as any
    const callbacks:Record<number,(value:unknown)=>void>={};const events:Record<string,number>={};let id=0
    const favorites:Record<string,boolean>={}
    let settings={theme:'light',requestTimeout:10,autoConnect:true,autoReconnect:true,closeToTray:false,autoStart:false}
    const entities=[{entityId:'scene.evening',name:'晚间场景',domain:'scene',state:'unknown',unit:null,lastUpdated:'2026-11-10T01:00:00Z',attributes:{}},{entityId:'scene.offline',name:'不可用场景',domain:'scene',state:'unavailable',unit:null,lastUpdated:'2026-11-10T01:00:00Z',attributes:{}}]
    const stats={executed:[] as string[],saved:[] as unknown[],exports:0,cancelExport:true,finish:null as ((value:unknown)=>void)|null}
    w.__phase5=stats
    w.isTauri=true
    w.__TAURI_EVENT_PLUGIN_INTERNALS__={unregisterListener(){}}
    w.__TAURI_INTERNALS__={
      transformCallback(callback:(value:unknown)=>void){callbacks[++id]=callback;return id},
      async invoke(command:string,args:any){
        if(command==='plugin:event|listen'){events[args.event]=args.handler;return args.handler}
        if(command==='plugin:event|unlisten')return null
        const values:Record<string,unknown>={
          settings_get:settings,settings_get_app_info:{name:'Mi Home Desktop',version:'0.1.0',platform:'test-fixture'},
          ha_get_saved_connection:{id:'primary',name:'HA fixture',baseUrl:'http://127.0.0.1:8123/'},ha_get_connection_status:{state:'connected',attempt:0,error:null},
          ha_get_entities:entities,ha_get_catalog:{areas:[],services:{scene:['turn_on']},entityAreas:{},warnings:[],servicesLoaded:true},device_get_preferences:[],
          camera_get_configuration:{gateway:{baseUrl:'',enabled:false},profiles:[]},
          desktop_get_status:{trayAvailable:true,autoStartSupported:true,autoStartEnabled:settings.autoStart,windowStateEnabled:true},
          settings_get_diagnostics:{version:'0.1.0',platform:'test-fixture',connectionState:'connected',retryAttempt:0,errorCode:null,entityCount:2,sceneCount:2,cameraCount:0},
          scene_get_favorites:Object.entries(favorites).map(([sceneEntityId,isFavorite])=>({sceneEntityId,isFavorite})),
        }
        if(command in values)return {ok:true,data:values[command]}
        if(command==='scene_execute'){stats.executed.push(args.sceneEntityId);return new Promise(resolve=>{stats.finish=resolve})}
        if(command==='scene_set_favorite'){favorites[args.sceneEntityId]=args.favorite;return {ok:true,data:{sceneEntityId:args.sceneEntityId,isFavorite:args.favorite}}}
        if(command==='settings_update'){settings=args.settings;stats.saved.push(args.settings);return {ok:true,data:settings}}
        if(command==='settings_export_logs'){if(!args.acknowledged)throw new Error('Missing privacy acknowledgment');stats.exports++;return {ok:true,data:{cancelled:stats.cancelExport,fileName:stats.cancelExport?null:'fixture-diagnostics.json'}}}
        throw new Error(`Unexpected phase 5 command: ${command}`)
      },
    }
  })
}

test('scene execution accepts unknown activation state and prevents duplicate pending requests',async({page})=>{
  const errors:string[]=[];page.on('pageerror',error=>errors.push(error.message))
  await phaseFiveFixture(page);await page.goto('/')
  await page.getByRole('button',{name:'智能场景',exact:true}).click()
  const scene=page.locator('[data-scene-id="scene.evening"]')
  await expect(scene.getByRole('button',{name:'执行场景'})).toBeEnabled()
  await expect(page.locator('[data-scene-id="scene.offline"]').getByRole('button',{name:'执行场景'})).toBeDisabled()
  await scene.getByRole('button',{name:'收藏',exact:true}).click()
  await expect(scene.getByRole('button',{name:'取消收藏',exact:true})).toBeVisible()
  await scene.getByRole('button',{name:'执行场景'}).click()
  await expect(scene.getByRole('button',{name:'执行场景'})).toBeDisabled()
  expect(await page.evaluate(()=>(window as any).__phase5.executed)).toEqual(['scene.evening'])
  await page.evaluate(()=>(window as any).__phase5.finish({ok:true,data:{accepted:true,message:'HA accepted'}}))
  await expect(scene.getByRole('status')).toContainText('HA 已处理场景请求')
  await expect(scene.getByText('最近激活：尚未提供激活时间')).toBeVisible()
  await page.getByRole('switch',{name:'仅收藏场景'}).click()
  await expect(page.locator('[data-scene-id]')).toHaveCount(1)
  await page.getByRole('textbox',{name:'搜索场景'}).fill('不存在')
  await expect(page.getByText('没有匹配的场景')).toBeVisible()
  expect(errors).toEqual([])
})

test('scene service failures remain explicit and do not retry automatically',async({page})=>{
  await phaseFiveFixture(page);await page.goto('/')
  await page.getByRole('button',{name:'智能场景',exact:true}).click()
  const scene=page.locator('[data-scene-id="scene.evening"]')
  await scene.getByRole('button',{name:'执行场景'}).click()
  await page.evaluate(()=>(window as any).__phase5.finish({ok:false,error:{code:'SERVICE_CALL_FAILED',message:'请先查看设备状态再决定是否重试',retryable:false}}))
  await expect(scene.getByRole('status')).toContainText('SERVICE_CALL_FAILED')
  expect(await page.evaluate(()=>(window as any).__phase5.executed.length)).toBe(1)
})

test('desktop preferences submit explicitly and diagnostic export requires acknowledgment',async({page})=>{
  await phaseFiveFixture(page);await page.goto('/')
  await page.getByRole('button',{name:'配置连接',exact:true}).click()
  await expect(page.getByRole('switch',{name:'Windows 开机启动'})).toBeEnabled()
  await page.getByRole('switch',{name:'关闭窗口后留在托盘'}).click()
  await page.getByRole('switch',{name:'Windows 开机启动'}).click()
  await page.getByRole('button',{name:'保存设置',exact:true}).click()
  await expect.poll(()=>page.evaluate(()=>(window as any).__phase5.saved.length)).toBe(1)
  const submitted=await page.evaluate(()=>(window as any).__phase5.saved[0])
  expect(submitted).toMatchObject({closeToTray:true,autoStart:true})
  await expect(page.getByRole('button',{name:'导出诊断包',exact:true})).toBeDisabled()
  await page.getByRole('checkbox').check()
  await page.getByRole('button',{name:'导出诊断包',exact:true}).click()
  await expect(page.getByText('已取消导出。',{exact:true})).toBeVisible()
  await page.evaluate(()=>{(window as any).__phase5.cancelExport=false})
  await page.getByRole('button',{name:'导出诊断包',exact:true}).click()
  await expect(page.getByText('已保存诊断包：fixture-diagnostics.json',{exact:true})).toBeVisible()
})
