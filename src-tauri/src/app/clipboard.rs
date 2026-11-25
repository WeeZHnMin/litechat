use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardImage {
    pub width: u32,
    pub height: u32,
    pub pixels: String,
}

fn encode_base64(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn launch_snipping_tool() -> Result<(), String> {
    std::process::Command::new("cmd")
        .args(["/C", "start", "", "ms-screenclip:"])
        .spawn()
        .map_err(|error| format!("无法打开系统截图工具: {error}"))?;
    Ok(())
}

#[tauri::command]
pub fn clipboard_has_image() -> bool {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::DataExchange::IsClipboardFormatAvailable;
        IsClipboardFormatAvailable(2) != 0
    }
    #[cfg(not(windows))]
    false
}

#[tauri::command]
pub fn read_clipboard_image() -> Result<ClipboardImage, String> {
    let image = read_clipboard()?;
    Ok(ClipboardImage {
        width: image.width(),
        height: image.height(),
        pixels: encode_base64(image.as_raw()),
    })
}

/// 把前端传来的 PNG 写入系统剪贴板。
#[tauri::command]
pub fn copy_image_to_clipboard(png: String) -> Result<(), String> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(png.as_bytes())
        .map_err(|error| format!("图片数据无效: {error}"))?;
    let image = image::load_from_memory(&bytes)
        .map_err(|error| format!("无法解析图片: {error}"))?
        .to_rgba8();
    write_clipboard(&image)
}

#[cfg(windows)]
fn write_clipboard(image: &image::RgbaImage) -> Result<(), String> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Graphics::Gdi::{BITMAPINFOHEADER, BI_RGB};
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
    };
    use windows_sys::Win32::System::Memory::{LocalAlloc, LMEM_MOVEABLE};

    const CF_DIB: u32 = 8;

    let width = image.width() as i32;
    let height = image.height() as i32;
    if width <= 0 || height <= 0 {
        return Err("图片尺寸无效".into());
    }

    let stride = width as usize * 4;
    let pixel_bytes = stride * height as usize;
    let header_size = std::mem::size_of::<BITMAPINFOHEADER>();
    let total = header_size + pixel_bytes;

    unsafe {
        let global = LocalAlloc(LMEM_MOVEABLE, total);
        if global.is_null() {
            return Err("内存分配失败".into());
        }
        let base = global as *mut u8;

        let header = base as *mut BITMAPINFOHEADER;
        std::ptr::write_bytes(base, 0, header_size);
        (*header).biSize = header_size as u32;
        (*header).biWidth = width;
        (*header).biHeight = -height;
        (*header).biPlanes = 1;
        (*header).biBitCount = 32;
        (*header).biCompression = BI_RGB;
        (*header).biSizeImage = pixel_bytes as u32;

        let pixels = base.add(header_size);
        for (index, source) in image.as_raw().chunks_exact(4).enumerate() {
            let target = pixels.add(index * 4);
            *target = source[2];
            *target.add(1) = source[1];
            *target.add(2) = source[0];
            *target.add(3) = 255;
        }

        if OpenClipboard(std::ptr::null_mut()) == 0 {
            LocalFree(global);
            return Err("无法打开剪贴板".into());
        }
        let handle = global as *mut std::ffi::c_void;
        let ok = EmptyClipboard() != 0 && !SetClipboardData(CF_DIB, handle).is_null();
        CloseClipboard();
        if !ok {
            LocalFree(global);
            return Err("写入剪贴板失败".into());
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn capture_with_snipping_tool() -> Result<ClipboardImage, String> {
    let baseline = clipboard_sequence();
    launch_snipping_tool()?;
    eprintln!("[shot] 已唤起截图工具，等待剪贴板更新");

    let mut seen_overlay = false;
    let mut gone_checks = 0;

    for tick in 0..900 {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        if clipboard_sequence() != baseline {
            let image = read_clipboard()?;
            eprintln!("[shot] 已截取剪贴板图片 {}x{}", image.width(), image.height());
            return Ok(ClipboardImage {
                width: image.width(),
                height: image.height(),
                pixels: encode_base64(image.as_raw()),
            });
        }

        // 窗口枚举较贵，每 10 拍（约 500ms）查一次
        if tick % 10 != 0 {
            continue;
        }

        if overlay_present() {
            seen_overlay = true;
            gone_checks = 0;
            continue;
        }

        if !seen_overlay {
            gone_checks += 1;
            if gone_checks >= 12 {
                eprintln!("[shot] 截图工具未出现");
                return Err("截图工具未能启动，请重试".into());
            }
            continue;
        }

        gone_checks += 1;
        if gone_checks >= 2 {
            eprintln!("[shot] 截图窗口已关闭且剪贴板未变，判定为取消");
            return Err("已取消截图".into());
        }
    }
    Err("截图超时（45 秒）".into())
}

#[cfg(windows)]
fn overlay_present() -> bool {
    use windows_sys::core::BOOL;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, TRUE};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextLengthW, GetWindowTextW, IsWindowVisible,
    };

    struct Search {
        found: bool,
    }

    unsafe extern "system" fn callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let search = &mut *(lparam as *mut Search);
        if IsWindowVisible(hwnd) == 0 {
            return TRUE;
        }
        let length = GetWindowTextLengthW(hwnd);
        if length <= 0 {
            return TRUE;
        }
        let mut buffer = vec![0u16; (length + 1) as usize];
        let written = GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32);
        if written <= 0 {
            return TRUE;
        }
        let title = String::from_utf16_lossy(&buffer[..written as usize]);
        if title.contains("Snipping Tool") || title.contains("截图") {
            search.found = true;
            return 0;
        }
        TRUE
    }

    let mut search = Search { found: false };
    unsafe {
        EnumWindows(Some(callback), &mut search as *mut _ as LPARAM);
    }
    search.found
}

