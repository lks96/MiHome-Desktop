# 设备管理与控制

## 本阶段范围

设备中心按 Home Assistant 实体呈现，同一物理设备可以有多个实体；不把实体数量称为物理设备数量。

已实现名称/ID 搜索、房间筛选和分组、domain 筛选、可用状态筛选、收藏、隐藏与恢复、自定义显示名、本地房间覆盖和本地排序。首页提供收藏快捷控制。偏好通过 SQLx 写入 SQLite `entity_overrides`，不修改 HA 注册表。

详情面板展示 HA 名称、实体 ID、房间、类别、状态、更新时间及经过白名单筛选的关键属性。未知 domain 仍显示基础状态。

## 控制能力

- light：开关；有对应属性和色彩模式时显示亮度、Kelvin 色温、RGB 颜色。
- switch：开关。
- climate：有目标温度特性时设置温度；模式、风速选项来自实体实际暴露的列表。
- fan：开关；有对应特性、参数与服务时设置百分比和预设模式。
- sensor、binary_sensor 和其他 domain：当前显示状态及关键属性，专用操作后续扩展。

只有服务注册表声明相应服务时才显示控制。断线、unavailable/unknown、服务信息不可用时禁用控制；Rust 再校验实体 ID、domain、服务、唯一目标、参数字段、数值范围、步进和枚举。前端无法通过此接口向任意实体组或任意服务提交参数。

一次 HTTP POST 只针对一个实体，不自动重试。HA 返回成功表示服务请求已经处理，不代表真实硬件状态已确认；UI 不提前改变状态，等待 WebSocket 事件。失败或结果不明确时先查看状态再决定是否重试。

## 房间与服务信息

同步实体快照后异步请求 `get_services`、`config/area_registry/list`、`config/entity_registry/list` 和 `config/device_registry/list`。HA 实体区域优先于物理设备区域，客户端本地房间覆盖优先于 HA 区域。权限不足只影响对应元数据，不阻塞基本实体列表；缺少服务信息时控制保持禁用。

注册表元数据目前在连接/重新同步时刷新。HA 中修改房间或服务后，请点击“重新同步”。UI 的“重新同步”会重新建立实时连接和订阅。隐藏项默认不展示，通过“显示隐藏”可找回并取消隐藏。

## IPC

- `ha_get_catalog`：房间、服务名、实体区域映射和非敏感加载警告。
- `device_get_preferences`：读取本地实体偏好。
- `device_update_preferences`：原子更新单个实体的本地偏好。
- `device_call_service`：接收 `{ request: { entityId, domain, service, serviceData } }`。

`ha:catalog_changed` 推送可选元数据，原有实体状态事件继续驱动设备状态。摄像头 `access_token`、认证 URL 和其他任意属性不会通过实体详情传到前端。

## 验证边界

Rust 测试覆盖真实 SQLite 偏好持久化、本地 HTTP 服务路由/授权头/目标数据、参数拒绝、注册表降级与区域优先级。Playwright 使用仅在测试中注入的 IPC fixture，检查控件、非乐观状态、收藏/重命名/隐藏恢复和搜索；应用不包含模拟设备数据。

真实 HA 设备、Windows Credential Manager、WebView2 和安装包测试仍等待最终 Windows 实机验收。尚未宣称任何具体设备型号实机通过。
