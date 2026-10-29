# AGENTS.md

## 输出与工作方式

使用简体中文，优先写自然段，只有命令、检查项或并列约束适合列表。直接说明结论、原因和下一步，不使用“不是……而是……”句式。

## 工程边界

Fan Control 的生产技术栈正在迁移到 Rust，目标仍是 macOS 14+、Apple Silicon 的菜单栏应用。主构建使用 Cargo workspace；Sources/ 中的 Swift 实现暂时保留用于迁移对照，不得将其当作 Rust 产物已实现功能的证据。完成兼容性与产品验收后再移除旧源码。

- 必须能在仅安装 Command Line Tools 和 Rust 的机器上构建，无需完整 Xcode 或 Swift 宏插件。保持 MACOSX_DEPLOYMENT_TARGET=14.0；当前只接受 arm64，不能通过改元数据宣称兼容 Intel。
- fan-core 持有硬件无关的模型、曲线、配置与安全策略；fan-platform 持有 Rust SMC/IOKit、Unix socket、helper 和租约；fan-app 使用 AppKit 原生系统控件。重复的背景、边框、圆角、字号和间距先扩展语义令牌。交互保留键盘、VoiceOver、深浅色与 Reduce Motion。
- FanControl 同一可执行文件兼任 GUI 和 --helper root daemon。helper 被单独复制到 /Library/PrivilegedHelperTools，主程序不得直接链接 Sparkle 或其他仅存在于 .app 内的动态框架；用独立 helper 的 otool -L 验证。
- Sparkle 仅在 GUI 或专用 updater 诊断模式中运行时加载。固定版本和 SHA-256 位于 script/package_app.sh；升级时同时更新版本、校验值并验证独立 helper。
- Rust helper 协议版本为 9，使用有界控制租约；协议 7 修复已接受连接继承非阻塞导致的同连接写命令失败，协议 8 让无 Ftst 的机型在直接模式写入后等待固件回读，协议 9 在交还系统后保留固件目标、禁止清零 F*Tg。修改请求格式、命令语义或需要替换已安装 helper 的实现时递增协议版本；安装或替换需要正常管理员授权，应用依靠实际 readiness 和版本一致判断可用状态。
- 租约过期、GUI 失联和 console-user 变化必须安全交还系统。helper 保留 root/console-user peer 校验、请求边界、失败回退和真实 SMC 模式读取；绝不能仅依靠 GUI 的退出回调。
- 睡眠前必须把风扇交还系统，唤醒后保留多时点重试。SMC 强制模式可能在睡眠中重置，写入转速前读取真实模式，不能只信进程内缓存。
- 风扇写入属于硬件安全路径。保留硬件 RPM 上下限、所有自定义模式的紧急保护、采样有效性与过期策略；不要用未经限定的 RPM 或传感器值做实机测试。自动化测试默认使用模拟硬件。
- 智能模式必须在持续负载和温升趋势阶段介入；默认体感目标为 38°C。内部传感器不得宣称键盘表面实测，体感估计只在同设备外部两点校准且范围有效时启用；越高端须停止估计但保持保守散热下界，不能因继续升温而降低散热。性能优于系统默认需要可重复的实机对比证据。
- 配置路径保持 ~/.config/fan-control/config.json，兼容旧 Swift 配置、自定义曲线和 UUID。持久化失败、损坏配置和硬件写入失败必须显式反馈；不能把用户目标显示成已确认的实际状态。
- 运行脚本只能定位当前用户的准确 GUI 可执行路径，禁止 pkill -x FanControl 等同名批量终止方式，以免误杀独立 root helper。

## 构建与验证

```bash
MACOSX_DEPLOYMENT_TARGET=14.0 cargo build --locked
./script/test_rust.sh
./script/package_app.sh
./script/test_update_runtime.sh
bash -n build.sh script/package_app.sh script/build_and_run.sh script/test_rust.sh
```

test_rust.sh 执行 rustfmt、Clippy 与 workspace tests，实际覆盖范围以当前测试为准。旧 Swift 脚本仅作为迁移对照，不再作为 Rust CI 的主检查入口。禁止用通过纯模型测试、进程存在或脚本退出码推断硬件验收完成。

启动 GUI 的 ./script/build_and_run.sh --verify 可能恢复配置；先确认目标机器与控制状态。涉及 SMC、helper 或睡眠恢复的修改还要验证 codesign --verify --deep --strict、独立 helper 的 otool -L，并完成管理员安装、真实模式回读与一次真实睡眠—唤醒。尚未执行时，按 [Rust 产品验收清单](docs/rust-product-acceptance.md) 保留待验证状态。

## 发布

后续正式发布统一采用“本地编译并上传候选包，Actions 只做校验、公证、生成更新签名和发布”的预构建路线。唯一发布入口仍是 .github/workflows/release.yml，通过 workflow_dispatch 调用；不得默认推送 vMAJOR.MINOR.PATCH 标签去触发远端重复编译。旧标签构建入口仅保留历史兼容，除非用户明确改变发布方式，否则不使用。

GitHub 管理的六项发布 Secrets 留在 GitHub，仅由 Actions 使用；不得尝试导出到本地，不得索要其明文，不在日志、提交或终端输出 Secret 值。本地候选签名使用已安装的 Developer ID 身份，不能从 GitHub 导出证书私钥来补齐本地环境，也不要求在本地配置 Apple 公证账户或 Sparkle 私钥。script/sign_app.sh 必须保留 Installer、Downloader、Autoupdate、Updater、Framework、Helper、App 的由内到外签名顺序，不能改回 codesign --deep。

发布前确认候选源码具有完整成功 CI 和对应产品验收记录；合并导致提交 SHA 改变时，CI 与候选的 Git 源码树必须完全一致。本地 release 构建完成后，将已签名的 arm64 FanControl.app 压缩为 FanControl-VERSION-prebuilt.zip，记录 SHA-256，上传到 targetCommitish 为完整源码 SHA 的草稿 Release。调用 release.yml 时提供 release_tag、source_sha、ci_run_id、candidate_sha256；Actions 校验来源和候选后完成 App/DMG 公证、Sparkle 签名及发布，不重新编译。不要预先推送同名标签；发布草稿时由 GitHub 创建标签。正常 CI 继续负责源码验证。

发布后必须重新下载公开 DMG/appcast，核验版本、架构、签名、公证票据、哈希和更新签名，不把任务成功或本地打包当作公开产物验证。实现和本地打包请求不自动授权对外发布。

## 深入文档

README.md 负责安装、Cargo 构建、迁移状态、睡眠恢复和发布用法；SECURITY.md 负责漏洞报告与 root helper 风险；THIRD_PARTY_NOTICES.md 负责 Sparkle 与 Rust 依赖许可证；docs/rust-product-acceptance.md 负责迁移、故障恢复、UX 与真实硬件的发布门槛。