#[cfg(not(windows))]
fn overlay_present() -> bool {
    false
}

#[cfg(windows)]
fn clipboard_sequence() -> u32 {
    use windows_sys::Win32::System::DataExchange::GetClipboardSequenceNumber;
    unsafe { GetClipboardSequenceNumber() }
}

#[cfg(not(windows))]
fn clipboard_sequence() -> u32 {
    0
}

#[cfg(windows)]
fn read_clipboard() -> Result<image::RgbaImage, String> {
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::Graphics::Gdi::{
        CreateCompatibleDC, DeleteDC, GetDC, GetDIBits, GetObjectW, ReleaseDC, SelectObject,
        BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HGDIOBJ,
    };
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    };

    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            unsafe { CloseClipboard() };
        }
    }

    unsafe {
        if IsClipboardFormatAvailable(2) == 0 {
            return Err("剪贴板里没有图片".into());
        }
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return Err("无法打开剪贴板".into());
        }
        let _guard = Guard;
        let handle: HANDLE = GetClipboardData(2);
        if handle.is_null() {
            return Err("剪贴板里没有图片".into());
        }
        let mut bitmap: BITMAP = std::mem::zeroed();
        if GetObjectW(
            handle,
            std::mem::size_of::<BITMAP>() as i32,
            &mut bitmap as *mut _ as *mut _,
        ) == 0
        {
            return Err("无法读取剪贴板位图".into());
        }
        let (width, height) = (bitmap.bmWidth, bitmap.bmHeight);
        if width <= 0 || height <= 0 {
            return Err("剪贴板图片尺寸无效".into());
        }
        let screen = GetDC(std::ptr::null_mut());
        let memory = CreateCompatibleDC(screen);
        let previous: HGDIOBJ = SelectObject(memory, handle as HGDIOBJ);
        let mut pixels = vec![0u8; (width as usize) * (height as usize) * 4];
        let mut info: BITMAPINFO = std::mem::zeroed();
        info.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..std::mem::zeroed()
        };
        let lines = GetDIBits(
            memory,
            handle as HGDIOBJ,
            0,
            height as u32,
            pixels.as_mut_ptr().cast(),
            &mut info,
            DIB_RGB_COLORS,
        );
        SelectObject(memory, previous);
        DeleteDC(memory);
        ReleaseDC(std::ptr::null_mut(), screen);
        if lines == 0 {
            return Err("无法从剪贴板取像素".into());
        }
        for pixel in pixels.chunks_exact_mut(4) {
            pixel.swap(0, 2);
            pixel[3] = 255;
        }
        image::RgbaImage::from_raw(width as u32, height as u32, pixels)
            .ok_or_else(|| "无法构造图片".to_string())
    }
}

#[cfg(not(windows))]
fn read_clipboard() -> Result<image::RgbaImage, String> {
    Err("当前平台不支持读取剪贴板图片".into())
}
