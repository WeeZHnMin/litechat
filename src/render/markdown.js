import { marked } from "marked";
import markedKatex from "marked-katex-extension";
import DOMPurify from "dompurify";
import hljs from "highlight.js/lib/common";
import { openResource, safeResourceUrl, showImagePreview } from "./media";

marked.use(markedKatex({ throwOnError: false }));
marked.setOptions({ gfm: true, breaks: true });
marked.use({
  renderer: {
    html({ text }) {
      return text.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;");
    },
  },
});

async function copyText(text) {
  if (navigator.clipboard?.writeText) {
    await navigator.clipboard.writeText(text);
    return;
  }
  const textarea = document.createElement("textarea");
  textarea.value = text;
  textarea.style.position = "fixed";
  textarea.style.opacity = "0";
  document.body.append(textarea);
  textarea.select();
  document.execCommand("copy");
  textarea.remove();
}

function enhanceCodeBlocks(container) {
  for (const pre of container.querySelectorAll("pre")) {
    const code = pre.querySelector("code");
    if (!code) continue;
    const languageClass = [...code.classList].find((name) => name.startsWith("language-"));
    const language = languageClass?.slice(9) || "text";
    code.classList.add("hljs");
    if (hljs.getLanguage(language)) hljs.highlightElement(code);
    const wrapper = document.createElement("section");
    wrapper.className = "code-block";
    const header = document.createElement("header");
    const label = document.createElement("span");
    label.textContent = language;
    const copy = document.createElement("button");
    copy.type = "button";
    copy.textContent = "复制";
    copy.addEventListener("click", async () => {
      try {
        await copyText(code.textContent);
        copy.textContent = "已复制";
        window.setTimeout(() => { copy.textContent = "复制"; }, 1400);
      } catch (error) {
        console.error("Unable to copy code:", error);
      }
    });
    header.append(label, copy);
    pre.replaceWith(wrapper);
    wrapper.append(header, pre);
  }
}

function enhanceLinks(container) {
  for (const link of container.querySelectorAll("a")) {
    const url = safeResourceUrl(link.getAttribute("href"));
    if (!url || !/^https?:\/\//.test(url)) {
      link.removeAttribute("href");
      continue;
    }
    link.href = url;
    link.target = "_blank";
    link.rel = "noopener noreferrer";
    link.addEventListener("click", (event) => {
      event.preventDefault();
      void openResource(url);
    });
  }
}

function enhanceImages(container) {
  for (const image of container.querySelectorAll("img")) {
    const url = safeResourceUrl(image.getAttribute("src"), true);
    if (!url) {
      image.remove();
      continue;
    }
    image.src = url;
    image.loading = "lazy";
    image.referrerPolicy = "no-referrer";
    image.tabIndex = 0;
    image.setAttribute("role", "button");
    const preview = () => showImagePreview(url, image.alt, image.alt || "image.png");
    image.addEventListener("click", preview);
    image.addEventListener("keydown", (event) => {
      if (event.key === "Enter" || event.key === " ") {
        event.preventDefault();
        preview();
      }
    });
  }
}

function enhanceTaskLists(container) {
  for (const checkbox of container.querySelectorAll('input[type="checkbox"]')) {
    checkbox.disabled = true;
  }
}

export function renderMarkdown(text = "", { enhance = true } = {}) {
  const container = document.createElement("div");
  container.className = "markdown-body";
  const rendered = marked.parse(String(text), { async: false });
  container.innerHTML = DOMPurify.sanitize(rendered, {
    USE_PROFILES: { html: true, mathml: true },
  });
  enhanceLinks(container);
  if (enhance) {
    enhanceImages(container);
    enhanceCodeBlocks(container);
  }
  enhanceTaskLists(container);
  return container;
}
