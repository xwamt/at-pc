//! Native low-latency screen capture optimizations for desktop streaming.

#[cfg(windows)]
pub mod windows {
    use std::ptr::null_mut;
    use windows_sys::Win32::Graphics::Gdi::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    /// Reusable Win32 GDI screen capturer with direct downsampling and hardware cursor synthesis.
    pub struct GdiScreenCapturer {
        desktop_dc: HDC,
        mem_dc: HDC,
        bitmap: HBITMAP,
        old_bitmap: HGDIOBJ,
        bitmap_w: u32,
        bitmap_h: u32,
        rgba_buffer: Vec<u8>,
        rgb_buffer: Vec<u8>,
    }

    // SAFETY: DC and HBITMAP pointers are thread-exclusive within the blocking capture task
    unsafe impl Send for GdiScreenCapturer {}

    impl GdiScreenCapturer {
        pub fn new() -> Result<Self, String> {
            unsafe {
                let desktop_dc = GetDC(0);
                if desktop_dc == 0 {
                    return Err("Failed to get desktop DC via GetDC(0)".to_string());
                }
                let mem_dc = CreateCompatibleDC(desktop_dc);
                if mem_dc == 0 {
                    ReleaseDC(0, desktop_dc);
                    return Err("Failed to create compatible DC via CreateCompatibleDC".to_string());
                }

                Ok(Self {
                    desktop_dc,
                    mem_dc,
                    bitmap: 0,
                    old_bitmap: 0,
                    bitmap_w: 0,
                    bitmap_h: 0,
                    rgba_buffer: Vec::new(),
                    rgb_buffer: Vec::new(),
                })
            }
        }

