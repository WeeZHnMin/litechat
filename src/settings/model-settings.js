import { invoke } from "@tauri-apps/api/core";
import { closeContextMenu, showContextMenu } from "../ui/context-menu";

const PRESETS = [
  {
    label: "DeepSeek Chat",
    hint: "通用对话，速度快",
    model: "deepseek-chat",
    provider: "deepseek",
    baseUrl: "https://api.deepseek.com/v1",
  },
  {
    label: "DeepSeek Reasoner",
    hint: "深度推理，支持思考过程",
    model: "deepseek-reasoner",
    provider: "deepseek",
    baseUrl: "https://api.deepseek.com/v1",
  },
  {
    label: "Qwen 2.5 7B",
    hint: "本地 Ollama",
    model: "qwen2.5:7b",
    provider: "custom",
    baseUrl: "http://localhost:11434/v1",
  },
  {
    label: "Llama 3.2",
    hint: "本地 Ollama",
    model: "llama3.2",
    provider: "custom",
    baseUrl: "http://localhost:11434/v1",
  },
];

const BASE_URLS = {
  deepseek: "https://api.deepseek.com/v1",
};

/** OpenAI 协议的推理强度档位。off 表示不发送该字段。 */
export const REASONING_LEVELS = [
  { value: "off", label: "关闭", hint: "不请求思考过程" },
  { value: "low", label: "低", hint: "快速思考，省 token" },
  { value: "medium", label: "中", hint: "平衡速度与深度" },
  { value: "high", label: "高", hint: "深入推理，较慢" },
];

export function reasoningLabel(value) {
  return REASONING_LEVELS.find((item) => item.value === value)?.label ?? "关闭";
}

const CSS = {
  provider: "#model-provider",
  baseUrl: "#model-base-url",
  model: "#model-name",
  apiKey: "#model-api-key",
  keyHint: "#model-key-hint",
  keyClear: "#model-key-clear",
  presetTrigger: "#model-preset-trigger",
  scanModels: "#model-scan",
  test: "#model-test",
  testResult: "#model-test-result",
  save: "#model-save",
  error: "#model-error",
  path: "#model-config-path",
};

