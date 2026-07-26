# AGENTS.md

## 输出与工作方式
使用简体中文，优先写自然段，只有命令、检查项或并列约束适合列表。直接说明结论、原因和下一步，不使用“不是……而是……”句式。

## 工程边界
Fan Control 是 macOS 14+、Apple Silicon、SwiftPM 管理的菜单栏应用。修改时保持以下约束：

- 项目必须能在仅安装 Command Line Tools 的机器上构建。SwiftUI 本地状态继续使用 `CLTState`，不要重新引入依赖 `SwiftUIMacros.StateMacro` 的 `@State`。
- 原生界面使用 `Sources/FanControl/FanUI` 中按项目持有源码的设计令牌和组合组件。新增重复的背景、边框、圆角、字号、字重或间距前先扩展语义令牌；Button、Picker、Slider、Menu 等交互继续以系统控件为基础。
- `FanControl` 同一可执行文件兼任 GUI 和 `--helper` root daemon。helper 会被单独复制到 `/Library/PrivilegedHelperTools`，主程序不得直接链接 Sparkle 或其他只存在于 `.app` 内的动态框架；合并前用 `otool -L` 检查。
- Sparkle 由 `Sources/SparkleRuntime` 在 GUI 进程中运行时加载，固定版本和 SHA-256 在 `script/package_app.sh`。升级 Sparkle 时同时更新版本、校验值并验证独立 helper。
- 修改 helper 请求格式、命令语义或需要替换已安装 helper 的实现时，递增 `FanHelperConstants.protocolVersion`；应用只依靠该版本判断是否提示重装。
- 睡眠前必须把风扇交还系统，唤醒后保留多时点重试。SMC 强制模式可能在睡眠中被硬件重置，写入转速前必须读取真实模式，不能只信进程内缓存。
- 风扇写入属于硬件安全路径。保留转速边界、console-user peer 校验和失败时回退系统自动模式；不要用未经限定的 RPM 或传感器值做实机测试。

## 构建与验证
```bash
swift build
./script/test_fan_ui.sh
./script/test_thermal_model.sh
./script/test_status_item_presentation.sh
./script/test_app_launch_mode.sh
./script/test_update_runtime.sh
./script/package_app.sh
./script/build_and_run.sh --verify
```
当前 CLT SDK 不提供可解析的 XCTest/Testing 模块，所以热模型使用独立验证脚本，`swift test` 不作为检查入口。涉及 SMC、helper 或睡眠恢复的修改还要验证 `codesign --verify --deep --strict`、独立 helper 的 `otool -L`，并完成一次真实睡眠—唤醒。

## 发布
发布只通过 `.github/workflows/release.yml`：`vMAJOR.MINOR.PATCH` 标签触发 arm64 构建、Developer ID 签名、Apple 公证、Sparkle Ed25519 appcast 和 GitHub Release。`script/sign_app.sh` 必须按 Installer、Downloader、Autoupdate、Updater、Framework、App 的顺序由内到外签名，不能改回 `codesign --deep`。先确认 `main` CI 与六个 Actions Secrets，再推送标签；不要在日志、提交或终端输出 Secret 值。

## 深入文档
README.md 负责安装、CLT 构建、睡眠恢复和发布用法；SECURITY.md 负责漏洞报告与 root helper 风险；THIRD_PARTY_NOTICES.md 负责 Sparkle 的 MIT 许可证说明。
