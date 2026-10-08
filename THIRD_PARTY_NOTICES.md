# 第三方许可证与版权声明

Fan Control 使用 GPL-3.0-only。第三方组件保留各自许可证和版权声明，完整 Rust 依赖版本以 `Cargo.lock` 为准。

## 发布包中的材料

打包脚本使用 `cargo metadata --locked --format-version 1` 生成完整解析依赖清单，再运行本项目的纯 Rust 工具 `fan-licenses`。工具收集所有 registry 依赖，包括传递依赖和构建工具依赖，逐字节保留 crate 中的 `LICENSE`、`LICENCE`、`COPYING`、`UNLICENSE`、`COPYRIGHT`、`NOTICE` 文本及 manifest 明确声明的 `license_file`；多份许可证和子目录中的声明都会保留。Cargo 字段含义见 [Cargo metadata 文档](https://doc.rust-lang.org/cargo/commands/cargo-metadata.html)。

材料写入 `FanControl.app/Contents/Resources/ThirdPartyLicenses/<crate>/<version>/`，同目录的 `manifest.json` 记录 crate、版本、完整许可证表达式、审核过的标识、文件相对路径及 SHA-256。清单不包含开发机绝对路径。工具不修改原文，不跟随 symlink，不允许 `license_file` 越过 crate 目录；缺少许可证声明、缺少原始文件、未知表达式、原文 hash 不符都会使打包失败。工具本身不会进入应用包。

[Sparkle](https://github.com/sparkle-project/Sparkle) 用于检查和安装更新，采用 MIT License。打包脚本从固定版本和 SHA-256 的官方发行包复制原始许可证到 `Resources/Sparkle-LICENSE.txt`。应用自身的 GPL 原文保存在 `Resources/FanControl-LICENSE.txt`。

## 已审核的 Rust 许可证集合

Apple Silicon M1–M5 的 CPU/GPU 温度键与分组表参考 [Stats](https://github.com/exelban/stats) 的 MIT 源码，固定于提交 `ee4265f3b9afdffebd3273cf6a83b9327ead45b5` 的 `Modules/Sensors/values.swift`。本项目仅持有精确键与分组表，按芯片代际选择，并避免沿用可能不匹配实际机器的核心编号。上游 MIT 原文与版权声明保存在 `crates/fan-platform/licenses/Stats-LICENSE.txt`，发布包随附 `Resources/Stats-LICENSE.txt`。固件聚合节点及补充诊断名称参考 [iSMC 的传感器表](https://github.com/dkorunic/iSMC/blob/6b107a39b5759fc9ac510291dff0f6f13e61b388/smc/sensors.go)，固定提交 `6b107a39b5759fc9ac510291dff0f6f13e61b388`。iSMC 源表标注 `GPL-3.0-only`、`Copyright (C) 2019 Dinko Korunic`，与本项目的 GPL-3.0-only 相容。适配后的中文精确键表在源码中注明原作者、来源和修改，GPLv3 原始许可保存在 `crates/fan-platform/licenses/iSMC-LICENSE.txt`，发布包随附 `Resources/iSMC-LICENSE.txt`。精确键、代际限制、来源分歧与只用于展示的边界见 [传感器语义说明](docs/sensor-semantics.md)。


当前允许的 SPDX 标识仅为 `MIT`、`Apache-2.0`、`Zlib`、`BSD-2-Clause`、`BSD-3-Clause`、`ISC`、`Unlicense`、`Unicode-3.0`。集合按本项目 GPL-3.0-only 的发行要求审核；Apache-2.0 的兼容判断以 GPLv3 为前提。兼容性依据见 [GNU 许可证说明](https://www.gnu.org/licenses/license-list.html)。

`Unlicense` 包含公共领域贡献及允许自由使用的补充许可，GNU 明确说明其与 GPL 兼容。`Unicode-3.0` 保留版权/许可声明和禁止以权利人名称做未经授权宣传的条件，GNU 明确将该版本列为与 GPL 兼容的宽松许可证；Unicode 官方也说明其基于 MIT，并区分它与 Unicode Terms of Use。[GNU Unlicense 说明](https://www.gnu.org/licenses/license-list.html#Unlicense)、[GNU Unicode v3 说明](https://www.gnu.org/licenses/license-list.html#Unicodev3)、[Unicode 官方许可政策](https://unicode.org/policies/licensing_policy.html)、[SPDX Unicode-3.0 原文](https://spdx.org/licenses/Unicode-3.0.html)。

工具完整解析括号以及 `AND`/`OR`，`AND` 的优先级高于 `OR`，并检查表达式中的每个标识。例如 `Unlicense OR MIT` 的两个许可都会检查；`(MIT OR Apache-2.0) AND Unicode-3.0` 的 Unicode 条件也必须通过审核。任何分支中出现未知标识都会失败。`WITH` 例外、自定义 `LicenseRef`、旧式斜线写法和其他新标识均需要维护者先审核并修改工具；不能仅因 Cargo 中存在 `license` 字符串就视为通过。[SPDX 表达式规则](https://spdx.github.io/spdx-spec/v2.3/SPDX-license-expressions/)。

一个明确审核的历史例外是 `version_check 0.9.5`：其声明为 `MIT/Apache-2.0`，发布提交的 [原始 README](https://github.com/SergioBenitez/version_check/blob/d77ef9f27cc336719b2d839d09ee6635dd22f758/README.md#license) 明确表示两者任选其一，两份完整原文也在 crate 归档中。`sources.json` 对该准确版本、原声明及提交记录 `MIT OR Apache-2.0` 的审核表达式，并固定 README 原文/hash。清单同时保留原声明和审核表达式，上游证据另存为 `UPSTREAM-DECLARATION.md`；其他斜线表达式仍拒绝。[Cargo 文档](https://doc.rust-lang.org/cargo/reference/manifest.html#the-license-and-license-file-fields) 也说明斜线分隔属于已弃用的历史格式。

## objc2 上游原文的固定补充

当前 `objc2`、`block2`、`dispatch2` 及 objc2 framework crates 的 crates.io 归档没有随附许可证原文。`crates/fan-licenses/licenses/sources.json` 对每个准确 crate/版本记录发布包 `.cargo_vcs_info.json` 的提交 SHA、[官方 objc2 仓库](https://github.com/madsmtm/objc2) 中该提交的 `LICENSE.md` 来源 URL，以及保存原文的 SHA-256。工具同时匹配 crate、版本、许可声明和提交，校验 vendored 原文的 hash；升级后的版本不能借用旧记录，也没有通用 fallback。

这些提交的 `LICENSE.md` 说明四个历史 crate 使用 MIT，其余 crate 可选择 Zlib、Apache-2.0 或 MIT，并链接至许可正文。发布材料保留完整原文及它链接的 [MIT 官方许可页面](https://opensource.org/license/MIT)、[Apache-2.0 官方正文](https://www.apache.org/licenses/LICENSE-2.0.txt)、[Zlib 官方许可页面](https://zlib.net/zlib_license.html) 的固定副本；参考文本另标为 `*-reference.*`，不会伪造上游未提供的版权署名。MIT-only crate 随附 MIT 参考文档，三选一 crate 随附三份参考文档。所有来源和 hash 同样进入材料清单。

上游 `LICENSE.md` 还讨论了 Apple SDK 派生声明的范围。这段原文随包保留；本工具验证许可证声明和随附材料，不替代对该上游问题的独立发行审查。新增或升级依赖时，必须重新检查声明、原文和来源。签名、公证及最终候选包验收仍见 [Rust 产品验收清单](docs/rust-product-acceptance.md)。

## 独立验证

```bash
cargo test --locked --package fan-licenses
cargo clippy --locked --package fan-licenses --all-targets -- -D warnings
mkdir -p .build
cargo metadata --locked --format-version 1 > .build/license-metadata.json
cargo run --locked --package fan-licenses -- \
  --metadata .build/license-metadata.json \
  --output .build/ThirdPartyLicenses
```

输出目录必须不存在或为空，以避免覆盖已有材料。测试覆盖完整表达式、未知许可拒绝、路径越界、symlink、缺失原文、精确上游版本/提交/hash、原文逐字节复制以及清单不泄露开发机路径。实际打包必须通过同一收集流程，不能用单元测试替代候选包检查。
