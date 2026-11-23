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
    thinking: true,
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

const CSS = {
  provider: "#model-provider",
  baseUrl: "#model-base-url",
  model: "#model-name",
  apiKey: "#model-api-key",
  keyHint: "#model-key-hint",
  keyClear: "#model-key-clear",
  presetTrigger: "#model-preset-trigger",
  save: "#model-save",
  error: "#model-error",
  path: "#model-config-path",
  thinking: "#model-thinking",
};

export function initModelSettings({ onThinkingChange } = {}) {
  const el = {
    provider: document.querySelector(CSS.provider),
    baseUrl: document.querySelector(CSS.baseUrl),
    model: document.querySelector(CSS.model),
    apiKey: document.querySelector(CSS.apiKey),
    keyHint: document.querySelector(CSS.keyHint),
    keyClear: document.querySelector(CSS.keyClear),
    presetTrigger: document.querySelector(CSS.presetTrigger),
    save: document.querySelector(CSS.save),
    error: document.querySelector(CSS.error),
    path: document.querySelector(CSS.path),
    thinking: document.querySelector(CSS.thinking),
  };
  const view = document.querySelector("#settings-view");

  let saved = null;
  let dirty = false;
  let loading = false;
  let feedbackTimer = 0;

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

  function applyPreset(preset) {
    el.model.value = preset.model;
    el.provider.value = preset.provider;
    el.baseUrl.value = preset.baseUrl;
    if (preset.thinking !== undefined) el.thinking.checked = preset.thinking;
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
    el.thinking.checked = Boolean(next.thinking);
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
      el.thinking.checked !== Boolean(saved.thinking) ||
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

  async function save() {
    setFeedback("", false);
    el.save.disabled = true;
    try {
      const next = await invoke("save_model_config", {
        provider: el.provider.value,
        baseUrl: el.baseUrl.value.trim(),
        model: el.model.value.trim(),
        thinking: el.thinking.checked,
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
  el.thinking.addEventListener("change", markDirty);
  el.apiKey.addEventListener("blur", () => {
    if (el.apiKey.dataset.masked === "false" && !el.apiKey.value.trim() && saved?.hasApiKey) {
      el.apiKey.value = saved.apiKeyHint;
      el.apiKey.dataset.masked = "true";
      el.apiKey.classList.add("is-masked");
      el.apiKey.type = "text";
    }
  });

  el.save.addEventListener("click", save);
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

  async function setThinking(enabled) {
    if (!saved) return;
    const next = { ...saved, thinking: Boolean(enabled) };
    try {
      const view = await invoke("save_model_config", {
        provider: next.provider,
        baseUrl: next.baseUrl,
        model: next.model,
        thinking: Boolean(enabled),
        apiKey: null,
      });
      applyView(view);
      onThinkingChange?.(view.thinking);
    } catch (error) {
      setFeedback(String(error), true);
      onThinkingChange?.(Boolean(saved.thinking));
    }
  }

  void reload().then(() => onThinkingChange?.(Boolean(saved?.thinking)));

  return { reload, setThinking };
}
