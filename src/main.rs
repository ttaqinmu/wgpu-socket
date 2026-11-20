mod graphics;
mod network;
mod protocol;

use graphics::{Graphics, RenderError};
use network::{Event as NetEvent, Network, Role};
use protocol::{Message, Scene};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowAttributes};

struct App {
    role: Role,
    path: PathBuf,
    network: Network,
    scene: Scene,
    status: String,
    connected: bool,
    window: Option<Arc<Window>>,
    graphics: Option<Graphics>,
    egui_ctx: egui::Context,
    egui_state: Option<egui_winit::State>,
}

impl App {
    fn drain_network(&mut self) {
        while let Ok(event) = self.network.try_recv() {
            match event {
                NetEvent::State(scene) => self.scene = scene,
                NetEvent::Status { text, connected } => {
                    self.status = text;
                    self.connected = connected;
                }
            }
        }
    }

    fn draw(&mut self, event_loop: &ActiveEventLoop) {
        self.drain_network();
        let Some(window) = &self.window else { return };
        let Some(graphics) = &mut self.graphics else {
            return;
        };
        let Some(egui_state) = &mut self.egui_state else {
            return;
        };
        if window.inner_size().width == 0 || window.inner_size().height == 0 {
            return;
        }

        let input = egui_state.take_egui_input(window);
        let mut updates = Vec::new();
        let role = self.role;
        let status = self.status.clone();
        let connected = self.connected;
        let path = self.path.display().to_string();
        let scene = &mut self.scene;
        let output = self.egui_ctx.run_ui(input, |ui| {
            egui::Panel::left("tools")
                .default_size(250.0)
                .show(ui, |ui| {
                    ui.heading("wgpu + winit + egui");
                    ui.label(format!(
                        "Mode: {}",
                        if role == Role::Host { "host" } else { "join" }
                    ));
                    ui.label(format!("Socket: {path}"));
                    ui.label(format!("Status: {status}"));
                    ui.label(format!("Revision: {}", scene.revision));
                    ui.separator();
                    ui.heading("Cube controls");
                    ui.add_enabled_ui(role == Role::Host || connected, |ui| {
                        let mut rotation_changed = false;
                        for (axis, value) in ["X", "Y", "Z"].into_iter().zip(&mut scene.rotation) {
                            rotation_changed |= ui
                                .add(
                                    egui::Slider::new(value, -180.0..=180.0)
                                        .text(format!("Rotation {axis}")),
                                )
                                .changed();
                        }
                        if rotation_changed {
                            updates.push(Message::SetRotation {
                                value: scene.rotation,
                            });
                        }
                        if ui
                            .add(egui::Slider::new(&mut scene.scale, 0.2..=3.0).text("Scale"))
                            .changed()
                        {
                            updates.push(Message::SetScale { value: scene.scale });
                        }
                        if ui.color_edit_button_rgb(&mut scene.color).changed() {
                            updates.push(Message::SetColor { value: scene.color });
                        }
                    });
                    if role == Role::Join && !connected {
                        ui.small("Controls become available after connecting.");
                    }
                    ui.separator();
                    ui.small("Changes from either window are ordered by the host.");
                });
        });
        egui_state.handle_platform_output(window, output.platform_output.clone());
        for update in updates {
            self.network.send(update);
        }
        match graphics.render(
            &self.scene,
            window.scale_factor() as f32,
            output,
            &self.egui_ctx,
        ) {
            Ok(()) => {}
            Err(RenderError::Reconfigure) => {
                let size = window.inner_size();
                graphics.resize(size.width, size.height);
            }
            Err(RenderError::Fatal) => event_loop.exit(),
            Err(RenderError::Skip) => {}
        }
    }
}

impl ApplicationHandler<()> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let title = format!(
            "wgpu socket demo — {}",
            if self.role == Role::Host {
                "host"
            } else {
                "join"
            }
        );
        let window = match event_loop.create_window(
            WindowAttributes::default()
                .with_title(title)
                .with_inner_size(winit::dpi::LogicalSize::new(1000.0, 700.0)),
        ) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                eprintln!("Cannot create window: {error}");
                event_loop.exit();
                return;
            }
        };
        let graphics = match futures::executor::block_on(Graphics::new(window.clone())) {
            Ok(graphics) => graphics,
            Err(error) => {
                eprintln!("Cannot initialize GPU: {error}");
                event_loop.exit();
                return;
            }
        };
        let egui_state = egui_winit::State::new(
            self.egui_ctx.clone(),
            egui::ViewportId::ROOT,
            window.as_ref(),
            Some(window.scale_factor() as f32),
            None,
            None,
        );
        self.graphics = Some(graphics);
        self.egui_state = Some(egui_state);
        self.window = Some(window);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        if let (Some(window), Some(egui_state)) = (&self.window, &mut self.egui_state) {
            let _ = egui_state.on_window_event(window, &event);
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(graphics) = &mut self.graphics {
                    graphics.resize(size.width, size.height);
                }
            }
            WindowEvent::RedrawRequested => self.draw(event_loop),
            _ => {}
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, _event: ()) {
        self.drain_network();
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + Duration::from_millis(16),
        ));
    }
}

fn usage() -> &'static str {
    "Usage: wgpu-socket <host|join> [--socket PATH]\nDefault socket: /tmp/wgpu-socket-demo.sock"
}

fn args() -> Result<(Role, PathBuf), String> {
    let mut args = std::env::args().skip(1);
    let role = match args.next().as_deref() {
        Some("host") => Role::Host,
        Some("join") => Role::Join,
        Some("--help" | "-h") => return Err(usage().into()),
        _ => return Err(usage().into()),
    };
    let mut path = PathBuf::from("/tmp/wgpu-socket-demo.sock");
    if let Some(option) = args.next() {
        if option != "--socket" {
            return Err(usage().into());
        }
        path = args
            .next()
            .map(PathBuf::from)
            .ok_or_else(|| usage().to_string())?;
    }
    if args.next().is_some() {
        return Err(usage().into());
    }
    Ok((role, path))
}

fn main() {
    let (role, path) = match args() {
        Ok(value) => value,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };
    let event_loop = EventLoop::<()>::with_user_event()
        .build()
        .expect("event loop");
    let network = Network::start(role, path.clone(), event_loop.create_proxy());
    let mut app = App {
        role,
        path,
        network,
        scene: Scene::default(),
        status: "Starting...".into(),
        connected: false,
        window: None,
        graphics: None,
        egui_ctx: egui::Context::default(),
        egui_state: None,
    };
    if let Err(error) = event_loop.run_app(&mut app) {
        eprintln!("Event loop error: {error}");
    }
}
