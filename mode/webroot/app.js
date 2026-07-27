const MODULE_ID = "SZE_FINALIZER";
const API_PATH = `/data/adb/modules/${MODULE_ID}/webroot/api.sh`;
const MODE_LABELS = {
  powersave: "省电",
  balance: "均衡",
  performance: "性能",
  fast: "极速",
};

const elements = {
  runtimeBadge: document.querySelector("#runtimeBadge"),
  runtimeText: document.querySelector("#runtimeText"),
  currentMode: document.querySelector("#currentMode"),
  currentLevel: document.querySelector("#currentLevel"),
  logSize: document.querySelector("#logSize"),
  modeControl: document.querySelector("#modeControl"),
  modeFeedback: document.querySelector("#modeFeedback"),
  logLevel: document.querySelector("#logLevel"),
  logFilter: document.querySelector("#logFilter"),
  pauseLog: document.querySelector("#pauseLog"),
  clearLog: document.querySelector("#clearLog"),
  logViewport: document.querySelector("#logViewport"),
  autoScroll: document.querySelector("#autoScroll"),
  configTabs: Array.from(document.querySelectorAll(".config-tab")),
  schedulerConfigPanel: document.querySelector("#schedulerConfigPanel"),
  gameListPanel: document.querySelector("#gameListPanel"),
  configEditor: document.querySelector("#configEditor"),
  configState: document.querySelector("#configState"),
  reloadConfig: document.querySelector("#reloadConfig"),
  saveConfig: document.querySelector("#saveConfig"),
  saveRestart: document.querySelector("#saveRestart"),
  gameListEditor: document.querySelector("#gameListEditor"),
  gameListState: document.querySelector("#gameListState"),
  reloadGameList: document.querySelector("#reloadGameList"),
  saveGameList: document.querySelector("#saveGameList"),
  toast: document.querySelector("#toast"),
};

let callbackSequence = 0;
let toastTimer;
let logPaused = false;
let latestLog = "";
let configDirty = false;
let gameListDirty = false;
let statusBusy = false;
let logBusy = false;

function hasKernelSuBridge() {
  return Boolean(window.ksu && typeof window.ksu.exec === "function");
}

function exec(command) {
  return new Promise((resolve, reject) => {
    if (!hasKernelSuBridge()) {
      reject(new Error("KernelSU WebUI bridge 不可用"));
      return;
    }

    const callbackName = `finalizer_exec_${Date.now()}_${callbackSequence++}`;
    const timeout = window.setTimeout(() => {
      delete window[callbackName];
      reject(new Error("命令执行超时"));
    }, 15000);

    window[callbackName] = (errno, stdout, stderr) => {
      window.clearTimeout(timeout);
      delete window[callbackName];
      const result = {
        errno: Number(errno),
        stdout: String(stdout ?? ""),
        stderr: String(stderr ?? ""),
      };
      if (result.errno === 0) {
        resolve(result);
      } else {
        reject(new Error(result.stderr.trim() || result.stdout.trim() || `命令失败 (${result.errno})`));
      }
    };

    try {
      window.ksu.exec(command, "{}", callbackName);
    } catch (error) {
      window.clearTimeout(timeout);
      delete window[callbackName];
      reject(error);
    }
  });
}

