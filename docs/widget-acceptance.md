# 原生桌面 Widget 验收

下一版目标为 0.5.0。扩展是 SwiftUI / WidgetKit `StaticConfiguration`，仅支持 `systemSmall` / `systemMedium`，macOS 14+ arm64。CLT `swiftc` 构建，不需要完整 Xcode、Swift 宏插件或迁移 Rust 主 App。

## 数据与边界

GUI 把 CPU 平均温度、实测风扇 RPM、硬件范围、风扇数量、时间和最多 120 个历史点写入版本化 JSON；不含配置、目标转速、设备标识或控制指令。`sampled_at` 是写入时墙钟减去控制采样年龄，控制内部单调时钟保持原状。扩展只读 App Group 文件，不访问 SMC、helper、网络或根权限。无 RPM 与实测 0 RPM 区别显示；硬件范围未知时隐藏进度。温度缺失不回退伪造。趋势只连接间隔不超过 90 秒的历史点，不向未来外推。

采集时每 30 秒最多写入一次。正常退出记录 stopped，睡眠记录 sleeping，唤醒清空趋势等待采样。崩溃依靠 120 秒采样过期；没有存活保证，不宣称正在控制或系统自动。WidgetKit 接收 15 分钟后的更新请求，生命周期变化请求 reload，实际调度由系统决定；已有时间线包含读数过期条目。展示采样时间，不承诺实时刷新。旧 schema、超大/损坏 JSON、未来时间和非法数值都显示未知。

使用 macOS 风格同团队 App Group `JFC5CWT3V6.com.local.fan-control.readings`。这避免新增 Developer 网站注册及 provisioning 凭据；Apple 当前建议新代码使用 iOS 风格 `group.*`，该路线需要 App ID / provisioning profiles 的额外准备。这里仍需 Developer ID 实测 macOS 14 与当前系统的共享读取，并检查是否出现 App Group 容器保护提示。出现意外权限提示立即停止，交由用户协调。主 App 非沙盒；Widget 沙盒无网络权限；root helper 不加入组。签名内到外增加 Swift bridge、Widget，然后主 App，保留 Hardened Runtime。

## 当前验证及发布门槛

- CLT 扩展 arm64 编译和 macOS14 最低版本：通过。
- Rust 时间换算、空读数、无风扇、非法 RPM，Swift 旧/损坏快照、生命周期、过期、未知范围：自动测试。
- 离线布局渲染只用于小号/中号、单双风扇、深浅色和缺失状态审查，不能代替系统 Widget 验收。
- 已批准设计图：Library 403，等待用户直接附图，尚未比对实际像素。
- 当前 Mac Studio 桌面：重新检查为锁定；Gallery 注册、桌面添加、点击进入主 App、系统深浅色、VoiceOver、系统刷新仍待解锁验证。
- Developer ID / App Group 真实读取：需要批准工作区候选签名与启动；不覆盖已安装 App/helper，不恢复用户控制配置、不执行 RPM 写入。可在候选 App 与 .appex 各运行 `--check-widget-container`，比对新 nonce 来验证真实签名下的共享读取；诊断只写独立测试文件，完全不连接 SMC 或 helper。应使用隔离只读诊断候选并验证容器归属，失败不能放宽机器安全设置。
- 真实签名、公证/Gatekeeper、Sparkle 资产校验：发布前完成。版本/build 必须与主 App 一致，独立 helper 必须无 App 内 dylib 依赖或组权限。
- 窗口上下抖动 hotfix：另任务开发，发布前整合可移植提交并复验；此分支只增加 URL delegate，不改布局。

依据：[Apple WidgetKit 刷新说明](https://developer.apple.com/documentation/widgetkit/keeping-a-widget-up-to-date)、[App Group 容器文档](https://developer.apple.com/documentation/xcode/accessing-app-group-containers)、[Apple DTS 的 macOS App Group 说明](https://developer.apple.com/forums/thread/721701)。
