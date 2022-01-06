use std::sync::Arc;

use ash_egui::egui::{CtxRef, self, vec2};
use kajiya::{
    backend::{
        ash::{self, vk},
        Device, Image, ImageDesc, ImageViewDesc,
    },
    ui_renderer::UiRenderer,
};

use parking_lot::Mutex;

struct GfxResources {
    //egui_render_pass: RenderPass,
    pub egui_render_pass: vk::RenderPass,
    pub egui_framebuffer: vk::Framebuffer,
    pub egui_texture: Arc<Image>,
}

pub struct EguiBackendInner {
    egui_renderer: ash_egui::Renderer,
    gfx: Option<GfxResources>,
}

pub struct EguiBackend {
    inner: Arc<Mutex<EguiBackendInner>>,
    device: Arc<Device>,
    pub raw_input: ash_egui::egui::RawInput,
}

impl EguiBackend {
    pub fn new(
        device: Arc<Device>,
        window_settings: (u32, u32, f64),
        context: &mut CtxRef,
    ) -> Self {
        let (window_width, window_height, window_scale_factor) = window_settings;

        // Create raw_input
        let raw_input = egui::RawInput {
            pixels_per_point: Some(window_scale_factor as f32),
            screen_rect: Some(egui::Rect::from_min_size(
                Default::default(),
                vec2(window_width as f32, window_height as f32) / window_scale_factor as f32,
            )),
            time: Some(0.0),
            ..Default::default()
        };
    
        let egui_renderer = {
            ash_egui::Renderer::new(
                window_width,
                window_height,
                window_scale_factor,
                &device.raw,
                &device.physical_device().properties,
                &device.physical_device().memory_properties,
                context,
                raw_input.clone(),
            )
        };

        Self {
            device,
            inner: Arc::new(Mutex::new(EguiBackendInner {
                egui_renderer,
                gfx: None,
            })),
            raw_input,
        }
    }

    pub fn create_graphics_resources(&mut self, surface_resolution: [u32; 2]) {
        self.inner
            .lock()
            .create_graphics_resources(self.device.as_ref(), surface_resolution);
    }

    #[allow(dead_code)]
    pub fn destroy_graphics_resources(&mut self) {
        let device = &self.device.raw;

        log::trace!("device_wait_idle");
        unsafe { device.device_wait_idle() }.unwrap();

        let mut inner = self.inner.lock();

        if inner.egui_renderer.has_pipeline() {
            inner.egui_renderer.destroy_pipeline(device);
        }

        if let Some(gfx) = inner.gfx.take() {
            unsafe {
                // TODO
                //device.destroy_render_pass(gfx.egui_render_pass, None);
                device.destroy_framebuffer(gfx.egui_framebuffer, None);
            }
        }
    }

    pub fn handle_event(
        &mut self,
        window: &winit::window::Window,
        egui: &mut ash_egui::egui::Context,
        event: &winit::event::Event<'_, ()>,
    ) {
    }

    pub fn prepare_frame<'a>(
        &mut self,
        context: &mut CtxRef,
        dt: f32,
        // window: &winit::window::Window,
    ) {

        // update time
        if let Some(time) = self.raw_input.time {
            self.raw_input.time = Some(time + dt as f64);
        } else {
            self.raw_input.time = Some(0.0);
        }

        context.begin_frame(self.raw_input.take());
    }

    pub fn finish_frame(
        &mut self,
        context: &mut CtxRef,
        window: &winit::window::Window,
        ui_renderer: &mut UiRenderer,
    ) {
        let ui_target_image =  self.inner.lock().get_target_image().unwrap();

        let inner = self.inner.clone();
        let device = self.device.clone();
        let gui_extent = [window.inner_size().width, window.inner_size().height];

        let (_, clipped_shapes) = context.end_frame();
        let clipped_meshes = context.tessellate(clipped_shapes);

        ui_renderer.ui_frame = Some((
            Box::new(move |cb| {
                inner
                    .lock()
                    .render(gui_extent, clipped_meshes, device, cb)
                    .expect("ui.render");
            }),
            ui_target_image,
        ));
    }
}

impl EguiBackendInner {
    fn create_graphics_resources(&mut self, device: &Device, surface_resolution: [u32; 2]) {
        assert!(self.gfx.is_none());

        let egui_render_pass = create_egui_render_pass(&device.raw);
        let (egui_framebuffer, egui_texture) =
            create_egui_framebuffer(device, egui_render_pass, surface_resolution);

        let gfx = GfxResources {
            egui_render_pass,
            egui_framebuffer,
            egui_texture,
        };

        self.egui_renderer
            .create_pipeline(&device.raw, gfx.egui_render_pass);

        self.gfx = Some(gfx);
    }

    fn get_target_image(&self) -> Option<Arc<Image>> {
        self.gfx.as_ref().map(|res| res.egui_texture.clone())
    }

