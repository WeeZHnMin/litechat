# LiteChat

基于 Tauri、Vite 和原生 JavaScript 的桌面聊天界面。

## 启动

```powershell
pnpm install
pnpm tauri dev
```

开发模式默认显示最近一次本地会话。访问 `?demo=1` 可加载不写入历史库的富消息演示。

## 托盘与快捷键

系统托盘左键切换窗口显示状态，右键提供显示、隐藏和退出菜单。

设置页支持自定义显示与隐藏窗口共用的全局切换快捷键。点击当前快捷键，按下新的组合键即可保存，Esc 取消。快捷键保存在应用配置目录，重启后继续生效。

## 模型接入

发送消息时，页面会触发 `litechat:send` 事件：

```js
window.addEventListener("litechat:send", async (event) => {
  const { text, files } = event.detail;
  const stream = window.liteChat.createAssistantStream();
  const reply = await requestModel(text, files);
  stream.push(reply);
  stream.done();
});
```

`requestModel` 需要替换为实际模型请求。持续收到模型内容时，反复调用 `push()`，结束后调用 `done()`，异常时调用 `fail(error)`。

## 本地历史

会话和消息使用 rusqlite 保存在应用数据目录的 `history.db`。首次发送消息时创建会话，完整助手回复在 `done()` 后保存，点击新对话后等待下一次发送再创建新会话。

消息的 `parts` 以 JSON 保存，可恢复 Markdown、图片、文件和引用元数据。Blob URL 仅在当前运行期间有效，附件本体持久化需要额外将文件复制到应用附件目录。

完整消息可以直接追加：

```js
window.liteChat.appendMessage({
  role: "assistant",
  parts: [
    { type: "markdown", text: "## 你好\n支持 **Markdown** 和 $E=mc^2$。" },
    { type: "image", url: "https://example.com/image.png", name: "image.png" },
    { type: "file", url: "https://example.com/report.pdf", name: "report.pdf", size: 204800 },
    {
      type: "citations",
      sources: [{ title: "来源", url: "https://example.com", snippet: "网页摘要" }]
    }
  ]
});
```

支持 Markdown、GFM 表格与任务列表、KaTeX 数学公式、代码高亮与复制、远程图片、生成图片、文件卡片、网页引用、图片预览和流式输出。模型原始 HTML 会经过 DOMPurify 清理，外部链接通过系统浏览器打开。