function shellQuote(value) {
  return `'${String(value).replaceAll("'", `'\\''`)}'`;
}

async function runApi(action, argument) {
  const parts = ["/system/bin/sh", shellQuote(API_PATH), shellQuote(action)];
  if (argument !== undefined) {
    parts.push(shellQuote(argument));
  }
  return exec(parts.join(" "));
}

function showToast(message) {
  elements.toast.textContent = message;
  elements.toast.classList.add("visible");
  window.clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => elements.toast.classList.remove("visible"), 2600);
  if (window.ksu && typeof window.ksu.toast === "function") {
    window.ksu.toast(message);
  }
}

function parseKeyValue(text) {
  return Object.fromEntries(
    text
      .split(/\r?\n/)
      .filter((line) => line.includes("="))
      .map((line) => {
        const separator = line.indexOf("=");
        return [line.slice(0, separator), line.slice(separator + 1)];
      }),
  );
}

function formatBytes(rawValue) {
  const bytes = Number(rawValue);
  if (!Number.isFinite(bytes) || bytes < 0) return "--";
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function setControlsDisabled(disabled) {
  elements.modeControl.querySelectorAll("button").forEach((button) => {
    button.disabled = disabled;
  });
  elements.logLevel.disabled = disabled;
}

function renderStatus(status) {
  const mode = status.mode || "unknown";
  const running = status.running === "1";
  elements.currentMode.textContent = MODE_LABELS[mode] || mode;
  elements.currentLevel.textContent = status.log_level || "--";
  elements.logSize.textContent = formatBytes(status.log_bytes);
  elements.runtimeBadge.dataset.state = running ? "running" : "stopped";
  elements.runtimeText.textContent = running ? "运行中" : "已停止";
  elements.logLevel.value = status.log_level || "info";
  elements.modeControl.querySelectorAll("button").forEach((button) => {
    button.setAttribute("aria-pressed", String(button.dataset.mode === mode));
  });
}

async function refreshStatus() {
  if (statusBusy || document.hidden) return;
  statusBusy = true;
  try {
    const result = await runApi("status");
    renderStatus(parseKeyValue(result.stdout));
  } catch (error) {
    elements.runtimeBadge.dataset.state = "error";
    elements.runtimeText.textContent = "连接失败";
  } finally {
    statusBusy = false;
  }
}

function detectLogLevel(line) {
  const match = line.match(/\[(ERROR|WARN|INFO|DEBUG)\]/);
  return match ? match[1].toLowerCase() : "other";
}

function renderLog() {
  const selectedLevel = elements.logFilter.value;
  const lines = latestLog.split(/\r?\n/).filter(Boolean);
  const visibleLines = selectedLevel === "all"
    ? lines
    : lines.filter((line) => detectLogLevel(line) === selectedLevel);

  elements.logViewport.replaceChildren();
  if (visibleLines.length === 0) {
    const empty = document.createElement("div");
    empty.className = "empty-state";
    empty.textContent = lines.length === 0 ? "暂无日志" : "没有匹配的日志";
    elements.logViewport.append(empty);
    return;
  }

  const fragment = document.createDocumentFragment();
  visibleLines.forEach((line) => {
    const row = document.createElement("div");
    row.className = "log-line";
    row.dataset.level = detectLogLevel(line);
    row.textContent = line;
    fragment.append(row);
  });
  elements.logViewport.append(fragment);
  if (elements.autoScroll.checked) {
    elements.logViewport.scrollTop = elements.logViewport.scrollHeight;
  }
}

function setActiveConfigTab(activeTab) {
  const tab = activeTab === "game-list" ? elements.configTabs[1] : elements.configTabs[0];
  elements.configTabs.forEach((button) => {
    const selected = button === tab;
    button.setAttribute("aria-selected", String(selected));
    button.tabIndex = selected ? 0 : -1;
  });
  const gameListActive = tab.id === "gameListTab";
  elements.schedulerConfigPanel.hidden = gameListActive;
  elements.gameListPanel.hidden = !gameListActive;
}

async function refreshLog() {
  if (logPaused || logBusy || document.hidden) return;
  logBusy = true;
  try {
    const result = await runApi("tail-log");
    if (result.stdout !== latestLog) {
      latestLog = result.stdout;
      renderLog();
    }
  } catch (error) {
    if (!latestLog) {
      elements.logViewport.innerHTML = '<div class="empty-state">日志读取失败</div>';
    }
  } finally {
    logBusy = false;
  }
}

function encodeUtf8Base64(value) {
  const bytes = new TextEncoder().encode(value);
  let binary = "";
  const chunkSize = 0x8000;
  for (let offset = 0; offset < bytes.length; offset += chunkSize) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + chunkSize));
  }
  return btoa(binary);
}

async function loadConfig(force = false) {
  if (configDirty && !force && !window.confirm("放弃尚未保存的配置修改？")) return;
  elements.configState.textContent = "载入中";
  try {
    const result = await runApi("read-config");
    elements.configEditor.value = result.stdout;
    configDirty = false;
    elements.configState.textContent = "已载入";
  } catch (error) {
    elements.configState.textContent = "载入失败";
    showToast(error.message);
  }
}

async function loadGameList(force = false) {
  if (gameListDirty && !force && !window.confirm("放弃尚未保存的游戏列表修改？")) return;
  elements.gameListState.textContent = "载入中";
  try {
    const result = await runApi("read-game-list");
    elements.gameListEditor.value = result.stdout;
    gameListDirty = false;
    elements.gameListState.textContent = "已载入";
  } catch (error) {
    elements.gameListState.textContent = "载入失败";
    showToast(error.message);
  }
}

async function writeConfig(restart) {
  const content = elements.configEditor.value;
  if (!content.trim()) {
    showToast("配置不能为空");
    return;
  }
  if (restart && !window.confirm("保存配置并重启 Finalizer？")) return;

  const buttons = [elements.reloadConfig, elements.saveConfig, elements.saveRestart];
  buttons.forEach((button) => { button.disabled = true; });
  elements.configState.textContent = "保存中";
  try {
    await runApi("write-config", encodeUtf8Base64(content));
    configDirty = false;
    elements.configState.textContent = restart ? "正在重启" : "已保存";
    if (restart) {
      await runApi("restart");
      elements.configState.textContent = "已保存并重启";
      latestLog = "";
      await refreshStatus();
      await refreshLog();
    }
    showToast(restart ? "配置已应用" : "配置已保存");
  } catch (error) {
    elements.configState.textContent = "操作失败";
    showToast(error.message);
  } finally {
    buttons.forEach((button) => { button.disabled = false; });
  }
}

async function writeGameList() {
  const content = elements.gameListEditor.value;
  if (!content.trim()) {
    showToast("游戏列表不能为空");
    return;
  }

  const buttons = [elements.reloadGameList, elements.saveGameList];
  buttons.forEach((button) => { button.disabled = true; });
  elements.gameListState.textContent = "保存中";
  try {
    await runApi("write-game-list", encodeUtf8Base64(content));
    gameListDirty = false;
    elements.gameListState.textContent = "已保存";
    showToast("游戏列表已保存");
  } catch (error) {
    elements.gameListState.textContent = "操作失败";
    showToast(error.message);
  } finally {
    buttons.forEach((button) => { button.disabled = false; });
  }
}

elements.modeControl.addEventListener("click", async (event) => {
  const button = event.target.closest("button[data-mode]");
  if (!button) return;
  const mode = button.dataset.mode;
  setControlsDisabled(true);
  elements.modeFeedback.textContent = "切换中";
  try {
    await runApi("set-mode", mode);
    elements.modeFeedback.textContent = "已切换";
    showToast(`已切换到${MODE_LABELS[mode]}模式`);
    await refreshStatus();
  } catch (error) {
    elements.modeFeedback.textContent = "切换失败";
    showToast(error.message);
  } finally {
    setControlsDisabled(false);
  }
});

elements.logLevel.addEventListener("change", async () => {
  const level = elements.logLevel.value;
  setControlsDisabled(true);
  try {
    await runApi("set-log-level", level);
    showToast(`日志级别已设为 ${level}`);
    await refreshStatus();
  } catch (error) {
    showToast(error.message);
    await refreshStatus();
  } finally {
    setControlsDisabled(false);
  }
});

elements.logFilter.addEventListener("change", renderLog);
elements.pauseLog.addEventListener("click", () => {
  logPaused = !logPaused;
  elements.pauseLog.setAttribute("aria-pressed", String(logPaused));
  elements.pauseLog.textContent = logPaused ? "继续" : "暂停";
  if (!logPaused) refreshLog();
});
elements.clearLog.addEventListener("click", async () => {
  try {
    await runApi("clear-log");
    latestLog = "";
    renderLog();
    showToast("日志已清空");
  } catch (error) {
    showToast(error.message);
  }
});
elements.configEditor.addEventListener("input", () => {
  configDirty = true;
  elements.configState.textContent = "未保存";
});
elements.gameListEditor.addEventListener("input", () => {
  gameListDirty = true;
  elements.gameListState.textContent = "未保存";
});
elements.configTabs.forEach((tab, index) => {
  tab.addEventListener("click", () => {
    setActiveConfigTab(tab.id === "gameListTab" ? "game-list" : "scheduler");
  });
  tab.addEventListener("keydown", (event) => {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    event.preventDefault();
    const nextIndex = event.key === "ArrowRight"
      ? (index + 1) % elements.configTabs.length
      : (index - 1 + elements.configTabs.length) % elements.configTabs.length;
    elements.configTabs[nextIndex].focus();
    setActiveConfigTab(elements.configTabs[nextIndex].id === "gameListTab" ? "game-list" : "scheduler");
  });
});
elements.reloadConfig.addEventListener("click", () => loadConfig(false));
elements.saveConfig.addEventListener("click", () => writeConfig(false));
elements.saveRestart.addEventListener("click", () => writeConfig(true));
elements.reloadGameList.addEventListener("click", () => loadGameList(false));
elements.saveGameList.addEventListener("click", writeGameList);
document.addEventListener("visibilitychange", () => {
  if (!document.hidden) {
    refreshStatus();
    refreshLog();
  }
});

async function initialize() {
  if (!hasKernelSuBridge()) {
    elements.runtimeBadge.dataset.state = "error";
    elements.runtimeText.textContent = "WebUI 不可用";
    setControlsDisabled(true);
    elements.configEditor.disabled = true;
    elements.gameListEditor.disabled = true;
    elements.reloadConfig.disabled = true;
    elements.saveConfig.disabled = true;
    elements.saveRestart.disabled = true;
    elements.reloadGameList.disabled = true;
    elements.saveGameList.disabled = true;
    elements.logViewport.innerHTML = '<div class="empty-state">请从 KernelSU 管理器打开</div>';
    return;
  }

  await Promise.all([refreshStatus(), refreshLog(), loadConfig(true), loadGameList(true)]);
  window.setInterval(refreshLog, 1000);
  window.setInterval(refreshStatus, 2500);
}

initialize();