        /// Captures monitor region (monitor_x, monitor_y, monitor_w, monitor_h), directly downsampling
        /// into (target_w, target_h), synthesizes current cursor, and returns (&rgba, &rgb) buffers.
        pub fn capture_scaled(
            &mut self,
            monitor_x: i32,
            monitor_y: i32,
            monitor_w: u32,
            monitor_h: u32,
            target_w: u32,
            target_h: u32,
        ) -> Result<(&[u8], &[u8]), String> {
            if target_w == 0 || target_h == 0 || monitor_w == 0 || monitor_h == 0 {
                return Err("Invalid capture dimensions".to_string());
            }

            unsafe {
                // 1. Reallocate bitmap and buffers if target dimensions changed
                if self.bitmap_w != target_w || self.bitmap_h != target_h {
                    if self.old_bitmap != 0 {
                        SelectObject(self.mem_dc, self.old_bitmap);
                        self.old_bitmap = 0;
                    }
                    if self.bitmap != 0 {
                        DeleteObject(self.bitmap);
                        self.bitmap = 0;
                    }

                    let new_bitmap =
                        CreateCompatibleBitmap(self.desktop_dc, target_w as i32, target_h as i32);
                    if new_bitmap == 0 {
                        return Err("Failed to create compatible bitmap".to_string());
                    }
                    self.old_bitmap = SelectObject(self.mem_dc, new_bitmap);
                    self.bitmap = new_bitmap;
                    self.bitmap_w = target_w;
                    self.bitmap_h = target_h;

                    let pixel_count = (target_w as usize) * (target_h as usize);
                    self.rgba_buffer.resize(pixel_count * 4, 0);
                    self.rgb_buffer.resize(pixel_count * 3, 0);
                }

                // 2. High-quality StretchBlt from physical monitor to memory DC
                SetStretchBltMode(self.mem_dc, HALFTONE as i32);
                SetBrushOrgEx(self.mem_dc, 0, 0, null_mut());
                let ret = StretchBlt(
                    self.mem_dc,
                    0,
                    0,
                    target_w as i32,
                    target_h as i32,
                    self.desktop_dc,
                    monitor_x,
                    monitor_y,
                    monitor_w as i32,
                    monitor_h as i32,
                    SRCCOPY,
                );
                if ret == 0 {
                    return Err("StretchBlt failed".to_string());
                }

                // 3. Synthesize hardware cursor onto the captured image
                let mut ci: CURSORINFO = std::mem::zeroed();
                ci.cbSize = std::mem::size_of::<CURSORINFO>() as u32;
                if GetCursorInfo(&mut ci) != 0 && (ci.flags & CURSOR_SHOWING) != 0 {
                    let mut ii: ICONINFO = std::mem::zeroed();
                    if GetIconInfo(ci.hCursor, &mut ii) != 0 {
                        let cx = ci.ptScreenPos.x - ii.xHotspot as i32;
                        let cy = ci.ptScreenPos.y - ii.yHotspot as i32;

                        if cx + 32 >= monitor_x
                            && cx <= monitor_x + monitor_w as i32
                            && cy + 32 >= monitor_y
                            && cy <= monitor_y + monitor_h as i32
                        {
                            let draw_x = (((cx - monitor_x) as f64 * target_w as f64)
                                / monitor_w as f64)
                                .round() as i32;
                            let draw_y = (((cy - monitor_y) as f64 * target_h as f64)
                                / monitor_h as f64)
                                .round() as i32;
                            let draw_w = ((32.0 * target_w as f64) / monitor_w as f64)
                                .round()
                                .max(16.0) as i32;
                            let draw_h = ((32.0 * target_h as f64) / monitor_h as f64)
                                .round()
                                .max(16.0) as i32;

                            DrawIconEx(
                                self.mem_dc,
                                draw_x,
                                draw_y,
                                ci.hCursor,
                                draw_w,
                                draw_h,
                                0,
                                0,
                                DI_NORMAL,
                            );
                        }

                        if ii.hbmColor != 0 {
                            DeleteObject(ii.hbmColor);
                        }
                        if ii.hbmMask != 0 {
                            DeleteObject(ii.hbmMask);
                        }
                    }
                }

                // 4. Extract pixels via GetDIBits (top-down 32-bit BGRA)
                let mut bmi: BITMAPINFO = std::mem::zeroed();
                bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
                bmi.bmiHeader.biWidth = target_w as i32;
                bmi.bmiHeader.biHeight = -(target_h as i32); // negative for top-down
                bmi.bmiHeader.biPlanes = 1;
                bmi.bmiHeader.biBitCount = 32;
                bmi.bmiHeader.biCompression = BI_RGB as u32;

                let lines = GetDIBits(
                    self.mem_dc,
                    self.bitmap,
                    0,
                    target_h,
                    self.rgba_buffer.as_mut_ptr() as *mut _,
                    &mut bmi,
                    DIB_RGB_COLORS,
                );
                if lines == 0 {
                    return Err("GetDIBits failed".to_string());
                }

                // 5. Convert BGRA in rgba_buffer to RGB in rgb_buffer, and adjust rgba_buffer to RGBA
                let num_pixels = (target_w as usize) * (target_h as usize);
                let raw_rgba = self.rgba_buffer.as_mut_slice();
                let raw_rgb = self.rgb_buffer.as_mut_slice();

                for i in 0..num_pixels {
                    let bgra_idx = i * 4;
                    let rgb_idx = i * 3;
                    let b = raw_rgba[bgra_idx];
                    let g = raw_rgba[bgra_idx + 1];
                    let r = raw_rgba[bgra_idx + 2];

                    raw_rgb[rgb_idx] = r;
                    raw_rgb[rgb_idx + 1] = g;
                    raw_rgb[rgb_idx + 2] = b;

                    raw_rgba[bgra_idx] = r;
                    raw_rgba[bgra_idx + 2] = b;
                    raw_rgba[bgra_idx + 3] = 255;
                }

                Ok((&self.rgba_buffer, &self.rgb_buffer))
            }
        }
    }

    impl Drop for GdiScreenCapturer {
        fn drop(&mut self) {
            unsafe {
                if self.old_bitmap != 0 {
                    SelectObject(self.mem_dc, self.old_bitmap);
                }
                if self.bitmap != 0 {
                    DeleteObject(self.bitmap);
                }
                if self.mem_dc != 0 {
                    DeleteDC(self.mem_dc);
                }
                if self.desktop_dc != 0 {
                    ReleaseDC(0, self.desktop_dc);
                }
            }
        }
    }
}
