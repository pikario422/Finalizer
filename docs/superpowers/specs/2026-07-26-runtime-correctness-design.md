# Finalizer 运行时正确性修复设计

## 目标

修复 Finalizer 在 Android 真机上的 CPUFreq 重复写入、升降频顺序、游戏白名单、屏幕状态和模式切换问题，使 SD8e 双 policy 配置能够持续、可预测地工作。

本阶段沿用现有多线程加 `mpsc` channel 架构，不引入异步运行时，也不重构为全局 actor。

## 范围

本阶段包含：

- CPUFreq 文件重复读写和上下限写入顺序。
- 硬件频率边界读取及白名单恢复行为。
- CPU 动态调频计算基准。
- 模式、游戏和屏幕之间的状态转换。
- Touch Boost 的屏幕约束和触发时序。
- 配置结构与频率边界校验。
- 可恢复的启动错误处理和相关单元测试。

本阶段不包含：

- `logger` 绝对路径依赖。
- `build.sh` 和交叉编译流程。
- sysfs `0666` 权限策略。
- `/dev/fas_rs` 模式路由。
- Magisk ZIP 生成或恢复。

## CPUFreq 写入

`Policy` 保留已打开的 sysfs 文件，但所有读写都必须先 seek 到 offset 0。`read_min` 必须 seek `min_freq`，不能操作 `max_freq`。

写入 `(target_min, target_max)` 前先校验 `target_min <= target_max`，然后读取当前上下限并生成安全顺序：

1. 如果 `target_max > current_max`，先写 max，为可能提高的 min 腾出合法范围。
2. 写入发生变化的 min。
3. 如果 max 尚未写入且发生变化，最后写 max。

这个顺序覆盖同时升高、同时降低和交叉收窄区间，并保证任何中间状态都满足 `min <= max`。未变化的字段不写入。

写入顺序计算提取为纯函数，以便单元测试覆盖升频、降频、仅修改一侧和非法目标。

每个 policy 初始化时读取 `cpuinfo_min_freq` 和 `cpuinfo_max_freq`，保存为硬件边界。游戏白名单恢复使用这些边界，不使用魔法数字。

## 调频算法

目标频率按以下方式计算：

```text
target = mode_policy.max_freq * cpu_load * margin
target = clamp(target, mode_policy.min_freq, mode_policy.max_freq)
```

当前 `scaling_max_freq` 仅用于与目标值计算 `diff`，不再作为下一轮目标的乘法基准。这样熄屏或低负载降低上限后，系统仍可按负载恢复至当前模式允许的最高频率。

## Manager 事件

Manager 继续作为唯一 sysfs 写入者。事件扩展为以下语义：

- 设置单个 policy 的频率范围。
- 设置单个 policy 的 governor。
- 应用指定模式：为每个顶层 policy 使用同索引的模式 policy，设置 governor 和该模式的初始频率范围，并设置 idle governor。
- 恢复硬件范围：所有 policy 写入各自硬件最小、最大频率。
- 应用休眠范围：按当前模式写入每个 policy 的 `min_freq` 和 `sleep_freq`。

模式 policy 与顶层 policy 必须按索引一一对应，不再使用笛卡尔积循环。`fast` 必须使用 `mode.fast.policy`。

## 状态转换

共享状态仍使用 `AtomicUsize` 和 `AtomicBool`：

- `mode` 表示用户最后选择的模式。
- `onf` 表示当前屏幕是否点亮。
- `is_game` 表示前台应用是否命中白名单。

### 启动

模式线程启动后立即读取 `config.txt`，解析并保存模式，不等待第一次 inotify 修改。屏幕线程立即读取真实屏幕状态并同步 `onf`。

启动状态应用规则：

- 屏幕关闭：应用休眠范围。
- 屏幕开启且前台是白名单应用：恢复硬件范围。
- 屏幕开启且非白名单应用：应用当前模式。

### 模式变化

- 始终读取并保存合法的新模式。
- 屏幕开启且非游戏时，立即发送“应用模式”。
- 游戏期间或熄屏时只保存选择，不覆盖硬件范围；退出游戏或亮屏时应用最新模式。
- 非法模式记录警告并保持原模式，不覆盖 `mode_temp`。

### 游戏变化

线程启动后立即检查一次前台应用，之后每轮间隔五秒。每轮先使用整个白名单执行一次匹配，再与旧状态比较：

- 非游戏进入游戏：设置 `is_game=true`；若屏幕开启，恢复硬件范围。
- 游戏退出：设置 `is_game=false`；若屏幕开启，应用当前最新模式。
- 状态未变化：不发送重复事件、不重复记录进入/退出日志。

### 屏幕变化

- 亮屏转熄屏：设置 `onf=false`，应用当前模式的休眠范围。
- 熄屏转亮屏：设置 `onf=true`；若 `is_game=true` 则恢复硬件范围，否则应用当前模式。
- 首次采样也执行对应动作，避免初始布尔值与真实屏幕状态不一致。

## Touch Boost

Touch Monitor 接收 `onf`：只有屏幕开启且非游戏时才允许 Boost。

识别到触摸后立即检查并发送 Boost，300ms 用作发送后的冷却时间，不再作为触发前延迟。本阶段保持现有触摸设备发现和事件读取方式，不扩展为精确手势识别。

找不到或无法打开触摸设备时记录错误并禁用 Touch Boost，CPU 动态调度、模式和屏幕监控继续运行。

## 配置校验

启动线程前执行一次校验：

- 顶层 policy 非空，且每项满足 `from <= to`。
- CPU 范围不能重叠。
- 四种模式的 policy 数量必须与顶层 policy 数量相同。
- 每项满足 `min_freq <= max_freq`。
- `boost_freq` 和 `sleep_freq` 必须位于 `[min_freq, max_freq]`。
- `can_boost_freq <= boost_freq`。
- `delay > 0`，`margin` 必须为有限正数。

校验失败时输出可定位到模式和 policy 索引的错误，并在创建硬件线程前终止启动。

## 错误处理

- 配置读取和解析改为返回 `Result`，由 `main` 记录并终止。
- 缺少配置声明的 cpufreq policy 属于不可恢复错误，启动失败。
- 缺少触摸设备属于可恢复错误，仅关闭 Boost。
- 线程创建失败必须记录错误；本阶段不实现运行中线程 watchdog。
- Manager 记录实际 sysfs 执行结果，生产者不得把 channel 入队成功记录为硬件操作成功。

## 测试

增加不依赖 Android sysfs 的单元测试：

- 升频、降频、单侧变化和非法范围的写入顺序。
- 同一文件连续写入会覆盖旧值，证明每次写前重置偏移。
- `read_min` 连续读取正确文件。
- 合法 SD8e 配置及每类非法配置校验。
- 白名单多条目匹配与未匹配。
- 模式字符串解析和模式 policy 索引映射。
- 游戏、屏幕、模式关键状态转换所产生的高级事件。

Android 真机回归重点验证：连续调频不出现 `WriteZero/EINVAL`、Touch Boost 能升频、游戏熄屏再亮屏恢复硬件范围、游戏期间切换模式在退出后生效。

当前 Windows 环境没有可用 Cargo；实施阶段先检查是否存在可用 WSL/容器 Rust 工具链。若仍不可用，必须保留测试代码并明确报告未执行项，不能声称编译或测试通过。