export function initModelSettings({ onReasoningChange } = {}) {
  const el = {
    provider: document.querySelector(CSS.provider),
    baseUrl: document.querySelector(CSS.baseUrl),
    model: document.querySelector(CSS.model),
    apiKey: document.querySelector(CSS.apiKey),
    keyHint: document.querySelector(CSS.keyHint),
    keyClear: document.querySelector(CSS.keyClear),
    presetTrigger: document.querySelector(CSS.presetTrigger),
    scanModels: document.querySelector(CSS.scanModels),
    test: document.querySelector(CSS.test),
    testResult: document.querySelector(CSS.testResult),
    save: document.querySelector(CSS.save),
    error: document.querySelector(CSS.error),
    path: document.querySelector(CSS.path),
  };
  const view = document.querySelector("#settings-view");

  let saved = null;
  let dirty = false;
  let loading = false;
  let testing = false;
  let scanning = false;
  let feedbackTimer = 0;

  function setScanning(value) {
    scanning = value;
    el.scanModels.disabled = value;
    el.scanModels.classList.toggle("is-testing", value);
    el.scanModels.setAttribute("aria-busy", String(value));
  }

  async function scanModels() {
    if (scanning) return;
    setFeedback("", false);
    setTestResult("", "ok");
    setScanning(true);
    try {
      const names = await invoke("list_models", {
        provider: el.provider.value,
        baseUrl: el.baseUrl.value.trim(),
        apiKey: currentApiKey(),
      });
      if (!names.length) {
        setTestResult("服务没有返回任何模型", "error");
        return;
      }
      setTestResult(`找到 ${names.length} 个模型，点选即可填入`, "ok");
      openModelMenu(names);
    } catch (error) {
      setTestResult(String(error), "error");
    } finally {
      setScanning(false);
    }
  }

  function openModelMenu(names) {
    const rect = el.scanModels.getBoundingClientRect();
    el.scanModels.setAttribute("aria-expanded", "true");
    const items = names.map((name) => ({
      label: name,
      action: () => {
        el.model.value = name;
        markDirty();
        setTestResult("", "ok");
      },
    }));
    const menu = showContextMenu(rect.left, rect.bottom + 4, items);
    const reset = () => el.scanModels.setAttribute("aria-expanded", "false");
    window.addEventListener("pointerdown", reset, { once: true });
    if (menu) menu.addEventListener("click", reset, { once: true });
  }

  function setFeedback(text, isError) {
    el.error.hidden = !text;
    el.error.textContent = text || "";
    el.error.classList.toggle("is-success", Boolean(text) && !isError);
    window.clearTimeout(feedbackTimer);
    if (text && !isError) {
      feedbackTimer = window.setTimeout(() => {
        el.error.hidden = true;
        el.error.textContent = "";
      }, 1800);
    }
  }

  function setTestResult(text, state = "ok") {
    el.testResult.hidden = !text;
    el.testResult.textContent = text || "";
    el.testResult.classList.toggle("is-error", state === "error");
    el.testResult.classList.toggle("is-ok", state === "ok");
  }

  function setTesting(value) {
    testing = value;
    el.test.disabled = value;
    el.test.classList.toggle("is-testing", value);
    el.test.setAttribute("aria-busy", String(value));
  }

  function applyPreset(preset) {
    el.model.value = preset.model;
    el.provider.value = preset.provider;
    el.baseUrl.value = preset.baseUrl;
    dirty = true;
    setFeedback("", false);
  }

  function openPresetMenu() {
    const rect = el.presetTrigger.getBoundingClientRect();
    el.presetTrigger.setAttribute("aria-expanded", "true");
    const items = PRESETS.map((preset) => ({
      label: `${preset.label} · ${preset.hint}`,
      action: () => applyPreset(preset),
    }));
    const menu = showContextMenu(rect.left, rect.bottom + 4, items);
    const reset = () => el.presetTrigger.setAttribute("aria-expanded", "false");
    window.addEventListener("pointerdown", reset, { once: true });
    if (menu) {
      menu.addEventListener("click", reset, { once: true });
    }
  }

  function renderKeyState() {
    el.keyClear.hidden = !saved?.hasApiKey;
    el.keyHint.textContent = saved?.hasApiKey
      ? "点击可替换为新密钥"
      : "尚未设置";
  }

  function applyView(next) {
    saved = next;
    el.provider.value = next.provider;
    el.baseUrl.value = next.baseUrl;
    el.model.value = next.model;
    el.apiKey.type = "text";
    el.apiKey.value = next.hasApiKey ? next.apiKeyHint : "";
    el.apiKey.dataset.masked = String(next.hasApiKey);
    el.apiKey.classList.toggle("is-masked", next.hasApiKey);
    dirty = false;
    renderKeyState();
  }

  function isDirty() {
    if (!saved) return false;
    if (dirty) return true;
    return (
      el.provider.value !== saved.provider ||
      el.baseUrl.value.trim() !== saved.baseUrl ||
      el.model.value.trim() !== saved.model ||
      (el.apiKey.dataset.masked !== "true" && el.apiKey.value.trim() !== "")
    );
  }

  function currentApiKey() {
    if (el.apiKey.dataset.masked === "true") return null;
    return el.apiKey.value.trim() || null;
  }

  function clearMask() {
    if (el.apiKey.dataset.masked !== "true") return;
    el.apiKey.dataset.masked = "false";
    el.apiKey.classList.remove("is-masked");
    el.apiKey.value = "";
    el.apiKey.type = "password";
  }

  function markDirty() {
    dirty = true;
  }

  async function reload() {
    if (isDirty() || loading) return;
    loading = true;
    try {
      applyView(await invoke("get_model_config"));
    } catch (error) {
      console.error("Unable to load model config:", error);
      setFeedback(String(error), true);
    } finally {
      loading = false;
    }
  }

  async function test() {
    if (testing) return;
    if (!el.baseUrl.value.trim() || !el.model.value.trim()) {
      setTestResult("请先填写接口地址和模型名称", "error");
      return;
    }
    setFeedback("", false);
    setTestResult("", "ok");
    setTesting(true);
    try {
      const result = await invoke("test_model_config", {
        provider: el.provider.value,
        baseUrl: el.baseUrl.value.trim(),
        model: el.model.value.trim(),
        apiKey: currentApiKey(),
      });
      setFeedback("", false);
      const seconds = (result.elapsedMs / 1000).toFixed(1);
      setTestResult(
        `连接成功 · ${seconds} 秒 · 模型 ${result.model} 回复「${result.reply}」`,
        "ok",
      );
    } catch (error) {
      setFeedback("", false);
      setTestResult(String(error), "error");
    } finally {
      setTesting(false);
    }
  }

  async function save() {
    setFeedback("", false);
    setTestResult("", "ok");
    el.save.disabled = true;
    try {
      const next = await invoke("save_model_config", {
        provider: el.provider.value,
        baseUrl: el.baseUrl.value.trim(),
        model: el.model.value.trim(),
        // 推理强度由输入栏的按钮控制，这里保存时保持原状，不要覆盖
        reasoningEffort: saved?.reasoningEffort ?? "off",
        apiKey: currentApiKey(),
      });
      applyView(next);
      setFeedback("已保存", false);
    } catch (error) {
      setFeedback(String(error), true);
    } finally {
      el.save.disabled = false;
    }
  }

  el.provider.addEventListener("change", () => {
    markDirty();
    if (el.provider.value === "deepseek" && saved?.provider !== "deepseek") {
      el.baseUrl.value = BASE_URLS.deepseek;
    } else if (!el.baseUrl.value.trim()) {
      el.baseUrl.value = BASE_URLS[el.provider.value] || "";
    }
  });
  el.baseUrl.addEventListener("input", markDirty);
  el.model.addEventListener("input", markDirty);
  el.presetTrigger.addEventListener("click", () => {
    if (el.presetTrigger.getAttribute("aria-expanded") === "true") {
      el.presetTrigger.setAttribute("aria-expanded", "false");
      closeContextMenu();
      return;
    }
    openPresetMenu();
  });
  el.apiKey.addEventListener("focus", clearMask);
  el.apiKey.addEventListener("pointerdown", clearMask);
  el.apiKey.addEventListener("input", markDirty);
  el.baseUrl.addEventListener("input", () => setTestResult("", "ok"));
  el.model.addEventListener("input", () => setTestResult("", "ok"));
  el.provider.addEventListener("change", () => setTestResult("", "ok"));
  el.apiKey.addEventListener("blur", () => {
    if (el.apiKey.dataset.masked === "false" && !el.apiKey.value.trim() && saved?.hasApiKey) {
      el.apiKey.value = saved.apiKeyHint;
      el.apiKey.dataset.masked = "true";
      el.apiKey.classList.add("is-masked");
      el.apiKey.type = "text";
    }
  });

  el.save.addEventListener("click", save);
  el.scanModels.addEventListener("click", () => void scanModels());
  el.test.addEventListener("click", () => void test());
  el.apiKey.addEventListener("keydown", (event) => {
    if (event.key === "Enter" && !event.isComposing) {
      event.preventDefault();
      void save();
    }
  });

  el.keyClear.addEventListener("click", async () => {
    setFeedback("", false);
    try {
      applyView(await invoke("clear_model_api_key"));
      setFeedback("已清除密钥", false);
    } catch (error) {
      setFeedback(String(error), true);
    }
  });

  new MutationObserver(() => {
    if (!view.hidden) void reload();
  }).observe(view, { attributes: true, attributeFilter: ["hidden"] });

  void invoke("model_config_path")
    .then((path) => {
      el.path.textContent = path;
      el.path.title = path;
    })
    .catch(() => {
      el.path.textContent = "";
      el.path.hidden = true;
    });

  /** 输入栏的档位按钮改了强度后调用：只写这一个字段。 */
  async function setReasoningEffort(effort) {
    if (!saved) return;
    try {
      const view = await invoke("set_reasoning_effort", { effort });
      saved = view;
      onReasoningChange?.(view.reasoningEffort);
    } catch (error) {
      setFeedback(String(error), true);
      onReasoningChange?.(saved?.reasoningEffort ?? "off");
    }
  }

  void reload().then(() => onReasoningChange?.(saved?.reasoningEffort ?? "off"));

  return { reload, setReasoningEffort };
}
