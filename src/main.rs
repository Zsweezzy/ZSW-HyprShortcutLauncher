mod ipc;
mod ui;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, atomic::AtomicBool};

use gtk4 as gtk;
use gtk4::{gio::prelude::*, glib};

/// Reverse-DNS application id, used by GTK for the window's WM class and for
/// the D-Bus name that makes a second invocation reach the running instance.
const APPLICATION_ID: &str = "dev.hyprland.ShortcutLauncher";

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments
        .iter()
        .any(|argument| argument == "--version" || argument == "-V")
    {
        println!("hypr-shortcut-launcher {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    if arguments
        .iter()
        .any(|argument| argument == "--help" || argument == "-h")
    {
        println!("Usage: hypr-shortcut-launcher [--toggle] [--background]");
        println!("Open or toggle the Hyprland shortcuts and running-apps overlay.");
        println!("--background starts the persistent instance without showing the overlay.");
        return;
    }
    let start_visible = !arguments
        .iter()
        .any(|argument| argument == "--background" || argument == "-b");

    let toggle_requested = Arc::new(AtomicBool::new(false));
    let _toggle_thread = match ipc::acquire_instance() {
        Ok(ipc::AcquiredInstance::Primary(instance)) => {
            Some(instance.spawn_toggle_listener(toggle_requested.clone()))
        }
        Ok(ipc::AcquiredInstance::Secondary) => return,
        Err(error) => {
            eprintln!("could not start launcher instance: {error}");
            return;
        }
    };

    let app = gtk::Application::builder()
        .application_id(APPLICATION_ID)
        .build();
    app.add_main_option(
        "toggle",
        glib::Char::from(b't'),
        glib::OptionFlags::NONE,
        glib::OptionArg::None,
        "Toggle the launcher (the default)",
        None,
    );
    app.add_main_option(
        "background",
        glib::Char::from(b'b'),
        glib::OptionFlags::NONE,
        glib::OptionArg::None,
        "Start the persistent instance without showing the overlay",
        None,
    );
    let ui: Rc<RefCell<Option<ui::UiHandle>>> = Rc::new(RefCell::new(None));

    app.connect_activate({
        let ui = ui.clone();
        let toggle_requested = toggle_requested.clone();
        move |app| {
            if let Some(existing) = ui.borrow().as_ref() {
                existing.toggle();
            } else {
                *ui.borrow_mut() = Some(ui::create_window(
                    app,
                    toggle_requested.clone(),
                    start_visible,
                ));
            }
        }
    });

    // Keep the process alive after the overlay is hidden. The local instance
    // socket forwards later invocations to this process, so Super+K toggles
    // one launcher instead of creating a second copy.
    let _hold = app.hold();
    app.run();
}
