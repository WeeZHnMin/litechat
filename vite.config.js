import { defineConfig } from "vite";

export default defineConfig({
  clearScreen: false,
  server: {
    host: "127.0.0.1",
    port: 1420,
    strictPort: true,
    watch: {
      // uploads 是运行时写入的用户附件，监听它会在文件被占用时报 EBUSY
      // 并拖垮整个 dev server
      ignored: ["**/src-tauri/**", "**/uploads/**", "**/.litechat/**"],
    },
  },
});
