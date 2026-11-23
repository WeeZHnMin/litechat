export function loadDemo(appendMessage = window.liteChat.appendMessage) {
  appendMessage({
    role: "user",
    content: "展示 Markdown、数学公式、代码、图片、文件和参考来源。",
  });
  appendMessage({
    role: "assistant",
    parts: [
      {
        type: "markdown",
        text: `## 富媒体回复

现在支持 **GFM Markdown**、行内代码 \`const ready = true\`、任务列表和表格。

- [x] Markdown 与数学公式
- [x] 代码高亮和一键复制
- [x] 图片、文件与网页引用

> 模型内容经过安全清理，外部链接由系统浏览器打开。

行内公式 $E = mc^2$，独立公式：

$$
\\frac{\\partial}{\\partial t} \\Psi = \\frac{i\\hbar}{2m} \\nabla^2 \\Psi
$$

| 类型 | 状态 |
| --- | --- |
| 流式回复 | 已支持 |
| 附件上传 | 已支持 |

\`\`\`ts
interface ChatMessage {
  role: "user" | "assistant";
  parts: Array<{
    type: "markdown" | "image" | "file";
    text?: string;
    url?: string;
  }>;
}

export const ready: boolean = true;
\`\`\`

也可以访问 [Tauri 官方文档](https://v2.tauri.app/)。`,
      },
      {
        type: "image",
        url: "/demo-image.svg",
        name: "generated-diagram.svg",
        alt: "LiteChat 生成图片演示",
      },
      {
        type: "file",
        url: "/demo-report.txt",
        name: "分析报告.txt",
        mime: "text/plain",
        size: 684,
      },
      {
        type: "citations",
        sources: [
          { id: 1, title: "Tauri 2 文档", url: "https://v2.tauri.app/", snippet: "桌面应用与窗口能力" },
          { id: 2, title: "MDN Web Docs", url: "https://developer.mozilla.org/", snippet: "Web 平台参考资料" },
        ],
      },
    ],
  });
}
