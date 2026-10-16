use alvr_common::glam::UVec2;
use alvr_graphics::GraphicsContext;
use alvr_session::ClientsidePostProcessingConfig;
use openxr::{self as xr, sys::Handle as _};
use std::ptr;

pub const XR_MNDX_EGL_ENABLE_EXTENSION_NAME: &str = "XR_MNDX_egl_enable";
pub const XR_TYPE_GRAPHICS_BINDING_EGL_MNDX_RAW: i32 = 1000048004;

#[repr(C)]
#[derive(Copy, Clone)]
pub struct GraphicsBindingEGLMNDX {
    pub ty: xr::sys::StructureType,
    pub next: *const std::ffi::c_void,
    pub get_proc_address:
        Option<unsafe extern "C" fn(*const std::ffi::c_char) -> Option<unsafe extern "C" fn()>>,
    pub display: *mut std::ffi::c_void,
    pub config: *mut std::ffi::c_void,
    pub context: *mut std::ffi::c_void,
}

pub struct SessionCreateInfoMndx {
    pub get_proc_address:
        Option<unsafe extern "C" fn(*const std::ffi::c_char) -> Option<unsafe extern "C" fn()>>,
    pub display: *mut std::ffi::c_void,
    pub config: *mut std::ffi::c_void,
    pub context: *mut std::ffi::c_void,
}

pub enum OpenGlEsMndx {}

impl xr::Graphics for OpenGlEsMndx {
    type Requirements = ();
    type SessionCreateInfo = SessionCreateInfoMndx;
    type Format = u32;
    type SwapchainImage = u32;

    fn raise_format(x: i64) -> u32 {
        x as _
    }
    fn lower_format(x: u32) -> i64 {
        x.into()
    }

    fn requirements(instance: &xr::Instance, system: xr::SystemId) -> xr::Result<()> {
        instance.graphics_requirements::<xr::OpenGlEs>(system).map(|_| ())
    }

    unsafe fn create_session(
        instance: &xr::Instance,
        system: xr::SystemId,
        info: &Self::SessionCreateInfo,
    ) -> xr::Result<xr::sys::Session> {
        let binding = GraphicsBindingEGLMNDX {
            ty: xr::sys::StructureType::from_raw(XR_TYPE_GRAPHICS_BINDING_EGL_MNDX_RAW),
            next: ptr::null(),
            get_proc_address: info.get_proc_address,
            display: info.display,
            config: info.config,
            context: info.context,
        };
        let session_info = xr::sys::SessionCreateInfo {
            ty: xr::sys::SessionCreateInfo::TYPE,
            next: &binding as *const _ as *const _,
            create_flags: Default::default(),
            system_id: system,
        };
        let mut out = xr::sys::Session::NULL;
        let res = unsafe { (instance.fp().create_session)(instance.as_raw(), &session_info, &mut out) };
        if res.into_raw() >= 0 {
            Ok(out)
        } else {
            Err(res)
        }
    }

    fn enumerate_swapchain_images(
        swapchain: &xr::Swapchain<Self>,
    ) -> xr::Result<Vec<Self::SwapchainImage>> {
        let mut count = 0u32;
        let res = unsafe {
            (swapchain.instance().fp().enumerate_swapchain_images)(
                swapchain.as_raw(),
                0,
                &mut count,
                ptr::null_mut(),
            )
        };
        if res.into_raw() < 0 {
            return Err(res);
        }

        let mut images = vec![
            xr::sys::SwapchainImageOpenGLESKHR {
                ty: xr::sys::SwapchainImageOpenGLESKHR::TYPE,
                next: ptr::null_mut(),
                image: 0,
            };
            count as usize
        ];

        let res = unsafe {
            (swapchain.instance().fp().enumerate_swapchain_images)(
                swapchain.as_raw(),
                count,
                &mut count,
                images.as_mut_ptr() as *mut _,
            )
        };
        if res.into_raw() < 0 {
            return Err(res);
        }

        Ok(images.into_iter().map(|x| x.image).collect())
    }
}

#[cfg(target_os = "android")]
pub type ClientGraphics = xr::OpenGlEs;

#[cfg(not(target_os = "android"))]
pub type ClientGraphics = OpenGlEsMndx;

