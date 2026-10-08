# Fan Control

[![CI](https://github.com/sm-yjr/fan-control/actions/workflows/ci.yml/badge.svg)](https://github.com/sm-yjr/fan-control/actions/workflows/ci.yml)
[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](LICENSE)

Fan Control 是面向 macOS 14+、Apple Silicon 的菜单栏风扇控制工具。项目正在迁移到 Rust：热模型、控制策略、配置、Apple SMC 和 privileged helper 使用 Rust；界面通过 `objc2` 使用 AppKit 原生系统控件。主构建已经切换到 Cargo，旧 Swift 源码暂时保留作为行为和配置兼容性对照，不参与新应用构建，完成迁移验收后再移除。

当前分发使用 Rust 实现。构建成功或自动化测试通过，只证明对应代码与产物检查通过；各型号真实风扇控制、睡眠恢复与实际更新的验证范围见 [Rust 产品验收清单](docs/rust-product-acceptance.md)。迁移审查见 [独立审查报告](docs/rust-migration-review.md)，0.2.2 智能散热 hotfix 见 [0.2.2 发布说明](docs/releases/0.2.2.md)，本轮温度详情修复见 [0.2.3 发布说明](docs/releases/0.2.3.md)。下载以对应 Release 的产物和说明为准。

> [!WARNING]
> 风扇控制会直接修改硬件状态。错误的曲线可能导致过热、降频、数据丢失或硬件损坏。首次使用保持系统自动模式；选择自定义模式时保留温度余量。软件按 GPL-3.0 的无担保条款提供。

## 系统与构建要求

需要 Apple Silicon Mac、macOS 14 或更新系统、Command Line Tools，以及 Rust 1.85+ 的 Cargo、rustfmt 和 Clippy。无需完整 Xcode，也无需 Swift 编译链。先安装 CLT 和 Rust 官方工具链，再检查本机环境：

```bash
xcode-select --install
rustc --version
cargo --version
rustup component add rustfmt clippy
```

工作区分为四个 crate：

| crate | 职责 |
| --- | --- |
| `fan-core` | 与硬件无关的热模型、曲线、安全策略和配置兼容性 |
| `fan-platform` | Rust IOKit/SMC 访问、本地 Unix socket、helper 安装和控制租约 |
| `fan-app` | AppKit 菜单栏、原生控件、状态反馈和 Sparkle 运行时加载 |
| `fan-licenses` | 收集固定依赖的许可证与可校验的随包清单 |

### Apple Silicon 机型适配

0.2.2 使用 M1–M5（包括对应 Pro、Max、Ultra）的分代 CPU/GPU 温度映射，并按 SMC 实际提供的节点采样。固件 CPU 均值 `TCMb` 与最高温度 `TCMz` 分别用于持续热负载和热点保护，避免重复混入核心均值；未知芯片代际只使用已知聚合与通用节点，不按温度键前缀猜测物理身份。

| Mac 家族 | 运行行为 |
| --- | --- |
| Mac mini | 按实际风扇数量及硬件 RPM 范围控制 |
| Mac Studio | 按实际风扇数量分别控制，包含 Ultra 芯片代际 |
| MacBook Pro | 按实际风扇数量分别控制，保留睡眠前交还与唤醒恢复 |
| MacBook Air | 无内置风扇时仅监测温度，禁用风扇控制 |

这些是代码适配范围，不代表每种芯片、机型和 macOS 组合都已实机验收。Intel 不在本项目构建与支持范围内。未知或不可用传感器不冒充 CPU/GPU 温度；风扇上限、模式或采样无法确认时保留系统安全回退。

温度详情使用 AppKit 原生表格复用可见行，搜索支持语义名称与原始 SMC key。名称按芯片代际和精确 key 核对，保留尚未确认的标识；来源、冲突和展示与控制的边界见 [传感器语义说明](docs/sensor-semantics.md)。

主可执行文件仍名为 `FanControl`，以 `--helper` 启动时成为 root daemon。helper 是同一可执行文件的独立签名副本，GUI 进程按需加载 Sparkle；helper 不允许链接任何仅位于 App bundle 内的动态框架。

## 安装与权限

从 [GitHub Releases](https://github.com/sm-yjr/fan-control/releases) 下载对应版本的 DMG，把 `FanControl.app` 拖入 `Applications`。读取传感器与使用系统自动模式不应要求安装 root helper；修改风扇需要管理员授权。helper 的安装位置保持兼容：

```text
/Library/PrivilegedHelperTools/com.local.fan-control.helper
/Library/LaunchDaemons/com.local.fan-control.helper.plist
```

Rust helper 使用协议版本 8，通过同一 socket 连接协商版本，并在锁后重新验证当前用户与 peer；控制租约限定 GUI 失联后的控制期限。新 socket 位于 root 持有且无额外写权限的 `/Library/PrivilegedHelperTools/com.local.fan-control.runtime/helper.sock`。旧 socket 仅允许只读状态诊断。旧协议 helper 必须通过正常管理员授权流程更新；应用不得向不兼容 helper 发送控制请求。租约用于在 GUI 失联或异常退出后把风扇交还系统，不能替代硬件转速边界、console-user 校验和实际 SMC 状态读取。安装是否成功以服务 readiness 和协议一致为准，不能仅凭文件已复制判定。

用户配置继续保存在 `~/.config/fan-control/config.json`。迁移必须保留旧 Swift 配置中的模式、曲线、UUID 与自定义控制点；只有完全匹配旧默认值的曲线才可以升级默认策略。损坏配置和持久化失败需要可见反馈。具体兼容性与故障恢复验收见验收清单。

## 本地构建与检查

不启动 GUI 或硬件控制的入口为：

```bash
MACOSX_DEPLOYMENT_TARGET=14.0 cargo build --locked
./script/test_rust.sh
./script/package_app.sh
./script/test_update_runtime.sh
```

`test_rust.sh` 实际执行 `cargo fmt --all -- --check`、`cargo clippy --locked --workspace --all-targets -- -D warnings` 和 `cargo test --locked --workspace`，测试覆盖以当前 crate 中的用例与执行输出为准。旧 `test_*.sh` 中的 Swift 验证脚本只作为迁移对照，不再属于新应用 CI 验收入口。

`package_app.sh` 默认构建 debug 版本，固定 `MACOSX_DEPLOYMENT_TARGET=14.0`，Cargo 输出位于 `target/debug/FanControl` 或 `target/release/FanControl`。脚本只接受 Apple Silicon 本机构建，`ARCHITECTURES` 只能为空或 `arm64`；不能用此参数宣称已经支持 Intel 或 universal 构建。App 产物默认位于 `dist/FanControl.app`。

```bash
APP_VERSION=0.2.3 \
BUILD_NUMBER=203 \
BUILD_CONFIGURATION=release \
ARCHITECTURES=arm64 \
./script/package_app.sh
```

`./build.sh` 也只构建 App。需要实际启动时，使用以下入口；启动后的控制配置可能会恢复，所以应先确认测试机器与现有配置：

```bash
./script/build_and_run.sh
./script/build_and_run.sh --verify
./script/build_and_run.sh --logs
./script/build_and_run.sh --telemetry
./script/build_and_run.sh --debug
```

运行脚本只会向当前用户、当前构建路径下的 GUI 发送 SIGTERM，等待其正常交还风扇并退出；不会按 `FanControl` 名称批量终止独立 root helper。`--verify` 检查对应 GUI 进程是否存在，不能证明界面布局、硬件控制或睡眠恢复已经通过。

## 控制策略与用户体验

每个风扇支持系统自动、智能热管理、固定转速和曲线控制，CPU/GPU 温度、热负荷和分组传感器详情，以及电池与电源状态。未检测到风扇、SMC 不可用、数据过期、转速上限未知与 helper 不可用必须使用不同状态说明，避免把检测中或失联状态呈现成正常停转。

产品目标是更早识别持续发热，使高负载时的性能释放更稳定，同时允许调整键盘区域的体感目标。选择“智能热管理”并应用后，日常转速依据持续热负载：芯片温度经过 30 秒滤波、机身内部热节点经过 90 秒滤波，再结合持续 CPU 负载和 CPU/GPU 各自的升温趋势。负载或趋势需确认 4 秒，趋势预测向前看 20 秒；这些输入统一通过上升 20 秒、下降 90 秒的惯性模型，避免跟随芯片短时波动。CPU 利用率不可用时保留整体热负载与趋势策略；GPU 目前只提供温升响应，不提供利用率读数。

普通智能调节每秒最多增加 100 RPM、降低 35 RPM；停转后只先达到硬件最小稳定转速，接管时保留系统当前转速下界，再逐步调整。启动需求阈值为 12%，退出阈值为 5%；近期有效散热需求后的冷却驻留为 180 秒，还须连续低需求至少 60 秒并降到最低转速附近，才交还系统决定是否停转。持续蓄热会延长冷却，长负载结束也保留这段过程。严重/危险热压力、原始芯片温度至少 96°C 的安全保护和校准高端的保守下界仍立即响应。时间和阈值是当前控制参数，噪声、表面温度与性能效果仍需同机实测。

0.2.2 将 CPU 聚合均值与芯片热点分开计算。持续热点经 30 秒过滤后，65–95°C 映射为 0–80% 的散热下界，防止冷机身或部分低温核心稀释持续热点；原始极端温度仍独立触发紧急保护。运行中的新鲜采样出现长间隔时保留已建立热负荷，只重建短期负载和温升确认；真正睡眠恢复仍显式重置。紧急升速立即生效，紧急降档及保护解除后的降速继续渐变并保留当前安全下界。

“体感目标”默认 38°C，可在 30–45°C 内微调。校准需要外部温度计在相同键盘区域实测两个稳定状态，再同时记录内部参考传感器；内部温度必须至少相差 3°C。校准限定于本机、指定传感器及已测温度范围，界面显示“表面估计”，不会把内部传感器值标成键盘实测温度。校准缺失、机型身份不符或传感器失效时暂停体感估计与调节，继续性能与安全策略。高于校准范围时停止估计并保留校准高端的保守散热下界，防止继续升温反而降速；低于范围时停止估计并按性能策略冷却。校准不保证表面恒温，达到目标的能力还取决于室温、负载与散热硬件。固定 RPM 与曲线是独立的高级调节，体感目标仅用于智能模式。

持续性能优于系统默认及键盘表面温度改善，需要在同一机器、相同负载和环境下进行系统默认/智能模式对比，记录吞吐、频率、风扇、噪声、功耗与外部表面实测值。当前代码和模拟测试只确认策略会提前介入，尚不能证明优于 Apple 系统控制。具体实验门槛见验收清单。

控制界面应同时表达用户设置、执行进度与硬件回读状态。手动目标和曲线目标不能当作当前 RPM；“系统自动”只有在回读确认后才能作为成功状态。失败要提供可执行的恢复入口，紧急安全接管要显示原因，保留用户配置以便正常恢复。所有交互沿用 AppKit 系统 Button、Picker/Menu、Slider 和表格控件，以保留键盘与 VoiceOver 行为；曲线需要精确数值编辑、添加/删除、撤销与应用前校验。颜色和动画只作辅助信息，并遵守 Reduce Motion。

默认 `Thermal Load` 是 0–100% 的控制指标，表示持续发热与机身热浸，单位不是瓦特。它使用 CPU 聚合均值或 CPU/GPU 组平均温度、独立热点下界、机身传感器和系统热压力，过滤短时核心尖峰；缺少机身传感器时使用持续热源降级模型。曲线中的 0–100% 表示风扇硬件最小至最大 RPM 区间，停转使用独立语义。自定义配置不能绕过系统热压力、极端芯片温度、硬件上下限或采样过期保护。

## 睡眠、退出与恢复

真实系统睡眠前必须完成有界的风扇交还，然后确认电源事件。唤醒后保留多个时点的恢复重试，并读取实际 SMC 模式；硬件可能在睡眠中重置强制位，不能只信进程缓存。正常退出、注销、helper 更新和卸载也要先交还系统；GUI 异常退出由 helper 租约处理。删除 App 文件不等于已移除系统 helper。

恢复问题的诊断应包括 App/helper 版本、协议、采样时间、用户模式、观察到的 SMC 模式和请求结果。日志对外分享前删除用户名、主目录路径及其他个人信息。真实睡眠—唤醒、GUI 崩溃与断连恢复须在明确测试机器上验收，模拟测试不能替代这一步。

## Sparkle、签名与发布

Sparkle 仍固定为 2.9.2，下载源和 SHA-256 在 `script/package_app.sh` 中。构建脚本从官方发行包下载，校验后复制 framework 与原始许可证。`test_update_runtime.sh` 通过 `--check-updater-runtime` 检查本地打包 App 中的 Sparkle 实际加载；该诊断模式不应初始化传感器、helper 或风扇控制。

稳定更新源保持为：

```text
https://github.com/sm-yjr/fan-control/releases/latest/download/appcast.xml
```

正式发布只通过 `.github/workflows/release.yml`，由 `vMAJOR.MINOR.PATCH` 标签触发。工作流执行 Rust 检查、arm64 打包、Developer ID 签名、App 与 DMG 公证/装订、Gatekeeper 验证及 Sparkle Ed25519 appcast，再创建 GitHub Release。发布前需要确认 main CI、验收清单和以下六个 Actions Secrets；不能在日志、提交或终端输出 Secret 值：

```text
MACOS_CERTIFICATE_P12_BASE64
MACOS_CERTIFICATE_PASSWORD
APPLE_ID
APPLE_TEAM_ID
APPLE_APP_SPECIFIC_PASSWORD
SPARKLE_PRIVATE_KEY
```

`sign_app.sh` 保留 Installer、Downloader、Autoupdate、Updater、Framework、Helper、App 的由内到外签名顺序。正式 Developer ID 构建启用 Hardened Runtime；本地 ad-hoc 构建采用现有开发签名策略。`codesign --verify --deep --strict` 用于验证，不能把签名过程改成 `codesign --deep`。

DMG 的本地构建检查不需要 Apple 公证账户：

```bash
./script/package_dmg.sh \
  dist/FanControl.app \
  dist/FanControl-0.2.3.dmg \
  "Fan Control 0.2.3"
hdiutil verify dist/FanControl-0.2.3.dmg
```

本地 ad-hoc 签名和 DMG 校验不能证明网络下载后的 Gatekeeper 接受或正式更新安装已经通过；正式候选包必须重新完成这些验收。

## 许可证

Fan Control 采用 [GNU General Public License v3.0 only](LICENSE)。Sparkle 与 Rust 依赖的许可证说明见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
