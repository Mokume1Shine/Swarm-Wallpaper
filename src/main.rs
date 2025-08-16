use std::sync::Arc;
use std::time::{Duration, Instant};

use wgpu::util::DeviceExt;
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{Window, WindowId},
};

const NOISE_WGSL: &str = r#"struct Params {
  size:  vec2<f32>, // 8B
  frame: u32,       // +4B
  _pad:  u32,       // +4B → 合計16B（std140でもOK）
}

@group(0) @binding(0) var<uniform> params: Params;

struct VSOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32>, };

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VSOut {
  var p = array<vec2<f32>, 3>(
    vec2<f32>(-1.0, -3.0),
    vec2<f32>(-1.0,  1.0),
    vec2<f32>( 3.0,  1.0)
  );
  var o: VSOut;
  o.pos = vec4<f32>(p[vid], 0.0, 1.0);
  o.uv = (p[vid] * 0.5 + vec2<f32>(0.5, 0.5));
  return o;
}

// 適当ハッシュ（そのままでOK）
fn hash2(p: vec2<f32>, seed: f32) -> f32 {
  let q = vec2<f32>(
    dot(p, vec2<f32>(127.1, 311.7)),
    dot(p, vec2<f32>(269.5, 183.3))
  );
  let s = sin(q + seed) * 43758.5453;
  return fract(sin(dot(s, vec2<f32>(1.0, 7.0))) * 0.5 + 0.5);
}

@fragment
fn fs_main(in: VSOut) -> @location(0) vec4<f32> {
  let coord = in.uv * params.size;
  let n = hash2(coord, f32(params.frame));
  return vec4<f32>(vec3<f32>(n), 1.0);
}"#;

struct State {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    window: Arc<Window>,
    pipeline: wgpu::RenderPipeline,
    params_buf: wgpu::Buffer,
    params_bg: wgpu::BindGroup,
    frame: u32,
}

impl State {
    async fn new(window: Window) -> Self {
        let window = Arc::new(window);
        let size = window.inner_size();

        let instance = wgpu::Instance::default();
        let surface = instance
            .create_surface(window.clone())
            .expect("create surface");
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
            .expect("adapter");

        let (device, queue) = adapter
            .request_device(&Default::default())
            .await
            .expect("device");

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 1,
        };

        surface.configure(&device, &config);

        #[repr(C)]
        #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
        struct Params {
            size: [f32; 2],
            frame: u32,
            _pad: u32,
        }

        let params_init = Params {
            frame: 0,
            _pad: 0,
            size: [config.width as f32, config.height as f32],
        };
        let params_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("params"),
            contents: bytemuck::bytes_of(&params_init),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let params_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bg"),
            layout: &bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: params_buf.as_entire_binding(),
            }],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("noise"),
            source: wgpu::ShaderSource::Wgsl(NOISE_WGSL.into()),
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("pipe"),
            layout: Some(
                &device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("layout"),
                    bind_group_layouts: &[&bgl],
                    push_constant_ranges: &[],
                }),
            ),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        Self {
            surface,
            device,
            queue,
            config,
            window: window.into(),
            pipeline,
            params_buf,
            params_bg,
            frame: 0,
        }
    }

    fn resize(&mut self, w: u32, h: u32) {
        if w == 0 || h == 0 {
            return;
        }
        self.config.width = w;
        self.config.height = h;
        self.surface.configure(&self.device, &self.config);

        #[repr(C)]
        #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
        struct Params {
            size: [f32; 2],
            frame: u32,
            _pad: u32,
        }
        let p = Params {
            frame: self.frame,
            _pad: 0,
            size: [w as f32, h as f32],
        };
        self.queue
            .write_buffer(&self.params_buf, 0, bytemuck::bytes_of(&p));
    }

    fn render(&mut self) -> Result<(), wgpu::SurfaceError> {
        self.frame = self.frame.wrapping_add(1);
        #[repr(C)]
        #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
        struct Params {
            size: [f32; 2],
            frame: u32,
            _pad: u32,
        }
        let p = Params {
            frame: self.frame,
            _pad: 0,
            size: [self.config.width as f32, self.config.height as f32],
        };
        self.queue
            .write_buffer(&self.params_buf, 0, bytemuck::bytes_of(&p));

        let output = self.surface.get_current_texture()?;
        let view = output.texture.create_view(&Default::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("encoder"),
            });
        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("noise"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            rpass.set_pipeline(&self.pipeline);
            rpass.set_bind_group(0, &self.params_bg, &[]);
            rpass.draw(0..3, 0..1);
        }
        self.queue.submit(Some(encoder.finish()));
        output.present();
        Ok(())
    }
}

#[derive(Default)]
struct App {
    state: Option<State>,
    animating: bool,
    fps_frames: u32,
    fps_last: Option<Instant>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window = event_loop
            .create_window(Window::default_attributes().with_title("Swarm Wallpaper"))
            .expect("create window");

        let state = pollster::block_on(State::new(window));

        state.window.set_visible(true);

        self.state = Some(state);
        self.animating = true;
        self.state.as_ref().unwrap().window.request_redraw();
        self.fps_frames = 0;
        self.fps_last = Some(Instant::now());
        self.state.as_ref().unwrap().window.request_redraw();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                self.animating = false;
                if let Some(s) = self.state.as_mut() {
                    let _ = s.device.poll(wgpu::PollType::Wait);
                }
                event_loop.exit();
            }

            WindowEvent::Resized(size) => {
                if let Some(s) = self.state.as_mut() {
                    s.resize(size.width, size.height);
                    // 直後に一度描画
                    s.window.request_redraw();
                }
            }

            WindowEvent::RedrawRequested => {
                if let Some(s) = self.state.as_mut() {
                    match s.render() {
                        Ok(()) => {
                            self.fps_frames += 1;
                            if let Some(t0) = self.fps_last {
                                let dt = t0.elapsed();
                                if dt >= Duration::from_secs(1) {
                                    let fps = (self.fps_frames as f64) / (dt.as_secs_f64());
                                    s.window
                                        .set_title(&format!("Swarm Wallpaper  |  {:.1} FPS", fps));
                                    self.fps_frames = 0;
                                    self.fps_last = Some(Instant::now());
                                }
                            }
                            if self.animating {
                                s.window.request_redraw();
                            }
                        }
                        Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                            let w = s.config.width;
                            let h = s.config.height;
                            s.resize(w, h); // ★再構成
                            if self.animating {
                                s.window.request_redraw();
                            }
                        }
                        Err(wgpu::SurfaceError::OutOfMemory) => {
                            eprintln!("Out of memory — exiting.");
                            self.animating = false;
                            let _ = s.device.poll(wgpu::PollType::Wait);
                            event_loop.exit();
                        }
                        Err(wgpu::SurfaceError::Timeout) => {
                            // スキップでOK。次フレームで回復しがち
                        }
                        Err(e) => {
                            eprintln!("Surface error: {e:?}");
                        }
                    };
                }
            }
            _ => (),
        }
    }
}

fn main() {
    let event_loop = EventLoop::new().unwrap();

    event_loop.set_control_flow(ControlFlow::Wait);

    let mut app = App::default();
    let _ = event_loop.run_app(&mut app);
}
