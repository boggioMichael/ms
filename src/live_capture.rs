use crate::errors::CaptureError;
use crate::frame::Frame;
use crate::frame_source::FrameSource;
use log::{debug, info};
use std::time::{Duration, Instant};
use windows::core::ComInterface;
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Direct3D11::{D3D11CreateDevice, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D, D3D11_TEXTURE2D_DESC, D3D11_CPU_ACCESS_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ, D3D11_BIND_FLAG, D3D11_RESOURCE_MISC_FLAG, D3D11_CREATE_DEVICE_FLAG, D3D11_USAGE_STAGING};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_11_0};
use windows::Win32::System::WinRT::Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess};
use windows::Graphics::Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Win32::UI::WindowsAndMessaging::{GetClientRect, IsIconic};

/// LiveCapture via Windows Graphics Capture (WinRT). This implementation prefers WGC and will
/// return detailed CaptureError on failures.
pub struct LiveCapture {
    hwnd: HWND,
    width: u32,
    height: u32,
    d3d_device: ID3D11Device,
    d3d_context: ID3D11DeviceContext,
    direct3d_device: IDirect3DDevice,
    pool: Option<Direct3D11CaptureFramePool>,
    session: Option<GraphicsCaptureSession>,
    frame_count: u64,
}


impl LiveCapture {
    /// Create a new LiveCapture for the given HWND using Windows Graphics Capture (WGC).
    pub fn new(hwnd: HWND) -> Result<Self, CaptureError> {
        unsafe {
            if IsIconic(hwnd).as_bool() {
                return Err(CaptureError::Other("Target window is minimized".into()));
            }
            let mut rect = RECT::default();
            if GetClientRect(hwnd, &mut rect).0 == 0 {
                return Err(CaptureError::Other("GetClientRect failed".into()));
            }
            let width = (rect.right - rect.left) as u32;
            let height = (rect.bottom - rect.top) as u32;
            if width == 0 || height == 0 {
                return Err(CaptureError::Other("Window has zero client size".into()));
            }

            // Create D3D11 device
            let (device, context) = Self::create_d3d11_device()?;
            // Create WinRT Direct3D device from DXGI device
            let dxgi: IDXGIDevice = device.cast().map_err(|e| CaptureError::Other(format!("IDXGI cast failed: {}", e)))?;
            let inspectable = CreateDirect3D11DeviceFromDXGIDevice(&dxgi).map_err(|e| CaptureError::Other(format!("CreateDirect3D11DeviceFromDXGIDevice failed: {}", e)))?;
            let direct3d_device: IDirect3DDevice = inspectable.cast().map_err(|e| CaptureError::Other(format!("IDirect3DDevice cast failed: {}", e)))?;

            info!("LiveCapture (WGC) initialized {}x{}", width, height);
                        Ok(Self { hwnd, width, height, d3d_device: device, d3d_context: context, direct3d_device, pool: None, session: None, frame_count: 0 })
        }
    }

    unsafe fn create_d3d11_device() -> Result<(ID3D11Device, ID3D11DeviceContext), CaptureError> {
        // Create D3D11 device with BGRA support
        let mut device: Option<ID3D11Device> = None;
        let mut context: Option<ID3D11DeviceContext> = None;
        let feature_levels = [D3D_FEATURE_LEVEL_11_0];
        let flags = D3D11_CREATE_DEVICE_BGRA_SUPPORT; // already a D3D11_CREATE_DEVICE_FLAG
        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                None,
                flags,
                Some(&feature_levels),
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
            .map_err(|e| CaptureError::Other(format!("D3D11CreateDevice failed: {}", e)))?;
        }

