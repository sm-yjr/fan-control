# Fan Control

Fan Control 是一个 SwiftPM 管理的 macOS 菜单栏应用，用于读取传感器和控制风扇。项目主入口是 `Package.swift`，可执行 target 名称是 `FanControl`。

本机开发默认使用统一脚本：

```bash
./script/build_and_run.sh
```

这个命令会停止已有的 `FanControl` 进程，执行 `swift build`，把 SwiftPM 产物打包到 `dist/FanControl.app`，然后启动应用。Codex 桌面端的 Run 动作也已经指向这个脚本。

这个项目使用 SwiftUI，需要完整 Xcode 提供 SwiftUI 宏插件。当前机器只选中了 Command Line Tools 时，脚本会在编译前提示安装 Xcode，并要求执行：

```bash
sudo xcode-select -s /Applications/Xcode.app/Contents/Developer
```

常用调试命令：

```bash
./script/build_and_run.sh --verify
./script/build_and_run.sh --logs
./script/build_and_run.sh --telemetry
./script/build_and_run.sh --debug
swift test
```

首次需要真正写入风扇控制时，应用内会引导安装 privileged helper。普通构建和启动不会自动安装 helper。
