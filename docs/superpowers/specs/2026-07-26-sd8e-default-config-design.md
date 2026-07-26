# 骁龙 8e 默认配置变更设计

## 目标

将 Magisk 安装包内的默认调度配置替换为用户提供的骁龙 8e 真机配置，使新安装的 Finalizer 默认匹配该设备的双 cpufreq policy 架构。

## 变更范围

- 仅替换 `mode/config/config.toml`。
- 保持 `debug/config.toml`、Rust 源码、构建脚本和其他安装包文件不变。
- 配置内容严格采用用户提供的字段、数值、注释和排列顺序。

## 配置结构

- 设备包含 8 个 CPU 核心。
- `policy0` 覆盖 CPU 0 至 CPU 5。
- `policy6` 覆盖 CPU 6 至 CPU 7。
- `power`、`blan`、`perf`、`fast` 四种模式各包含两个 policy 配置，并统一使用 `walt` governor 和 `menu` idle governor。

## 验证

替换完成后执行以下静态检查：

1. 确认只有 `mode/config/config.toml` 发生产品配置变更。
2. 确认顶层包含两个 `[[policy]]`。
3. 确认四种模式各包含两个 policy 条目。
4. 确认每个策略的 `min_freq` 不大于 `max_freq`，且频率值来自用户提供的频点集合。
5. 检查 Git diff，确保内容与用户给出的配置一致。

## 非目标

- 不修复现有 Rust 调度逻辑。
- 不修改调试配置。
- 不重新构建二进制或 Magisk ZIP。
