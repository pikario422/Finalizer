# Snapdragon 8e Default Config Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the Magisk package's default Finalizer configuration with the approved Snapdragon 8e dual-policy configuration.

**Architecture:** This is a data-only change to the package payload. The Rust scheduler already maps each top-level `[[policy]]` to the policy entry at the same index in every mode, so the new file defines two top-level clusters and two entries in each of the four modes.

**Tech Stack:** TOML configuration, PowerShell static validation, Git diff verification

## Global Constraints

- Modify only `mode/config/config.toml` as product behavior.
- Keep `debug/config.toml`, Rust source, build scripts, and all other package files unchanged.
- Preserve the approved field values, comments, and ordering exactly.
- Do not rebuild the binary or Magisk ZIP.

---

### Task 1: Replace and validate the packaged default configuration

**Files:**
- Modify: `mode/config/config.toml`
- Test: static assertions against `mode/config/config.toml`

**Interfaces:**
- Consumes: Finalizer's existing `Config` TOML schema from `src/config/data.rs`
- Produces: two top-level CPU policies and two policy entries for each of `power`, `blan`, `perf`, and `fast`

- [x] **Step 1: Capture the pre-change validation failure**

Run:

```powershell
$text = Get-Content -Raw -Encoding UTF8 mode/config/config.toml
if (($text | Select-String -AllMatches 'governor = "walt"').Matches.Count -ne 8) { throw 'Expected the old configuration to fail the SD8e governor assertion' }
```

Expected: command throws because the old configuration contains zero `walt` governor entries.

- [x] **Step 2: Replace the complete configuration**

Set `mode/config/config.toml` to:

```toml
# 骁龙8e 真机适配 Finalizer 完整调度配置
# 硬件采集信息：
# CPU核心 0-7 共8核
# cpufreq 仅存在 policy0、policy6
# policy0: cpu0~5 大小核集群
# policy6: cpu6~7 X2超大核集群
# 可用频点(kHz):
# 384000 556800 748800 960000 1152000 1363200
# 1555200 1785600 1996800 2227200 2400000
# 2745600 2918400 3072000 3321600 3532800
[name]
name = "SD8e"
version = "1.0"
author = "pikario"

# 内核调频集群绑定，匹配本机双policy架构
[[policy]]
from = 0
to = 5

[[policy]]
from = 6
to = 7

# ---------------------------
# Power 省电模式（待机/社交/轻度浏览）
# ---------------------------
[mode.power]
idle_governor = "menu"

# policy0 0~5 中小核策略
[[mode.power.policy]]
delay = 600
max_freq = 2400000
min_freq = 384000
can_boost_freq = 1000000
boost_freq = 1996800
margin = 2.1
diff = 70000
governor = "walt"
sleep_freq = 960000

# policy6 6~7 X2超大核策略
[[mode.power.policy]]
delay = 600
max_freq = 2400000
min_freq = 384000
can_boost_freq = 1000000
boost_freq = 1996800
margin = 2.0
diff = 70000
governor = "walt"
sleep_freq = 960000

# ---------------------------
# Blan 均衡模式（日常游戏/多任务）
# ---------------------------
[mode.blan]
idle_governor = "menu"

# policy0 0~5 中小核策略
[[mode.blan.policy]]
delay = 400
max_freq = 2918400
min_freq = 384000
can_boost_freq = 1000000
boost_freq = 2400000
margin = 2.3
diff = 70000
governor = "walt"
sleep_freq = 960000

# policy6 6~7 X2超大核策略
[[mode.blan.policy]]
delay = 400
max_freq = 3072000
min_freq = 384000
can_boost_freq = 1000000
boost_freq = 2400000
margin = 2.3
diff = 70000
governor = "walt"
sleep_freq = 960000

# ---------------------------
# Perf 性能模式（高画质手游/剪辑/重度负载）
# ---------------------------
[mode.perf]
idle_governor = "menu"

# policy0 0~5 中小核策略
[[mode.perf.policy]]
delay = 200
max_freq = 3321600
min_freq = 384000
can_boost_freq = 1000000
boost_freq = 2745600
margin = 2.8
diff = 70000
governor = "walt"
sleep_freq = 960000

# policy6 6~7 X2超大核策略
[[mode.perf.policy]]
delay = 200
max_freq = 3532800
min_freq = 384000
can_boost_freq = 1000000
boost_freq = 2745600
margin = 2.8
diff = 70000
governor = "walt"
sleep_freq = 960000

# ---------------------------
# Fast 极限模式（电竞/跑分/极致帧率）
# ---------------------------
[mode.fast]
idle_governor = "menu"

# policy0 0~5 中小核策略
[[mode.fast.policy]]
delay = 100
max_freq = 3321600
min_freq = 384000
can_boost_freq = 1000000
boost_freq = 2745600
margin = 3.0
diff = 70000
governor = "walt"
sleep_freq = 960000

# policy6 6~7 X2超大核策略
[[mode.fast.policy]]
delay = 100
max_freq = 3532800
min_freq = 384000
can_boost_freq = 1000000
boost_freq = 2745600
margin = 3.0
diff = 70000
governor = "walt"
sleep_freq = 960000
```

- [x] **Step 3: Run structural assertions**

Run:

```powershell
$text = Get-Content -Raw -Encoding UTF8 mode/config/config.toml
$checks = @{
  top_level_policy = ($text | Select-String -AllMatches '(?m)^\[\[policy\]\]$').Matches.Count
  power_policy = ($text | Select-String -AllMatches '(?m)^\[\[mode\.power\.policy\]\]$').Matches.Count
  blan_policy = ($text | Select-String -AllMatches '(?m)^\[\[mode\.blan\.policy\]\]$').Matches.Count
  perf_policy = ($text | Select-String -AllMatches '(?m)^\[\[mode\.perf\.policy\]\]$').Matches.Count
  fast_policy = ($text | Select-String -AllMatches '(?m)^\[\[mode\.fast\.policy\]\]$').Matches.Count
  walt = ($text | Select-String -AllMatches '(?m)^governor = "walt"$').Matches.Count
}
if ($checks.top_level_policy -ne 2 -or $checks.power_policy -ne 2 -or $checks.blan_policy -ne 2 -or $checks.perf_policy -ne 2 -or $checks.fast_policy -ne 2 -or $checks.walt -ne 8) { throw ($checks | ConvertTo-Json -Compress) }
$checks
```

Expected: every policy count is `2`, and `walt` is `8`.

- [x] **Step 4: Verify frequency bounds and change scope**

Run:

```powershell
$lines = Get-Content -Encoding UTF8 mode/config/config.toml
$mins = @($lines | Where-Object { $_ -match '^min_freq = (\d+)$' } | ForEach-Object { [int]$Matches[1] })
$maxs = @($lines | Where-Object { $_ -match '^max_freq = (\d+)$' } | ForEach-Object { [int]$Matches[1] })
for ($i = 0; $i -lt $mins.Count; $i++) { if ($mins[$i] -gt $maxs[$i]) { throw "Invalid frequency bounds at policy index $i" } }
git diff --check
git status --short
```

Expected: no exception, `git diff --check` emits no output, and Git status lists `mode/config/config.toml` plus this implementation plan only.

- [x] **Step 5: Commit the change**

```powershell
git add -- mode/config/config.toml docs/superpowers/plans/2026-07-26-sd8e-default-config.md
git commit -m "config: adapt default profile for Snapdragon 8e"
```

Expected: one commit containing the approved default configuration and its implementation plan.
