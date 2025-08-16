use std::time::{Duration, Instant};

use crate::state::State;
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::ActiveEventLoop,
    window::{Window, WindowId},
};

#[derive(Default)]
pub struct App {
    pub state: Option<State>,
    pub animating: bool,
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