    fn render(
        &mut self,
        physical_size: [u32; 2],
        draw_data: Vec<ash_egui::egui::ClippedMesh>,
        device: Arc<Device>,
        cb: vk::CommandBuffer,
    ) -> Option<Arc<Image>> {
        let device = &device.raw;

        match self.gfx {
            Some(ref gfx) => {
                /*record_image_barrier(
                    self.device.as_ref(),
                    cb,
                    ImageBarrier::new(
                        gfx.egui_texture.raw,
                        vk_sync::AccessType::Nothing,
                        vk_sync::AccessType::ColorAttachmentWrite,
                        vk::ImageAspectFlags::COLOR,
                    )
                    .with_discard(true),
                );*/

                self.egui_renderer.begin_frame(device, cb);

                {
                    let clear_values = [vk::ClearValue {
                        color: vk::ClearColorValue {
                            float32: [0.0, 0.0, 0.0, 0.0],
                        },
                    }];

                    let render_pass_begin_info = vk::RenderPassBeginInfo::builder()
                        .render_pass(gfx.egui_render_pass)
                        .framebuffer(gfx.egui_framebuffer)
                        .render_area(vk::Rect2D {
                            offset: vk::Offset2D { x: 0, y: 0 },
                            extent: vk::Extent2D {
                                width: physical_size[0],
                                height: physical_size[1],
                            },
                        })
                        .clear_values(&clear_values);

                    unsafe {
                        device.cmd_begin_render_pass(
                            cb,
                            &render_pass_begin_info,
                            vk::SubpassContents::INLINE,
                        );
                    }
                }

                self.egui_renderer.render(draw_data, device, cb);

                unsafe {
                    device.cmd_end_render_pass(cb);
                }

                /*record_image_barrier(
                    self.device.as_ref(),
                    cb,
                    ImageBarrier::new(
                        gfx.egui_texture.raw,
                        vk_sync::AccessType::ColorAttachmentWrite,
                        vk_sync::AccessType::ComputeShaderReadSampledImageOrUniformTexelBuffer,
                        vk::ImageAspectFlags::COLOR,
                    ),
                );*/

                Some(gfx.egui_texture.clone())
            }
            None => None,
        }
    }
}

fn create_egui_render_pass(device: &ash::Device) -> vk::RenderPass {
    let renderpass_attachments = [vk::AttachmentDescription {
        format: vk::Format::R8G8B8A8_UNORM,
        samples: vk::SampleCountFlags::TYPE_1,
        load_op: vk::AttachmentLoadOp::CLEAR,
        store_op: vk::AttachmentStoreOp::STORE,
        final_layout: vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
        ..Default::default()
    }];
    let color_attachment_refs = [vk::AttachmentReference {
        attachment: 0,
        layout: vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
    }];
    let dependencies = [vk::SubpassDependency {
        src_subpass: vk::SUBPASS_EXTERNAL,
        src_stage_mask: vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
        dst_access_mask: vk::AccessFlags::COLOR_ATTACHMENT_READ
            | vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
        dst_stage_mask: vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
        ..Default::default()
    }];

    let subpasses = [vk::SubpassDescription::builder()
        .color_attachments(&color_attachment_refs)
        .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
        .build()];

    let renderpass_create_info = vk::RenderPassCreateInfo::builder()
        .attachments(&renderpass_attachments)
        .subpasses(&subpasses)
        .dependencies(&dependencies);

    unsafe {
        device
            .create_render_pass(&renderpass_create_info, None)
            .unwrap()
    }
}

fn create_egui_framebuffer(
    device: &Device,
    render_pass: vk::RenderPass,
    surface_resolution: [u32; 2],
) -> (vk::Framebuffer, Arc<Image>) {
    //let surface_resolution = vk_state.swapchain.as_ref().unwrap().surface_resolution;

    let tex = device
        .create_image(
            ImageDesc::new_2d(vk::Format::R8G8B8A8_UNORM, surface_resolution)
                .usage(vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::COLOR_ATTACHMENT),
            vec![],
        )
        .unwrap();

    let framebuffer_attachments = [tex.view(device, &ImageViewDesc::default())];
    let frame_buffer_create_info = vk::FramebufferCreateInfo::builder()
        .render_pass(render_pass)
        .attachments(&framebuffer_attachments)
        .width(surface_resolution[0] as _)
        .height(surface_resolution[1] as _)
        .layers(1);

    let fb = unsafe {
        device
            .raw
            .create_framebuffer(&frame_buffer_create_info, None)
    }
    .expect("create_framebuffer");

    (fb, Arc::new(tex))
}

// // Based on https://github.com/ocornut/egui/issues/707#issuecomment-430613104
// fn setup_egui_style(ctx: &mut egui::Context) {
//     let hi = |v: f32| [0.502, 0.075, 0.256, v];
//     let med = |v: f32| [0.455, 0.198, 0.301, v];
//     let low = |v: f32| [0.232, 0.201, 0.271, v];
//     let bg = |v: f32| [0.200, 0.220, 0.270, v];
//     let text = |v: f32| [0.860, 0.930, 0.890, v];

