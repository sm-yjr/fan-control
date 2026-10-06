# fan-core

这个 crate 保存可跨平台的纯业务规则：热负荷估计、曲线插值与迟滞、转速边界、写入调度、模式校准和配置迁移。它不打开 SMC、不安装 helper、不写硬件，也不读取文件或创建线程。宿主必须串行执行 `Action`，然后以真实结果调用 `acknowledge`。

## 采样与确认

`Snapshot.sampled_at` 和 `Controller::update` 的 `now` 使用同一单调时钟，单位为秒。SMC 每轮采样都必须重新读取风扇数量、模式和转速边界；失败的温度或 tachometer 值表示为 `None`，不能复制上一轮值冒充新数据。超过 15 秒的快照会触发系统自动控制。

`Controller::update(&snapshot, now)` 返回本轮动作。每个风扇最多有一个等待确认的动作。`SetRpm` 同时请求强制模式和目标转速，由平台层在写入前重新读取真实硬件模式并验证实时边界。平台层发生部分失败时必须立即尝试自动控制。

`acknowledge(&action, success, now)` 只确认当前 action id 和配置 generation。成功后才提交 `applied_rpm`、停转驻留和启动驻留；失败不会提交计划目标，并会要求下一轮优先交还系统。配置变更、睡眠和恢复使旧回执失效。宿主必须确认 helper 实际写入值与 action 的 RPM 相同；helper 重新钳制后的不同 RPM 不能当作计划值成功。

`FanStatus.desired_rpm` 是曲线或用户目标，`applied_rpm` 是最近确认成功的命令值，`Fan.current_rpm` 是实际测速值。三者用途不同。`pending`、`last_failure`、`safety_override` 和 `status_reason` 可用于界面说明等待、失败与保护状态。传感器短暂缺失期间不一定产生新 action，宿主仍应查询状态。

## 保护规则

- 正转请求必须满足本轮真实的 `min_rpm` / `max_rpm`，并不超过 16,383 RPM。缺失、倒置、非有限边界禁止自定义写入；fan id 仅接受实时有效的 0...9。
- 所有自定义模式都响应系统严重热压、危险热压和原始芯片温度至少 96°C 的紧急保护，绕过曲线 ramp 和驻留。危险热压要求实时硬件上限；严重热压至少 70% 区间；96°C 保护至少 80% 区间。
- 自定义输入短暂缺失时保留已确认目标。首次无安全温度数据时保持系统控制；已有控制状态在 CPU/GPU 安全遥测全部缺失 30 秒后交还系统。仍有有效严重 / 危险热压时立即按保护底线散热。
- 曲线负速度仅在有效实时最小转速为 0 的设备上发出显式 0 RPM。此路径停转驻留 90 秒，重新启动驻留 180 秒，均从成功确认开始。实时最小转速大于 0 时，负速度产生 `LowDemandSystem` 自动控制；系统决定低负荷下是否停转。升温后恢复曲线，不把系统物理停转当作应用确认的停转能力。
- 曲线正常升速限制为 350 RPM/s，降速为 250 RPM/s；手动修改立即执行。重复目标不重复写，较小变化采用 75 RPM / 5 秒节流。
- 硬件意外返回自动模式时重新应用自定义目标；持续 10 秒的测速偏差按至少 15 秒间隔重试。期望自动模式但观察到强制模式时，也限频交还系统。

热模型对 CPU / GPU 组平均温度的较大值应用 30 秒低通，对机身代表温度应用 90 秒低通。原始最热芯片值独立触发紧急保护。Airport 温度不计入机身热容量。丢失的单独滤波节点最多保守保留 30 秒，之后不再把旧温度持续作为模型输入。

## 智能性能与舒适策略

