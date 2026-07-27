# Finalizer

项目简介
--------

Finalizer 是一个用 Rust 编写的轻量守护进程/工具，面向 Android 设备，用于基于配置管理设备的调度与电源策略（CPU 频率、调度模式、设备输入处理等）。代码通过模块化的配置和模式切换，支持在运行时调整策略以优化性能和功耗。

主要特性
--------
- 触屏Boost
- 动态调节CPU频率
- manager管理事件
- 运行时调整模式
- 熄屏停止大部分工作
- 白名单应用关闭
- WebUI 管理运行模式、日志和配置文件


贡献
--------

- 欢迎通过 Issue 报告问题或通过 Pull Request 提交改进。
- 请在提交前确保代码通过本地构建并遵循现有代码风格。

许可
--------

本项目使用仓库根目录下的 `LICENSE` 文件中指定的许可证。

联系
--------

如需更多信息，请在仓库中创建 Issue 或联系维护者（仓库 README/元数据中可能含有维护者信息）。

WebUI
--------

在支持 KernelSU WebUI bridge 的模块管理器中打开 Finalizer，可使用以下功能：

- 查看运行状态并切换省电、均衡、性能和极速模式。
- 调整日志级别，查看、筛选、暂停或清空实时日志。
- 在“调度配置”Tab 中编辑 `config.toml`，可选择保存或保存并重启。
- 在“游戏配置”Tab 中编辑 `game_list.toml`；保存后会在下一次轮询时自动热重载，通常不超过 5 秒，无需重启 Finalizer。

WebUI 保存配置前会调用 Finalizer 校验 TOML 内容。校验失败时不会覆盖当前配置；保存成功前，原文件会分别备份为 `config.toml.webui.bak` 或 `game_list.toml.webui.bak`。

配置字段说明
--------

下面基于 `debug/config_bak.toml` 示例，列出常用字段及注释：

- `name.name`：配置名称（示例：`8100_MAX`）。
- `name.version`：配置版本（示例：`2.0`）。
- `name.author`：配置作者（示例：`ShenEternity`）。

- `[[policy]]`：CPU 分组策略列表；每个 `[[policy]]` 表示一组 CPU id 范围，列表项数量决定分组数量。
	- `from`：该分组起始 CPU id（示例：`0`），表示此策略适用于从该 id 开始的 CPU。 注释: "cpu_id 第一个"。
	- `to`：该分组结束 CPU id（示例：`3`），表示此策略适用于到该 id 为止的 CPU。 注释: "cpu_id 最后一个"。程序会自动补充中间的 CPU id。

- `[mode]`：模式集合顶层表，包含不同运行模式（例如 `power`、`blan`、`perf`、`fast`）。
- `mode.<mode_name>.idle_governor`：空闲时使用的 governor 名称（示例：`menu`）。注释提示：如果不确定可使用默认值。
- `mode.<mode_name>.policy`：某一模式下的策略列表；每个策略字段含义如下：
	- `delay`：轮询延迟（单位：毫秒），控制采样/调节的间隔。 注释: "轮训延迟"。
	- `max_freq`：该策略允许的最高频率（单位：Hz）。 注释: "当前 policy 的最高频率"。
	- `min_freq`：该策略允许的最低频率（单位：Hz）。 注释: "当前 policy 的最低频率"。
	- `can_boost_freq`：允许 boost 的最低频率阈值（单位：Hz）；当当前频率低于此值时才允许进入 boost 状态。 注释: "允许boost的频率 当频率地狱这个值时允许boost"。
	- `boost_freq`：突发/boost 时使用的频率（单位：Hz）。 注释: "boost 频率"。
	- `margin`：冗余倍率（浮点），越高表示调频越激进，优先级更高。 注释: "冗余倍率,越高调频越激进,低则相反,最高优先级"。
	- `diff`：触发调频的阈值（单位：Hz）；当当前频率与计算出的目标频率差值大于此值时才会执行调频。 注释: "调频的差值 当前频率 与 计算出的频率 的差值高于这个值才会调频"。
	- `governor`：为该策略选择的 governor（示例：`sugov_ext`）。 注释: "调速器"。
	- `sleep_freq`：熄屏/休眠状态时的目标频率（单位：Hz）。 注释: "熄屏时的频率"。

示例中不同模式的 `delay` 值依次为：`power`=600，其他模式可设更小值以获得更快响应。
有关完整示例，请参阅仓库内的 `debug/config_bak.toml` 示例配置。

游戏列表配置
--------

`game_list.toml` 使用 `[[listvalue]]` 定义游戏或需要特殊策略的应用：

- `pkg`：应用包名，不能为空；当前前台窗口包含该值时视为匹配。
- `name`：用于日志显示的应用名称。
- `mode`：可选，指定应用匹配时使用的策略。支持 `powersave`、`balance`、`performance`、`fast` 和 `hardware`；省略时等同于 `hardware`，即恢复硬件频率范围。

示例：

```toml
[[listvalue]]
pkg = "com.example.game"
name = "示例游戏"
mode = "performance"
```

运行中的 Finalizer 每 5 秒检查一次 `game_list.toml` 是否变化。新文件解析或校验失败时会继续使用上一份有效配置，并在日志中记录错误。