//     let style = ctx.style_mut();
//     style.colors[egui::StyleColor::Text as usize] = text(0.78);
//     style.colors[egui::StyleColor::TextDisabled as usize] = text(0.28);
//     style.colors[egui::StyleColor::WindowBg as usize] = [0.13, 0.14, 0.17, 0.7];
//     style.colors[egui::StyleColor::ChildBg as usize] = bg(0.58);
//     style.colors[egui::StyleColor::PopupBg as usize] = bg(0.9);
//     style.colors[egui::StyleColor::Border as usize] = [0.31, 0.31, 1.00, 0.00];
//     style.colors[egui::StyleColor::BorderShadow as usize] = [0.00, 0.00, 0.00, 0.00];
//     style.colors[egui::StyleColor::FrameBg as usize] = bg(1.00);
//     style.colors[egui::StyleColor::FrameBgHovered as usize] = med(0.78);
//     style.colors[egui::StyleColor::FrameBgActive as usize] = med(1.00);
//     style.colors[egui::StyleColor::TitleBg as usize] = low(1.00);
//     style.colors[egui::StyleColor::TitleBgActive as usize] = hi(1.00);
//     style.colors[egui::StyleColor::TitleBgCollapsed as usize] = bg(0.75);
//     style.colors[egui::StyleColor::MenuBarBg as usize] = bg(0.47);
//     style.colors[egui::StyleColor::ScrollbarBg as usize] = bg(1.00);
//     style.colors[egui::StyleColor::ScrollbarGrab as usize] = [0.09, 0.15, 0.16, 1.00];
//     style.colors[egui::StyleColor::ScrollbarGrabHovered as usize] = med(0.78);
//     style.colors[egui::StyleColor::ScrollbarGrabActive as usize] = med(1.00);
//     style.colors[egui::StyleColor::CheckMark as usize] = [0.71, 0.22, 0.27, 1.00];
//     style.colors[egui::StyleColor::SliderGrab as usize] = [0.47, 0.77, 0.83, 0.14];
//     style.colors[egui::StyleColor::SliderGrabActive as usize] = [0.71, 0.22, 0.27, 1.00];
//     style.colors[egui::StyleColor::Button as usize] = [0.47, 0.77, 0.83, 0.14];
//     style.colors[egui::StyleColor::ButtonHovered as usize] = med(0.86);
//     style.colors[egui::StyleColor::ButtonActive as usize] = med(1.00);
//     style.colors[egui::StyleColor::Header as usize] = med(0.76);
//     style.colors[egui::StyleColor::HeaderHovered as usize] = med(0.86);
//     style.colors[egui::StyleColor::HeaderActive as usize] = hi(1.00);
//     //style.colors[egui::StyleColor::Column as usize] = [0.14, 0.16, 0.19, 1.00];
//     //style.colors[egui::StyleColor::ColumnHovered as usize] = med(0.78);
//     //style.colors[egui::StyleColor::ColumnActive as usize] = med(1.00);
//     style.colors[egui::StyleColor::ResizeGrip as usize] = [0.47, 0.77, 0.83, 0.04];
//     style.colors[egui::StyleColor::ResizeGripHovered as usize] = med(0.78);
//     style.colors[egui::StyleColor::ResizeGripActive as usize] = med(1.00);
//     style.colors[egui::StyleColor::PlotLines as usize] = text(0.63);
//     style.colors[egui::StyleColor::PlotLinesHovered as usize] = med(1.00);
//     style.colors[egui::StyleColor::PlotHistogram as usize] = text(0.63);
//     style.colors[egui::StyleColor::PlotHistogramHovered as usize] = med(1.00);
//     style.colors[egui::StyleColor::TextSelectedBg as usize] = med(0.43);
//     style.colors[egui::StyleColor::ModalWindowDimBg as usize] = bg(0.73);

//     style.window_padding = [6.0, 4.0];
//     style.window_rounding = 0.0;
//     style.frame_padding = [5.0, 2.0];
//     style.frame_rounding = 3.0;
//     style.item_spacing = [7.0, 1.0];
//     style.item_inner_spacing = [1.0, 1.0];
//     style.touch_extra_padding = [0.0, 0.0];
//     style.indent_spacing = 6.0;
//     style.scrollbar_size = 12.0;
//     style.scrollbar_rounding = 16.0;
//     style.grab_min_size = 20.0;
//     style.grab_rounding = 2.0;

//     style.window_title_align[0] = 0.50;

//     style.colors[egui::StyleColor::Border as usize] = [0.539, 0.479, 0.255, 0.162];
//     style.frame_border_size = 0.0;
//     style.window_border_size = 1.0;
// }