`ControlMode::Adaptive` / `FanConfig::adaptive(id)` 是独立智能模式。快照提供同轮 `cpu_utilization_percent`（0...100 或 `None`）与私有 `machine_id`；没有 GPU 利用率时只使用 GPU 温度。CPU 利用率至少 65% 持续 4 秒才触发 25...55% 区间前馈，单次负载 spike 不启动。CPU/GPU 分别计算温升率，确认持续上升 4 秒后预测未来 20 秒，再取两组较高预测需求。持续芯片/机身热浸使用缓慢衰减；已确认智能散热至少驻留 180 秒，冷负荷后交还系统。智能模式不请求显式停转，初次接管和硬件恢复自动模式时至少保留系统当前 RPM。

`Config.thermal_policy` 包含默认 `Some(38.0)` 的 `comfort_target_celsius` 和可选 `SurfaceCalibration`。目标允许 30...45°C。`SurfaceCalibration::from_measurements` 接受同机器、同传感器的两个独立 proxy/真实表面测量点：内部跨度至少 3°C、表面正相关、两个偏移均在 ±40°C 且偏移差不超过 3°C。估计使用平均偏移，且当前 proxy 和表面估计必须同时位于实测校准范围。跨设备、未知身份、缺失/无效读数和无效策略停用舒适估计及其散热下界，仅使用性能/温度趋势策略；CPU/GPU 安全遥测、快照时效和实时 RPM 边界仍是自定义控制的前置条件。

越过校准高端时，`AboveCalibrationRange` 不输出表面温度估计，但保留最高双范围有效端点对应的舒适散热下界。最高有效估计端点为 `min(sensor_max_celsius + offset_celsius, surface_max_celsius)`；这条下界不依赖历史 ACK，并且最终写入不会因正常升速 ramp 而低于它。因此越过高端不会突然撤掉边缘已经需要的散热，首次采样已经越界时同样适用。越过低端时返回 `BelowCalibrationRange`、无估计且舒适需求为 0；温度回到两项范围内后恢复正常估计及 ramp。`comfort_demand_percent` 表示这部分舒适需求，高端越界时表示保守下界，不表示未经校准的温度外推。

`ThermalReading.adaptive` 给出当前 CPU 利用率、负载是否持续、温升率、芯片温度预测、热浸需求、表面估计、`ComfortStatus` 和 `AdaptiveIntervention`。表面估计不是实测键盘表面温度；未校准的机身传感器只可代表热浸 proxy。`Snapshot.machine_id` 不参与 JSON 导出，配置内身份仅供本机私有保存。预测控制已用模拟验证提前介入，但持续性能释放和表面温度效果仍需指定设备实测。

## 配置兼容

`Config::from_json` 支持 version 2 当前配置、version 1 Rust 对象和旧 Swift `[FanState]` 数组。旧 Swift 枚举如 `{"manual":{"rpm":3000}}` 与 `{"curve":{"configId":"..."}}` 可解析。曲线 UUID 保留，旧 runtime 的温度、速度与方向缓存不恢复。未改动的旧 `Default` / `Average CPU` 预设迁移到 `Balanced Thermal`，自定义曲线点保留。

`ConfigLoad.migrated` 说明需要写回新格式，`warnings` 说明哪些损坏配置已降级为系统自动。未来配置版本拒绝加载；宿主应保留原文件并显示错误。`to_json` 在编码前验证，不允许把非有限数值写入配置。原文件备份、私有权限和原子替换由宿主负责。

`set_fan_config` 和 `replace_config` 验证候选配置后再修改控制器。草稿编辑可在 GUI 中独立进行，保存时一次性应用。`prepare_for_sleep` / `prepare_for_shutdown` 返回交还动作并暂停控制；宿主还应调用平台级 `reset_all` 并等待真实确认。唤醒后 `resume` 清理滤波与调度缓存，宿主应在多个恢复时点重新采样和校准。

## 验证

```bash
cargo test -p fan-core
cargo clippy -p fan-core --all-targets -- -D warnings
```

测试使用虚构硬件快照和逻辑时钟，覆盖热模型、配置、驻留、紧急绕过、写入回执、失败回退、输入失联和恢复。它们不证明实机支持停转，也不替代 root helper、真实睡眠—唤醒、签名、公证与发布验收。