        let device = device.ok_or_else(|| CaptureError::Other("D3D11CreateDevice returned no device".into()))?;
        let context = context.ok_or_else(|| CaptureError::Other("D3D11CreateDevice returned no context".into()))?;
        Ok((device, context))
    }

    fn bgra_to_rgba(mut pixels: Vec<u8>) -> Vec<u8> {
        for chunk in pixels.chunks_exact_mut(4) {
            let b = chunk[0];
            chunk[0] = chunk[2];
            chunk[2] = b;
        }
        pixels
    }

    fn ensure_pool(&mut self) -> Result<(), CaptureError> {
        if self.pool.is_some() {
            return Ok(());
        }
        // Create a dummy GraphicsCaptureItem via interop for hwnd
        unsafe {
                    info!("Creating WGC frame pool for hwnd={:?}", self.hwnd);
                    let interop: windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop = windows::core::factory::<GraphicsCaptureItem, windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop>()
                        .map_err(|e| CaptureError::Other(format!("WGC factory failed: {}", e)))?;
                    let item: GraphicsCaptureItem = interop.CreateForWindow(self.hwnd).map_err(|e| CaptureError::Other(format!("CreateForWindow failed: {}", e)))?;
                    let size = item.Size().map_err(|e| CaptureError::Other(format!("Item.Size failed: {}", e)))?;
                    let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(&self.direct3d_device, DirectXPixelFormat::B8G8R8A8UIntNormalized, 2, size)
                        .map_err(|e| CaptureError::Other(format!("Create frame pool failed: {}", e)))?;
                    let session = pool.CreateCaptureSession(&item).map_err(|e| CaptureError::Other(format!("CreateCaptureSession failed: {}", e)))?;
                    session.StartCapture().map_err(|e| CaptureError::Other(format!("StartCapture failed: {}", e)))?;
                    self.pool = Some(pool);
                    self.session = Some(session);
                    info!("WGC frame pool created: {:?}", size);
                    info!("WGC capture session started for hwnd={:?}", self.hwnd);
                    Ok(())
            }
        }

    fn try_capture_frame_internal(&mut self) -> Result<Frame, CaptureError> {
        unsafe {
            self.ensure_pool()?;
            // Try get next frame with retries. On timeout, attempt to reinitialize the frame pool
            let mut frame_opt = None;
            let mut attempt = 0;
            const MAX_ATTEMPTS: usize = 3;
            while attempt < MAX_ATTEMPTS {
                // ensure we have a pool for this attempt
                if self.pool.is_none() {
                    self.ensure_pool()?;
                }
                let pool_ref = self.pool.as_ref().unwrap();

                let deadline = Instant::now() + Duration::from_millis(1500);
                while Instant::now() < deadline {
                    match pool_ref.TryGetNextFrame() {
                        Ok(f) => { frame_opt = Some(f); break; }
                        Err(_) => { std::thread::sleep(Duration::from_millis(20)); }
                    }
                }
                if frame_opt.is_some() {
                    break;
                }

                // No frame received this attempt. If we have retries left, reinitialize the pool and try again.
                attempt += 1;
                if attempt < MAX_ATTEMPTS {
                    info!("WGC frame timeout, reinitializing frame pool (attempt {}/{})", attempt + 1, MAX_ATTEMPTS);
                    // drop current pool and session and recreate on next loop iteration
                    self.pool = None;
                    self.session = None;
                    frame_opt = None;
                    // small backoff before recreate
                    std::thread::sleep(Duration::from_millis(50));
                    continue;
                }
            }

            let frame = frame_opt.ok_or_else(|| CaptureError::Other("WGC did not return a frame in time".into()))?;
            let surface = frame.Surface().map_err(|e| CaptureError::Other(format!("Frame Surface failed: {}", e)))?;
            let access: IDirect3DDxgiInterfaceAccess = surface.cast().map_err(|e| CaptureError::Other(format!("Surface cast failed: {}", e)))?;
            let texture: ID3D11Texture2D = access.GetInterface().map_err(|e| CaptureError::Other(format!("GetInterface failed: {}", e)))?;
            self.frame_count = self.frame_count.wrapping_add(1);

            let mut src_desc = D3D11_TEXTURE2D_DESC::default();
            texture.GetDesc(&mut src_desc);
            info!(
                "Captured WGC frame #{} desc: format={:?} size={}x{} array={} mips={} sample={}/{} usage={} bind={} cpu={} misc={}",
                self.frame_count,
                src_desc.Format,
                src_desc.Width,
                src_desc.Height,
                src_desc.ArraySize,
                src_desc.MipLevels,
                src_desc.SampleDesc.Count,
                src_desc.SampleDesc.Quality,
                src_desc.Usage.0,
                src_desc.BindFlags.0,
                src_desc.CPUAccessFlags.0,
                src_desc.MiscFlags.0,
            );

            let mut staging_desc = src_desc;
            staging_desc.Usage = D3D11_USAGE_STAGING;
            staging_desc.BindFlags = D3D11_BIND_FLAG(0);
            staging_desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
            staging_desc.MiscFlags = D3D11_RESOURCE_MISC_FLAG(0);
            if staging_desc.SampleDesc.Count > 1 {
                info!("Captured texture is multisampled; resolving to non-MSAA texture");
                staging_desc.SampleDesc.Count = 1;
                staging_desc.SampleDesc.Quality = 0;
            }

            let mut staging: Option<ID3D11Texture2D> = None;
            self.d3d_device
                .CreateTexture2D(&staging_desc, None, Some(&mut staging))
                .map_err(|e| CaptureError::Other(format!("CreateTexture2D failed: {}", e)))?;
            let staging = staging.ok_or_else(|| CaptureError::Other("Staging texture creation failed".into()))?;
            self.d3d_context.CopyResource(&staging, &texture);

            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            self.d3d_context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped)).map_err(|e| CaptureError::Other(format!("Map failed: {}", e)))?;

            let width = src_desc.Width as usize;
            let height = src_desc.Height as usize;
            let stride = mapped.RowPitch as usize;
            let mut pixels = vec![0u8; width * height * 4];
            let src = mapped.pData as *const u8;
            for row in 0..height {
                std::ptr::copy_nonoverlapping(src.add(row * stride), pixels.as_mut_ptr().add(row * width * 4), width * 4);
            }
                        // Unmap returns ()
                        self.d3d_context.Unmap(&staging, 0);

                        let pixels = Self::bgra_to_rgba(pixels);
                        Ok(Frame::from_rgba(width as u32, height as u32, pixels))
        }
    }
}

impl FrameSource for LiveCapture {
    fn next_frame(&mut self) -> Result<Frame, CaptureError> {
        self.try_capture_frame_internal()
    }

    fn width(&self) -> Option<u32> {
        Some(self.width)
    }

    fn height(&self) -> Option<u32> {
        Some(self.height)
    }
}
