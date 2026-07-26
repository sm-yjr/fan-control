# Fan Control

[![CI](https://github.com/sm-yjr/fan-control/actions/workflows/ci.yml/badge.svg)](https://github.com/sm-yjr/fan-control/actions/workflows/ci.yml)
[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](LICENSE)

Fan Control 是一个面向 macOS 的菜单栏风扇控制工具。它读取 Apple SMC 传感器，支持系统自动、固定转速和温度曲线三种模式，并在睡眠前把风扇交还给系统；唤醒后会重新读取 SMC 状态并恢复用户配置。

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
MACOS_CERTIFICATE
MACOS_CERTIFICATE_PASSWORD
APPLE_ID
APPLE_APP_SPECIFIC_PASSWORD
SPARKLE_PRIVATE_KEY
```

`MACOS_CERTIFICATE` 是 Developer ID Application 的 PKCS#12 文件经过 Base64 编码后的内容。`APPLE_APP_SPECIFIC_PASSWORD` 用于 `notarytool`。Sparkle 私钥只保存在 GitHub Actions Secrets 和发布者钥匙串中；仓库只包含对应公钥。

## 许可证

Fan Control 采用 [GNU General Public License v3.0 only](LICENSE)。发布包包含的 Sparkle 使用 MIT License，详见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
