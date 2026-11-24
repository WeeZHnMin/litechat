import { invoke } from "@tauri-apps/api/core";

export function pixelsToFile({ pixels, width, height }, name) {
  const binary = atob(pixels);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) bytes[i] = binary.charCodeAt(i);

  const canvas = document.createElement("canvas");
  canvas.width = width;
  canvas.height = height;
  const context = canvas.getContext("2d");
  const image = context.createImageData(width, height);
  image.data.set(bytes);
  context.putImageData(image, 0, 0);

  return new Promise((resolve) => {
    canvas.toBlob((blob) => {
      resolve(blob ? new File([blob], name, { type: "image/png" }) : null);
    }, "image/png");
  });
}

export async function captureWithSnippingTool() {
  const shot = await invoke("capture_with_snipping_tool");
  return pixelsToFile(shot, `screenshot-${Date.now()}.png`);
}
