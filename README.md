# Fan Control

[![CI](https://github.com/sm-yjr/fan-control/actions/workflows/ci.yml/badge.svg)](https://github.com/sm-yjr/fan-control/actions/workflows/ci.yml)
[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](LICENSE)

Fan Control 是一个面向 macOS 的菜单栏风扇控制工具。它读取 Apple SMC 传感器，支持系统自动、固定转速和热负荷曲线三种模式。默认曲线根据机身需要排出的持续热量调速，过滤 CPU/GPU 核心的短时温度尖峰；进入睡眠前会把风扇交还系统，唤醒后重新读取 SMC 状态并恢复用户配置。

> [!WARNING]
> 错误的风扇曲线可能导致过热、降频、数据丢失或硬件损坏。请保留温度余量并观察实际温度。软件按 GPL-3.0 的无担保条款提供。

## 系统要求

当前发布目标是 Apple Silicon Mac，最低系统版本为 macOS 14。源码使用 Swift Package Manager 管理，可以只安装 Command Line Tools 构建，无需完整 Xcode：

```bash
xcode-select --install
swift --version
```

项目内的 `CLTState` 属性包装器替代了依赖完整 Xcode 插件目录的 `SwiftUIMacros.StateMacro`。Sparkle 以固定版本的预编译框架随应用打包，不参与 privileged helper 的动态链接。

## 安装

从 [GitHub Releases](https://github.com/sm-yjr/fan-control/releases) 下载最新的 `FanControl-版本号.zip`，解压后把 `FanControl.app` 移到 `/Applications`。首次修改风扇设置时，应用会请求管理员授权，把同一个可执行文件安装为 launch daemon：

```text
/Library/PrivilegedHelperTools/com.local.fan-control.helper
/Library/LaunchDaemons/com.local.fan-control.helper.plist
```

helper 只监听本机 Unix domain socket。应用升级后，如果 helper 协议版本变化，界面会提示重新安装。

## 本地构建

最小构建与运行命令如下：

```bash
./script/build_and_run.sh
```

产物位于 `dist/FanControl.app`。常用诊断命令为：

```bash
./script/build_and_run.sh build
./script/test_thermal_model.sh
./script/test_status_item_presentation.sh
./script/test_app_launch_mode.sh
./script/build_and_run.sh --verify
./script/build_and_run.sh --logs
./script/build_and_run.sh --telemetry
./script/build_and_run.sh --debug
```

发布模式可以显式指定版本、构建号和架构：

```bash
APP_VERSION=1.2.0 \
BUILD_NUMBER=120 \
BUILD_CONFIGURATION=release \
ARCHITECTURES=arm64 \
./script/package_app.sh
```

构建脚本会从 Sparkle 官方 GitHub Release 下载 2.9.2，校验 SHA-256 后再复制框架。`.app` 内的可执行文件通过运行时桥接加载 Sparkle，因此被单独复制到 privileged helper 目录后仍能启动。

## 界面与状态栏反馈

界面继续使用原生 SwiftUI 控件，风扇和控制模式使用 macOS 分段选择器，控制源使用菜单选择器。温度、热负荷和转速同时提供文本值，颜色只用于补充风险层级；展开按钮、滑块、曲线控制点和状态栏图标均提供辅助功能标签或操作。

状态栏图标根据所有风扇中最高的工作区间展示活动状态：

| 状态 | 判定 | 图标反馈 |
| --- | --- | --- |
| 停转 | 所有风扇低于 100 RPM | 静止、降低不透明度 |
| 低速 | 至少一个风扇工作，最高转速低于可控区间的 55% | 2.4 秒一圈，由系统合成层旋转 |
| 高速 | 至少一个风扇达到可控区间的 55% | 0.8 秒一圈，由系统合成层旋转 |

状态项沿用 [Stats](https://github.com/exelban/stats) 等开源 macOS 菜单栏工具的原生 `NSStatusItem` 结构，弹窗内容仍由 SwiftUI 承载。风扇 SF Symbol 放在状态栏按钮的 18×18 pt 正方形子视图中，Core Animation 围绕图层中心执行线性旋转，应用进程无需按帧切换图片或重新计算 SwiftUI 视图。停转或系统开启“减少动态效果”时会移除动画。图标粗细、不透明度、工具提示和 VoiceOver 状态仍能表达当前层级。

本项目不使用 shadcn/ui 替换原生控件。shadcn/ui 当前面向 React 和 Tailwind CSS，组件依赖浏览器 DOM、Radix 语义和前端构建链；接入菜单栏应用需要额外嵌入 WebView 与 JavaScript 运行时，会削弱原生键盘、VoiceOver、系统外观和菜单栏行为，同时扩大 helper 同一可执行文件的打包与安全边界。它适合未来独立的 Web 管理界面，当前 macOS 客户端只借鉴其信息层级和间距做法。参考 [Apple HIG Accessibility](https://developer.apple.com/design/human-interface-guidelines/accessibility)、[Apple HIG Motion](https://developer.apple.com/design/human-interface-guidelines/motion) 和 [shadcn/ui Tailwind v4 文档](https://ui.shadcn.com/docs/tailwind-v4)。

## 热负荷模型

瞬时核心温度适合保护芯片结温，无法直接表示机身中已经积累、需要由风扇排出的热量。默认控制源 `Thermal Load` 使用一个 0–100% 的两节点热模型：

1. CPU 与 GPU 的组平均温度取较高值，经过 30 秒低通滤波，表示持续发热源。单个核心的短时睿频尖峰通常不会明显改变这一项。
2. Mainboard、Airflow、NAND 和 Battery 等系统传感器取第 75 百分位，再经过 90 秒低通滤波，表示机身热容。该节点的正向升温速度用于提前识别热量正在积累。
3. 有机身传感器时，热负荷由 35% 持续热源、55% 机身热浸和 10% 机身升温趋势组成。机型未暴露可用机身传感器时，应用退化为持续热源模型，并在展开的传感器区域明确显示。
4. macOS `ProcessInfo.thermalState` 提供平台级安全下限：`fair`、`serious` 和 `critical` 至少对应 35%、75% 和 100% 热负荷。原始芯片温度从 96°C 起保留独立紧急保护，避免低通滤波延迟安全响应。

这个指标是跨机型的控制量，不是热功率计，单位也不是瓦特。Apple 没有为这些 SMC 键公开统一校准值，所以曲线保留系统热压力和极端结温两条保护路径。模型依据可在界面展开区域检查，包括持续芯片温度、机身热容温度、升温速度和系统热压力。

## 默认散热曲线

在相似风机和阻抗条件下，风量近似随转速线性变化，风机功率近似随转速的三次方变化。默认曲线因此在中低热负荷区保持关闭或最低连续转速，仅在热浸持续增加时进入高转速区。参考资料包括 [ASHRAE fan laws](https://terminology.ashrae.org/?letter=F)、[美国能源部风机系统资料](https://www1.eere.energy.gov/manufacturing/tech_assistance/pdfs/fan_sourcebook.pdf) 和 [Apple thermal state 文档](https://developer.apple.com/documentation/foundation/processinfo/thermalstate-swift.enum)。

| 热负荷 | 风扇目标 |
| ---: | ---: |
| 0–18% | 停转 |
| 28% | 最低可持续转速 |
| 45% | 可控转速区间的 12% |
| 60% | 可控转速区间的 25% |
| 75% | 可控转速区间的 45% |
| 88% | 可控转速区间的 70% |
| 100% | 最大转速 |

这里的“可控转速区间”是风扇硬件最小 RPM 到最大 RPM 之间的范围。热负荷迟滞默认为 8 个百分点；停转后至少保持 90 秒，启动后至少运行 180 秒。达到 `serious`/`critical` 热压力、90% 热负荷或高转速需求时会绕过驻留和缓升限制。这样可以减少临界点附近的频繁起停，同时保留快速排热能力。

从旧版本升级时，内容完全等于旧版默认值的 `Average CPU` 曲线会迁移到新模型。修改过控制点、迟滞或名称的自定义曲线保持原配置；用户仍可在控制源菜单中选择 CPU、GPU 或单个温度传感器。

## 睡眠与唤醒

进入睡眠前，控制器会恢复系统自动模式。唤醒后，macOS 可能已经重置 SMC 的强制控制位，所以应用会在多个时间点重新读取实际模式，并重放睡眠前的固定转速或曲线配置。这个延迟重试覆盖显示器唤醒、系统唤醒和登录完成之间的时序差异。

如果配置没有恢复，请先查看统一日志：

```bash
./script/build_and_run.sh --telemetry
```

重点检查 `workspaceDidWake`、`wakeReapply`、helper 协议版本和 SMC 写入结果。

## Sparkle 与 GitHub Release

应用的稳定更新源是：

```text
https://github.com/sm-yjr/fan-control/releases/latest/download/appcast.xml
```

推送 `vMAJOR.MINOR.PATCH` 标签会触发发布工作流。工作流编译 Apple Silicon 版本，使用 Developer ID 签名并公证，生成 Ed25519 签名的 Sparkle appcast，然后把 zip 和 `appcast.xml` 上传到同一个 GitHub Release。

发布工作流需要以下 GitHub Actions Secrets：

```text
MACOS_CERTIFICATE_P12_BASE64
MACOS_CERTIFICATE_PASSWORD
APPLE_ID
APPLE_TEAM_ID
APPLE_APP_SPECIFIC_PASSWORD
SPARKLE_PRIVATE_KEY
```

`MACOS_CERTIFICATE_P12_BASE64` 是 Developer ID Application 的 PKCS#12 文件经过 Base64 编码后的内容。`APPLE_APP_SPECIFIC_PASSWORD` 用于 `notarytool`。Sparkle 私钥只保存在 GitHub Actions Secrets 和发布者钥匙串中；仓库只包含对应公钥。

## 许可证

Fan Control 采用 [GNU General Public License v3.0 only](LICENSE)。发布包包含的 Sparkle 使用 MIT License，详见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