pub fn session_create_info(
    ctx: &GraphicsContext,
) -> <ClientGraphics as xr::Graphics>::SessionCreateInfo {
    #[cfg(target_os = "android")]
    {
        xr::opengles::SessionCreateInfo::Android {
            display: ctx.egl_display.as_ptr(),
            config: ctx.egl_config.as_ptr(),
            context: ctx.egl_context.as_ptr(),
        }
    }
    #[cfg(not(target_os = "android"))]
    {
        #[cfg(target_os = "linux")]
        unsafe extern "C" fn get_proc_address_stub(
            name: *const std::ffi::c_char,
        ) -> Option<unsafe extern "C" fn()> {
            unsafe {
                let proc_addr = libc::dlsym(libc::RTLD_DEFAULT, c"eglGetProcAddress".as_ptr());
                if !proc_addr.is_null() {
                    let egl_get_proc_addr: unsafe extern "C" fn(
                        *const std::ffi::c_char,
                    ) -> Option<
                        unsafe extern "C" fn(),
                    > = std::mem::transmute(proc_addr);
                    egl_get_proc_addr(name)
                } else {
                    None
                }
            }
        }

        SessionCreateInfoMndx {
            #[cfg(target_os = "linux")]
            get_proc_address: Some(get_proc_address_stub),
            #[cfg(not(target_os = "linux"))]
            get_proc_address: None,
            display: ctx.egl_display.as_ptr(),
            config: ctx.egl_config.as_ptr(),
            context: ctx.egl_context.as_ptr(),
        }
    }
}

pub fn swapchain_format(
    gfx_ctx: &GraphicsContext,
    session: &xr::Session<ClientGraphics>,
    enable_hdr: bool,
) -> u32 {
    gfx_ctx.make_current();

    let formats = session.enumerate_swapchain_formats().unwrap();
    alvr_common::info!("FrameConnect OpenXR swapchain formats: {formats:?}");
    alvr_graphics::choose_swapchain_format(&formats, enable_hdr)
}

#[allow(unused_variables)]
pub fn create_swapchain(
    session: &xr::Session<ClientGraphics>,
    gfx_ctx: &GraphicsContext,
    resolution: UVec2,
    format: u32,
    foveation: Option<&xr::FoveationProfileFB>,
) -> xr::Swapchain<ClientGraphics> {
    gfx_ctx.make_current();

    let swapchain_info = xr::SwapchainCreateInfo {
        create_flags: xr::SwapchainCreateFlags::EMPTY,
        usage_flags: xr::SwapchainUsageFlags::COLOR_ATTACHMENT | xr::SwapchainUsageFlags::SAMPLED,
        format,
        sample_count: 1,
        width: resolution.x,
        height: resolution.y,
        face_count: 1,
        array_size: 1,
        mip_count: 1,
    };

    session.create_swapchain(&swapchain_info).unwrap()
}

pub struct ProjectionLayerAlphaConfig {
    pub premultiplied: bool,
}

// This is needed to work around lifetime limitations. Deref cannot be implemented because there are
// nested references, and in a way or the other I would get `cannot return reference to temporary
// value`
pub struct ProjectionLayerBuilder<'a> {
    reference_space: &'a xr::Space,
    layers: [xr::CompositionLayerProjectionView<'a, ClientGraphics>; 2],
    alpha: Option<ProjectionLayerAlphaConfig>,
    composition_layer_settings: Option<xr::sys::CompositionLayerSettingsFB>,
}

impl<'a> ProjectionLayerBuilder<'a> {
    pub fn new(
        reference_space: &'a xr::Space,
        layers: [xr::CompositionLayerProjectionView<'a, ClientGraphics>; 2],
        alpha: Option<ProjectionLayerAlphaConfig>,
        clientside_post_processing_config: Option<ClientsidePostProcessingConfig>,
    ) -> Self {
        let composition_layer_settings = clientside_post_processing_config
            .map(|post_processing| {
                xr::CompositionLayerSettingsFlagsFB::from_raw(
                    (post_processing.sharpening as u64) | (post_processing.super_sampling as u64),
                )
            })
            .filter(|&flags| flags != xr::CompositionLayerSettingsFlagsFB::EMPTY)
            .map(|flags| xr::sys::CompositionLayerSettingsFB {
                ty: xr::StructureType::COMPOSITION_LAYER_SETTINGS_FB,
                next: std::ptr::null(),
                layer_flags: flags,
            });
        Self {
            reference_space,
            layers,
            alpha,
            composition_layer_settings,
        }
    }

    pub fn build(&self) -> xr::CompositionLayerProjection<'_, ClientGraphics> {
        let mut flags = xr::CompositionLayerFlags::EMPTY;

        if let Some(alpha) = &self.alpha {
            flags |= xr::CompositionLayerFlags::BLEND_TEXTURE_SOURCE_ALPHA;

            if !alpha.premultiplied {
                flags |= xr::CompositionLayerFlags::UNPREMULTIPLIED_ALPHA;
            }
        }

        let layer = xr::CompositionLayerProjection::new()
            .layer_flags(flags)
            .space(self.reference_space)
            .views(&self.layers);

        if let Some(composition_layer_settings) = self.composition_layer_settings.as_ref() {
            unsafe {
                xr::CompositionLayerProjection::from_raw(xr::sys::CompositionLayerProjection {
                    next: ptr::from_ref(composition_layer_settings).cast(),
                    ..layer.into_raw()
                })
            }
        } else {
            layer
        }
    }
}
