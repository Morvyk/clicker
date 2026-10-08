#![forbid(unsafe_code)]

#[cfg(not(target_os = "linux"))]
compile_error!("Rust Clicker currently uses Linux evdev/uinput and requires Linux.");

mod app;
mod engine;
mod icons;
mod input;
mod model;
mod style;
mod tester;
mod theme;

use iced::{Size, window};

fn main() -> iced::Result {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() && std::env::var_os("WAYLAND_SOCKET").is_none()
    {
        eprintln!("Clicker requires a Wayland session (WAYLAND_DISPLAY or WAYLAND_SOCKET).");
        std::process::exit(1);
    }
    iced::application(app::ClickerApp::new, app::ClickerApp::update, app::ClickerApp::view)
        .title(app::ClickerApp::title)
        .theme(app::ClickerApp::theme)
        .style(app::ClickerApp::style)
        .subscription(app::ClickerApp::subscription)
        .exit_on_close_request(false)
        .window(window::Settings {
            size: Size::new(900.0, 640.0),
            min_size: Some(Size::new(800.0, 520.0)),
            platform_specific: window::settings::PlatformSpecific {
                application_id: "rust-clicker".into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .run()
}
