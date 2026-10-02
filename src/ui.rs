use std::{
    cell::{Cell, RefCell},
    env,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use gtk4 as gtk;
use gtk4::{
    gdk,
    gio::prelude::*,
    glib::{self, ControlFlow, Propagation},
    prelude::*,
};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use hypr_shortcut_launcher::{
    AppGroup, InstalledCommand, InstalledRow,
    apps::{append_running_windowless_apps, filter_apps, group_windows},
    hyprland::{self, HyprlandService, ProcessCommandExecutor},
    installed,
    model::{Shortcut, filter_shortcuts},
};

const PANEL_WIDTH: i32 = 900;
const PANEL_HEIGHT: i32 = 650;
const MAIN_MONITOR_ENV: &str = "HYPR_LAUNCHER_MAIN_MONITOR";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActiveTab {
    Shortcuts,
    Apps,
    Installed,
}

impl ActiveTab {
    /// Number of tabs, and so the width of the Tab cycle.
    const COUNT: u32 = 3;

    fn index(self) -> u32 {
        match self {
            ActiveTab::Shortcuts => 0,
            ActiveTab::Apps => 1,
            ActiveTab::Installed => 2,
        }
    }

    fn from_index(index: u32) -> Self {
        match index {
            0 => ActiveTab::Shortcuts,
            1 => ActiveTab::Apps,
            _ => ActiveTab::Installed,
        }
    }

    fn next(self, forward: bool) -> Self {
        let step = if forward { 1 } else { Self::COUNT - 1 };
        Self::from_index((self.index() + step) % Self::COUNT)
    }
}

struct State {
    shortcuts: Vec<Shortcut>,
    apps: Vec<AppGroup>,
    installed: Vec<InstalledCommand>,
    filtered_shortcuts: Vec<Shortcut>,
    filtered_apps: Vec<AppGroup>,
    filtered_installed: Vec<InstalledCommand>,
    /// Section headings interleaved with `filtered_installed`, in the order the
    /// rows are rendered. A row index indexes this, not `filtered_installed`.
    installed_rows: Vec<InstalledRow>,
    active_tab: ActiveTab,
    error: Option<String>,
    has_refresh_result: bool,
}

struct RefreshResult {
    shortcuts: Result<Vec<Shortcut>, String>,
    apps: Result<Vec<AppGroup>, String>,
    /// A `PATH` scan that swallows unreadable directories, so it cannot fail.
    installed: Vec<InstalledCommand>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct RefreshChanges {
    shortcuts: bool,
    apps: bool,
    installed: bool,
    status: bool,
}

struct LauncherUi {
    window: gtk::ApplicationWindow,
    state: Rc<RefCell<State>>,
    search: gtk::SearchEntry,
    notebook: gtk::Notebook,
    shortcut_list: gtk::ListBox,
    app_list: gtk::ListBox,
    installed_list: gtk::ListBox,
    shortcut_scroll: gtk::ScrolledWindow,
    app_scroll: gtk::ScrolledWindow,
    installed_scroll: gtk::ScrolledWindow,
    shortcut_scroll_generation: Rc<Cell<u64>>,
    app_scroll_generation: Rc<Cell<u64>>,
    installed_scroll_generation: Rc<Cell<u64>>,
    shortcut_tab_label: gtk::Label,
    app_tab_label: gtk::Label,
    installed_tab_label: gtk::Label,
    status: gtk::Label,
    detail: gtk::Label,
    service: Arc<HyprlandService<ProcessCommandExecutor>>,
    refresh_result: Arc<Mutex<Option<RefreshResult>>>,
    refresh_in_flight: Arc<AtomicBool>,
    focus_change_requested: Arc<AtomicBool>,
}

pub struct UiHandle(Rc<LauncherUi>);

impl UiHandle {
    pub fn toggle(&self) {
        self.0.toggle();
    }
}

pub fn create_window(
    app: &gtk::Application,
    toggle_requested: Arc<AtomicBool>,
    start_visible: bool,
) -> UiHandle {
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("Shortcuts & Apps")
        .default_width(PANEL_WIDTH)
        .default_height(PANEL_HEIGHT)
        .build();
    window.set_decorated(false);
    window.add_css_class("launcher-window");
    // Keep the layer surface resizable so gtk4-layer-shell can honor all four
    // anchors and expand its transparent hit area across the selected output.
    window.set_resizable(true);

    let main_monitor = select_main_monitor();
    configure_layer_shell(&window, main_monitor.as_ref());
    install_css();

    let state = Rc::new(RefCell::new(State {
        shortcuts: Vec::new(),
        apps: Vec::new(),
        installed: Vec::new(),
        filtered_shortcuts: Vec::new(),
        filtered_apps: Vec::new(),
        filtered_installed: Vec::new(),
        installed_rows: Vec::new(),
        active_tab: ActiveTab::Shortcuts,
        error: None,
        has_refresh_result: false,
    }));

    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some("Search shortcuts, running apps, or commands…"));
    search.set_hexpand(true);
    search.add_css_class("search-entry");

    let shortcut_list = gtk::ListBox::new();
    shortcut_list.set_selection_mode(gtk::SelectionMode::Single);
    shortcut_list.add_css_class("result-list");

    let app_list = gtk::ListBox::new();
    app_list.set_selection_mode(gtk::SelectionMode::Single);
    app_list.set_activate_on_single_click(true);
    app_list.add_css_class("result-list");

    let shortcut_scroll = gtk::ScrolledWindow::new();
    shortcut_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    shortcut_scroll.set_vexpand(true);
    shortcut_scroll.set_child(Some(&shortcut_list));

    let app_scroll = gtk::ScrolledWindow::new();
    app_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    app_scroll.set_vexpand(true);
    app_scroll.set_child(Some(&app_list));

    let installed_list = gtk::ListBox::new();
    installed_list.set_selection_mode(gtk::SelectionMode::Single);
    installed_list.set_activate_on_single_click(false);
    installed_list.add_css_class("result-list");

    let installed_scroll = gtk::ScrolledWindow::new();
    installed_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    installed_scroll.set_vexpand(true);
    installed_scroll.set_child(Some(&installed_list));

    let shortcut_tab_label = gtk::Label::new(Some("Hyprland shortcuts"));
    let app_tab_label = gtk::Label::new(Some("Running apps"));
    let installed_tab_label = gtk::Label::new(Some("Installed apps"));
    let notebook = gtk::Notebook::new();
    notebook.set_show_border(false);
    notebook.set_vexpand(true);
    notebook.append_page(&shortcut_scroll, Some(&shortcut_tab_label));
    notebook.append_page(&app_scroll, Some(&app_tab_label));
    notebook.append_page(&installed_scroll, Some(&installed_tab_label));

    let status = gtk::Label::new(Some("Loading Hyprland data…"));
    status.add_css_class("status");
    status.set_xalign(0.0);

    let detail = gtk::Label::new(Some("Select a shortcut to see its action."));
    detail.add_css_class("detail");
    detail.set_xalign(0.0);
    detail.set_ellipsize(gtk::pango::EllipsizeMode::End);

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let title = gtk::Label::new(Some("Shortcuts & Apps"));
    title.add_css_class("title");
    header.append(&title);
    header.append(&search);

    let footer = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let hints = gtk::Label::new(Some(
        "Tab / Shift+Tab switch  •  ↑↓ select  •  Enter act  •  Esc close",
    ));
    hints.add_css_class("hints");
    hints.set_xalign(0.0);
    hints.set_hexpand(true);
    footer.append(&hints);
    footer.append(&status);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 10);
    content.set_margin_start(22);
    content.set_margin_end(22);
    content.set_margin_top(20);
    content.set_margin_bottom(16);
    content.append(&header);
    content.append(&notebook);
    content.append(&detail);
    content.append(&footer);

    let panel = gtk::Box::new(gtk::Orientation::Vertical, 0);
    panel.add_css_class("launcher-panel");
    panel.set_size_request(PANEL_WIDTH, PANEL_HEIGHT);
    panel.set_halign(gtk::Align::Center);
    panel.set_valign(gtk::Align::Center);
    panel.append(&content);

    let overlay = gtk::Overlay::new();
    overlay.add_css_class("launcher-overlay");
    overlay.set_can_target(true);
    overlay.set_hexpand(true);
    overlay.set_vexpand(true);
    overlay.set_child(Some(&panel));
    window.set_child(Some(&overlay));

    let focus_change_requested = Arc::new(AtomicBool::new(false));
    hyprland::spawn_focus_change_listener(focus_change_requested.clone());
    let refresh_result = Arc::new(Mutex::new(None));
    let refresh_in_flight = Arc::new(AtomicBool::new(false));

    let ui = Rc::new(LauncherUi {
        window: window.clone(),
        state,
        search,
        notebook,
        shortcut_list,
        app_list,
        installed_list,
        shortcut_scroll: shortcut_scroll.clone(),
        app_scroll: app_scroll.clone(),
        installed_scroll: installed_scroll.clone(),
        shortcut_scroll_generation: Rc::new(Cell::new(0)),
        app_scroll_generation: Rc::new(Cell::new(0)),
        installed_scroll_generation: Rc::new(Cell::new(0)),
        shortcut_tab_label,
        app_tab_label,
        installed_tab_label,
        status,
        detail,
        service: Arc::new(HyprlandService::default()),
        refresh_result,
        refresh_in_flight,
        focus_change_requested,
    });

    {
        let callback_ui = ui.clone();
        ui.search.connect_search_changed(move |_| {
            callback_ui.rebuild_lists();
        });
    }

    {
        let callback_ui = ui.clone();
        ui.notebook.connect_switch_page(move |_, _, page| {
            {
                let mut state = callback_ui.state.borrow_mut();
                state.active_tab = ActiveTab::from_index(page);
            }
            callback_ui.update_status();
        });
    }

    {
        let callback_ui = ui.clone();
        ui.shortcut_list.connect_row_activated(move |_, _| {
            callback_ui.show_selected_shortcut();
        });
    }

    {
        let callback_ui = ui.clone();
        ui.app_list.connect_row_activated(move |_, _| {
            callback_ui.activate_selected_app();
        });
    }

    {
        let callback_ui = ui.clone();
        ui.installed_list.connect_row_activated(move |_, _| {
            callback_ui.activate_selected_command();
        });
    }

    {
        let callback_ui = ui.clone();
        let click_overlay = overlay.clone();
        let click_panel = panel.clone();
        let click = gtk::GestureClick::new();
        click.set_button(1);
        click.set_propagation_phase(gtk::PropagationPhase::Capture);
        click.set_propagation_limit(gtk::PropagationLimit::None);
        click.connect_pressed(move |_, _, x, y| {
            if !point_is_inside_panel(&click_overlay, &click_panel, x, y) {
                callback_ui.hide();
            }
        });
        overlay.add_controller(click);
    }

    {
        let callback_ui = ui.clone();
        let controller = gtk::EventControllerKey::new();
        // Capture keys before the focused search entry can consume Escape or
        // navigation shortcuts. Returning Proceed below still allows normal
        // text input to reach the entry.
        controller.set_propagation_phase(gtk::PropagationPhase::Capture);
        controller.set_propagation_limit(gtk::PropagationLimit::None);
        controller.connect_key_pressed(move |_, key, _, modifiers| {
            if callback_ui.handle_key(key, modifiers) {
                Propagation::Stop
            } else {
                Propagation::Proceed
            }
        });
        ui.window.add_controller(controller);
    }

    {
        let callback_ui = ui.clone();
        ui.window.connect_close_request(move |_| {
            callback_ui.hide();
            Propagation::Stop
        });
    }

    if start_visible {
        ui.show();
    } else {
        if main_monitor.is_some() {
            // Prime the layer surface while the daemon starts hidden, then keep
            // the window ready for the first Super+K press without a visible flash.
            ui.window.set_opacity(0.0);
            ui.window.present();
            ui.window.hide();
            ui.window.set_opacity(1.0);
        }
        // Warm the data cache while the daemon is hidden in the background.
        ui.start_refresh();
    }

    let refresh_ui = ui.clone();
    glib::timeout_add_local(Duration::from_secs(2), move || {
        if refresh_ui.window.is_visible() {
            refresh_ui.start_refresh();
        }
        ControlFlow::Continue
    });

    let event_ui = ui.clone();
    glib::timeout_add_local(Duration::from_millis(8), move || {
        let result = {
            let mut slot = event_ui
                .refresh_result
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            slot.take()
        };
        if let Some(result) = result {
            event_ui.apply_refresh(result);
        }

        if event_ui
            .focus_change_requested
            .swap(false, Ordering::SeqCst)
            && event_ui.window.is_visible()
        {
            event_ui.hide();
        }

        if toggle_requested.swap(false, Ordering::SeqCst) {
            event_ui.toggle();
        }
        ControlFlow::Continue
    });

    UiHandle(ui)
}

fn configure_layer_shell(window: &gtk::ApplicationWindow, monitor: Option<&gdk::Monitor>) {
    window.init_layer_shell();
    window.set_layer(Layer::Overlay);
    window.set_keyboard_mode(KeyboardMode::Exclusive);
    window.set_namespace(Some("hypr-shortcut-launcher"));
    for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
        window.set_anchor(edge, true);
        window.set_margin(edge, 0);
    }
    window.set_monitor(monitor);
    window.set_exclusive_zone(0);
}

fn select_main_monitor() -> Option<gdk::Monitor> {
    let display = gdk::Display::default()?;
    let model = display.monitors();
    let monitors: Vec<_> = (0..model.n_items())
        .filter_map(|index| model.item(index)?.downcast::<gdk::Monitor>().ok())
        .collect();

    let preferred = env::var(MAIN_MONITOR_ENV)
        .ok()
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty());
    if let Some(preferred) = preferred.as_deref()
        && let Some(monitor) = monitors.iter().find(|monitor| {
            monitor
                .connector()
                .is_some_and(|connector| connector == preferred)
        })
    {
        return Some(monitor.clone());
    }

    monitors.into_iter().max_by_key(monitor_priority)
}

fn monitor_priority(monitor: &gdk::Monitor) -> (i64, i64) {
    let physical_area = i64::from(monitor.width_mm()) * i64::from(monitor.height_mm());
    let geometry = monitor.geometry();
    let logical_area = i64::from(geometry.width()) * i64::from(geometry.height());
    (physical_area, logical_area)
}

fn point_is_inside_panel(overlay: &gtk::Overlay, panel: &gtk::Box, x: f64, y: f64) -> bool {
    let width = panel.allocated_width();
    let height = panel.allocated_height();
    if width <= 0 || height <= 0 {
        return true;
    }
    let Some((panel_x, panel_y)) = panel.translate_coordinates(overlay, 0.0, 0.0) else {
        return true;
    };
    x >= panel_x
        && y >= panel_y
        && x < panel_x + f64::from(width)
        && y < panel_y + f64::from(height)
}

fn scroll_row_into_view(
    scroll: &gtk::ScrolledWindow,
    list: &gtk::ListBox,
    row: &gtk::ListBoxRow,
    generation: &Rc<Cell<u64>>,
) {
    // Rows are rebuilt while the window may still be laying out. Retry once on
    // the next idle turn so the first keyboard interaction is not lost.
    if row.allocated_height() <= 0 || scroll.vadjustment().page_size() <= 0.0 {
        let scheduled_generation = generation.get();
        let scroll = scroll.clone();
        let list = list.clone();
        let row = row.clone();
        let generation = generation.clone();
        glib::idle_add_local_once(move || {
            if generation.get() == scheduled_generation {
                scroll_row_into_view_now(&scroll, &list, &row);
            }
        });
        return;
    }

    scroll_row_into_view_now(scroll, list, row);
}

fn scroll_row_into_view_now(
    scroll: &gtk::ScrolledWindow,
    list: &gtk::ListBox,
    row: &gtk::ListBoxRow,
) {
    let Some((_, row_top)) = row.translate_coordinates(list, 0.0, 0.0) else {
        return;
    };
    let adjustment = scroll.vadjustment();
    let Some(value) = scroll_value_for_row(
        adjustment.value(),
        adjustment.page_size(),
        row_top,
        f64::from(row.allocated_height()),
    ) else {
        return;
    };
    adjustment.set_value(value);
}

fn scroll_value_for_row(
    current: f64,
    page_size: f64,
    row_top: f64,
    row_height: f64,
) -> Option<f64> {
    if !current.is_finite()
        || !page_size.is_finite()
        || !row_top.is_finite()
        || !row_height.is_finite()
        || page_size <= 0.0
        || row_height <= 0.0
    {
        return None;
    }

    let row_bottom = row_top + row_height;
    if row_top < current {
        Some(row_top.max(0.0))
    } else if row_bottom > current + page_size {
        Some((row_bottom - page_size).max(0.0))
    } else {
        None
    }
}

fn install_css() {
    let css = r#"
    window.launcher-window {
        background: transparent;
    }
    window.launcher-window .launcher-overlay {
        background: transparent;
    }
    .launcher-overlay {
        background: transparent;
    }
    .launcher-panel {
        background: rgba(22, 22, 30, 0.94);
        border: 1px solid #292e42;
        border-radius: 20px;
        color: #c0caf5;
    }
    .title {
        color: #bb9af7;
        font-size: 20px;
        font-weight: 700;
    }
    .search-entry {
        background: rgba(26, 27, 38, 0.84);
        color: #c0caf5;
        border: 1px solid #414868;
        border-radius: 10px;
        padding: 8px 12px;
    }
    notebook > header {
        background: rgba(26, 27, 38, 0.84);
    }
    notebook > header tab {
        color: #7aa2f7;
        padding: 8px 14px;
    }
    notebook > header tab:checked {
        color: #bb9af7;
        font-weight: 700;
    }
    .result-list {
        background: rgba(26, 27, 38, 0.78);
        border: 1px solid #292e42;
        border-radius: 12px;
        padding: 6px;
    }
    .result-row {
        padding: 2px 0;
    }
    .result-row:selected {
        background: #283457;
        border-radius: 8px;
    }
    .keycap {
        color: #7aa2f7;
        font-family: monospace;
        font-size: 14px;
        font-weight: 700;
        min-width: 170px;
    }
    .action {
        color: #c0caf5;
    }
    .metadata {
        color: #565f89;
        font-size: 12px;
    }
    .app-name {
        color: #9ece6a;
        font-size: 15px;
        font-weight: 700;
        min-width: 170px;
    }
    .app-detail {
        color: #a9b1d6;
    }
    .section-heading {
        color: #7aa2f7;
        font-size: 12px;
        font-weight: 700;
        text-transform: uppercase;
    }
    .status, .detail, .hints {
        color: #565f89;
        font-size: 12px;
    }
    .detail {
        min-height: 18px;
    }
    "#;

    let provider = gtk::CssProvider::new();
    provider.load_from_data(css);
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(&display, &provider, 1000);
    }
}

impl LauncherUi {
    fn toggle(&self) {
        if self.window.is_visible() {
            self.hide();
        } else {
            self.show();
        }
    }

    fn show(&self) {
        self.focus_change_requested.store(false, Ordering::SeqCst);

        // Present the shell before querying Hyprland. Both hyprctl calls are
        // subprocesses and must not delay the first frame of the launcher.
        self.window.present();
        self.search.grab_focus();
        self.start_refresh();
    }

    fn hide(&self) {
        self.focus_change_requested.store(false, Ordering::SeqCst);
        self.window.hide();
    }

    fn start_refresh(&self) {
        if self.refresh_in_flight.swap(true, Ordering::SeqCst) {
            return;
        }

        let service = self.service.clone();
        let result_slot = self.refresh_result.clone();
        let in_flight = self.refresh_in_flight.clone();
        std::thread::spawn(move || {
            let result = RefreshResult {
                shortcuts: service.bindings().map_err(|error| error.to_string()),
                apps: service
                    .clients()
                    .map(|windows| {
                        let mut apps = group_windows(windows);
                        append_running_windowless_apps(&mut apps);
                        apps
                    })
                    .map_err(|error| error.to_string()),
                installed: installed::scan(),
            };
            {
                let mut slot = result_slot
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                *slot = Some(result);
            }
            in_flight.store(false, Ordering::SeqCst);
        });
    }

    fn apply_refresh(&self, result: RefreshResult) {
        let changes = merge_refresh_result(&mut self.state.borrow_mut(), result);
        if changes.shortcuts || changes.apps || changes.installed {
            self.rebuild_lists_with_changes(changes);
        } else if changes.status {
            self.update_status();
        }
    }

    fn rebuild_lists(&self) {
        self.rebuild_lists_with_changes(RefreshChanges {
            shortcuts: true,
            apps: true,
            installed: true,
            status: true,
        });
    }

    fn rebuild_lists_with_changes(&self, changes: RefreshChanges) {
        let (shortcuts_changed, apps_changed, installed_changed) =
            (changes.shortcuts, changes.apps, changes.installed);
        let selected_shortcut_index = if shortcuts_changed {
            selected_row_index(&self.shortcut_list)
        } else {
            None
        };
        let selected_shortcut = if shortcuts_changed {
            let state = self.state.borrow();
            selected_shortcut_index.and_then(|index| state.filtered_shortcuts.get(index).cloned())
        } else {
            None
        };
        let selected_app_index = if apps_changed {
            selected_row_index(&self.app_list)
        } else {
            None
        };
        let selected_app_class = if apps_changed {
            let state = self.state.borrow();
            selected_app_index
                .and_then(|index| state.filtered_apps.get(index).map(|app| app.class.clone()))
        } else {
            None
        };

        let query = self.search.text().to_string();
        let (shortcuts, apps, installed, active_tab) = {
            let state = self.state.borrow();
            let shortcuts = if shortcuts_changed {
                filter_shortcuts(&state.shortcuts, &query)
            } else {
                state.filtered_shortcuts.clone()
            };
            let apps = if apps_changed {
                filter_apps(&state.apps, &query)
            } else {
                state.filtered_apps.clone()
            };
            let installed = if installed_changed {
                installed::filter(&state.installed, &query)
            } else {
                state.filtered_installed.clone()
            };
            (shortcuts, apps, installed, state.active_tab)
        };
        let shortcut_index = if shortcuts_changed {
            resolve_list_index(
                preserved_shortcut_index(&shortcuts, selected_shortcut.as_ref()),
                selected_shortcut_index,
                shortcuts.len(),
            )
        } else {
            None
        };
        let app_index = if apps_changed {
            resolve_list_index(
                preserved_app_index(&apps, selected_app_class.as_deref()),
                selected_app_index,
                apps.len(),
            )
        } else {
            None
        };
        // Row indices count headings, so resolve the selection against the
        // rendered rows and hand back a row index.
        let installed_rows = installed::rows(&installed);
        let installed_index = if installed_changed {
            let previous = selected_row_index(&self.installed_list);
            let previous_name = previous.and_then(|index| {
                self.state
                    .borrow()
                    .installed_rows
                    .get(index)
                    .and_then(InstalledRow::command)
                    .map(|command| command.name.clone())
            });
            resolve_list_index(
                preserved_installed_index(&installed_rows, previous_name.as_deref()),
                previous,
                installed_rows.len(),
            )
        } else {
            None
        };
        // The installed list opens with a section heading, which cannot be run.
        // Land the selection on the first command instead so Enter works right
        // after typing a query, the same as in the other tabs.
        let installed_index =
            installed_index.and_then(|index| command_row_at_or_after(&installed_rows, index));

        {
            let mut state = self.state.borrow_mut();
            if shortcuts_changed {
                state.filtered_shortcuts = shortcuts;
            }
            if apps_changed {
                state.filtered_apps = apps;
            }
            if installed_changed {
                state.filtered_installed = installed;
                state.installed_rows = installed_rows;
            }
        }

        if shortcuts_changed {
            self.shortcut_scroll_generation
                .set(self.shortcut_scroll_generation.get().wrapping_add(1));
            clear_list(&self.shortcut_list);
            let state = self.state.borrow();
            if state.filtered_shortcuts.is_empty() {
                add_empty_row(&self.shortcut_list, "No matching Hyprland shortcuts");
            } else {
                for shortcut in &state.filtered_shortcuts {
                    self.shortcut_list.append(&shortcut_row(shortcut));
                }
            }
        }

        if apps_changed {
            self.app_scroll_generation
                .set(self.app_scroll_generation.get().wrapping_add(1));
            clear_list(&self.app_list);
            let state = self.state.borrow();
            if state.filtered_apps.is_empty() {
                add_empty_row(&self.app_list, "No matching running applications");
            } else {
                for app in &state.filtered_apps {
                    self.app_list.append(&app_row(app));
                }
            }
        }

        if installed_changed {
            self.installed_scroll_generation
                .set(self.installed_scroll_generation.get().wrapping_add(1));
            clear_list(&self.installed_list);
            let state = self.state.borrow();
            if state.installed_rows.is_empty() {
                add_empty_row(&self.installed_list, "No matching installed commands");
            } else {
                for row in &state.installed_rows {
                    self.installed_list.append(&installed_row(row));
                }
            }
        }

        select_and_scroll(
            &self.shortcut_list,
            &self.shortcut_scroll,
            &self.shortcut_scroll_generation,
            shortcut_index,
        );
        select_and_scroll(
            &self.app_list,
            &self.app_scroll,
            &self.app_scroll_generation,
            app_index,
        );
        select_and_scroll(
            &self.installed_list,
            &self.installed_scroll,
            &self.installed_scroll_generation,
            installed_index,
        );

        let (shortcut_count, app_count, installed_count) = {
            let state = self.state.borrow();
            (
                state.filtered_shortcuts.len(),
                state.filtered_apps.len(),
                state.filtered_installed.len(),
            )
        };
        self.shortcut_tab_label
            .set_text(&format!("Hyprland shortcuts ({shortcut_count})"));
        self.app_tab_label
            .set_text(&format!("Running apps ({app_count})"));
        self.installed_tab_label
            .set_text(&format!("Installed apps ({installed_count})"));
        self.notebook.set_current_page(Some(active_tab.index()));
        self.update_status();
    }

    fn update_status(&self) {
        let state = self.state.borrow();
        let text = if let Some(error) = &state.error {
            format!("Hyprland unavailable: {error}")
        } else {
            format!(
                "{} shortcuts  •  {} running apps  •  {} installed",
                state.filtered_shortcuts.len(),
                state.filtered_apps.len(),
                state.filtered_installed.len()
            )
        };
        self.status.set_text(&text);
    }

    fn handle_key(&self, key: gdk::Key, modifiers: gdk::ModifierType) -> bool {
        let control = modifiers.contains(gdk::ModifierType::CONTROL_MASK);
        let alt = modifiers.contains(gdk::ModifierType::ALT_MASK);

        if is_super_focus_navigation_key(key, modifiers) {
            self.hide();
            return true;
        }

        // The Hyprland binding and IPC own the actual Super+K toggle. If the
        // compositor also forwards the key to GTK, consume it without toggling
        // a second time (or inserting a stray "k" into the search entry).
        if key == gdk::Key::k && modifiers.contains(gdk::ModifierType::SUPER_MASK) {
            return true;
        }

        if key == gdk::Key::Escape {
            self.hide();
            return true;
        }

        if let Some(forward) = tab_switch_direction(key, modifiers) {
            self.switch_tab(forward);
            return true;
        }

        if alt && key == gdk::Key::_1 {
            self.switch_to_tab(ActiveTab::Shortcuts);
            return true;
        }
        if alt && key == gdk::Key::_2 {
            self.switch_to_tab(ActiveTab::Apps);
            return true;
        }
        if alt && key == gdk::Key::_3 {
            self.switch_to_tab(ActiveTab::Installed);
            return true;
        }
        if control && key == gdk::Key::r {
            self.start_refresh();
            return true;
        }
        if key == gdk::Key::Up || (control && key == gdk::Key::k) {
            self.move_selection(-1);
            return true;
        }
        if key == gdk::Key::Down || (control && key == gdk::Key::j) {
            self.move_selection(1);
            return true;
        }
        if key == gdk::Key::Return || key == gdk::Key::KP_Enter || key == gdk::Key::ISO_Enter {
            match self.state.borrow().active_tab {
                ActiveTab::Shortcuts => self.show_selected_shortcut(),
                ActiveTab::Apps => self.activate_selected_app(),
                ActiveTab::Installed => self.activate_selected_command(),
            }
            return true;
        }
        false
    }

    fn switch_tab(&self, forward: bool) {
        let next = self.state.borrow().active_tab.next(forward);
        self.switch_to_tab(next);
    }

    fn switch_to_tab(&self, tab: ActiveTab) {
        self.notebook.set_current_page(Some(tab.index()));
        self.search.grab_focus();
    }

    fn move_selection(&self, direction: i32) {
        let (list, scroll, generation, count) = {
            let state = self.state.borrow();
            match state.active_tab {
                ActiveTab::Shortcuts => (
                    self.shortcut_list.clone(),
                    self.shortcut_scroll.clone(),
                    self.shortcut_scroll_generation.clone(),
                    state.filtered_shortcuts.len(),
                ),
                ActiveTab::Apps => (
                    self.app_list.clone(),
                    self.app_scroll.clone(),
                    self.app_scroll_generation.clone(),
                    state.filtered_apps.len(),
                ),
                ActiveTab::Installed => (
                    self.installed_list.clone(),
                    self.installed_scroll.clone(),
                    self.installed_scroll_generation.clone(),
                    state.installed_rows.len(),
                ),
            }
        };
        if count == 0 {
            return;
        }

        let current = list
            .selected_row()
            .map(|row| row.index())
            .unwrap_or(if direction > 0 { -1 } else { count as i32 });
        // Section headings are rows like any other, so step past the ones that
        // cannot be selected rather than letting the cursor rest on a label.
        let mut next = (current + direction).rem_euclid(count as i32);
        for _ in 0..count {
            let Some(row) = list.row_at_index(next) else {
                return;
            };
            if row.is_activatable() {
                generation.set(generation.get().wrapping_add(1));
                list.select_row(Some(&row));
                scroll_row_into_view(&scroll, &list, &row, &generation);
                return;
            }
            next = (next + direction).rem_euclid(count as i32);
        }
    }

    fn show_selected_shortcut(&self) {
        let index = self
            .shortcut_list
            .selected_row()
            .map(|row| row.index())
            .unwrap_or(0) as usize;
        let state = self.state.borrow();
        let Some(shortcut) = state.filtered_shortcuts.get(index) else {
            return;
        };
        let mut detail = format!("{}  —  {}", shortcut.display_key(), shortcut.action_label());
        if shortcut.locked {
            detail.push_str("  [locked]");
        }
        if shortcut.repeat {
            detail.push_str("  [repeats]");
        }
        self.detail.set_text(&detail);
    }

    fn activate_selected_app(&self) {
        let index = self
            .app_list
            .selected_row()
            .map(|row| row.index())
            .unwrap_or(0) as usize;
        let address = {
            let state = self.state.borrow();
            state
                .filtered_apps
                .get(index)
                .and_then(|app| app.latest_window())
                .map(|window| window.address.clone())
        };
        let Some(address) = address else {
            return;
        };

        match self.service.focus_window(&address) {
            Ok(()) => self.hide(),
            Err(error) => self
                .detail
                .set_text(&format!("Could not focus app: {error}")),
        }
    }

    fn activate_selected_command(&self) {
        let index = self
            .installed_list
            .selected_row()
            .map(|row| row.index())
            .unwrap_or(0) as usize;
        let name = {
            let state = self.state.borrow();
            state
                .installed_rows
                .get(index)
                .and_then(InstalledRow::command)
                .map(|command| command.name.clone())
        };
        let Some(name) = name else {
            return;
        };

        match installed::launch(&name) {
            Ok(()) => self.hide(),
            Err(error) => self.detail.set_text(&error),
        }
    }
}

/// Selects a row and brings it into view, if the list has that many rows.
fn select_and_scroll(
    list: &gtk::ListBox,
    scroll: &gtk::ScrolledWindow,
    generation: &Rc<Cell<u64>>,
    index: Option<usize>,
) {
    let Some(index) = index else {
        return;
    };
    let Ok(index) = i32::try_from(index) else {
        return;
    };
    let Some(row) = list.row_at_index(index) else {
        return;
    };
    list.select_row(Some(&row));
    scroll_row_into_view(scroll, list, &row, generation);
}

fn merge_refresh_result(state: &mut State, result: RefreshResult) -> RefreshChanges {
    let RefreshResult {
        shortcuts,
        apps,
        installed,
    } = result;
    let first_result = !state.has_refresh_result;
    let shortcuts_changed = shortcuts
        .as_ref()
        .is_ok_and(|updated| state.shortcuts.as_slice() != updated.as_slice());
    let apps_changed = apps
        .as_ref()
        .is_ok_and(|updated| state.apps.as_slice() != updated.as_slice());
    let installed_changed = state.installed != installed;
    let previous_error = state.error.clone();

    let mut errors = Vec::new();
    match shortcuts {
        Ok(shortcuts) => state.shortcuts = shortcuts,
        Err(error) => errors.push(format!("shortcuts: {error}")),
    }
    match apps {
        Ok(apps) => state.apps = apps,
        Err(error) => errors.push(format!("clients: {error}")),
    }
    state.installed = installed;
    state.error = (!errors.is_empty()).then(|| errors.join("; "));
    state.has_refresh_result = true;

    RefreshChanges {
        shortcuts: first_result || shortcuts_changed,
        apps: first_result || apps_changed,
        installed: first_result || installed_changed,
        status: previous_error.as_deref() != state.error.as_deref(),
    }
}

fn clear_list(list: &gtk::ListBox) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
}

fn add_empty_row(list: &gtk::ListBox, text: &str) {
    let row = gtk::ListBoxRow::new();
    row.set_activatable(false);
    let label = gtk::Label::new(Some(text));
    label.add_css_class("metadata");
    label.set_margin_top(18);
    label.set_margin_bottom(18);
    label.set_margin_start(14);
    row.set_child(Some(&label));
    list.append(&row);
}

fn shortcut_row(shortcut: &Shortcut) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.add_css_class("result-row");

    let layout = gtk::Box::new(gtk::Orientation::Horizontal, 14);
    layout.set_margin_top(8);
    layout.set_margin_bottom(8);
    layout.set_margin_start(12);
    layout.set_margin_end(12);

    let key = gtk::Label::new(Some(&shortcut.display_key()));
    key.add_css_class("keycap");
    key.set_xalign(0.0);

    let action = gtk::Label::new(Some(&shortcut.action_label()));
    action.add_css_class("action");
    action.set_xalign(0.0);
    action.set_hexpand(true);
    action.set_ellipsize(gtk::pango::EllipsizeMode::End);

    let mut metadata = Vec::new();
    if shortcut.locked {
        metadata.push("locked");
    }
    if shortcut.repeat {
        metadata.push("repeat");
    }
    if shortcut.release {
        metadata.push("release");
    }
    if shortcut.non_consuming {
        metadata.push("non-consuming");
    }
    let meta = gtk::Label::new(Some(&metadata.join("  •  ")));
    meta.add_css_class("metadata");
    meta.set_xalign(1.0);

    layout.append(&key);
    layout.append(&action);
    layout.append(&meta);
    row.set_child(Some(&layout));
    row
}

/// One line of the installed-apps tab: a section heading, or a runnable command.
fn installed_row(item: &InstalledRow) -> gtk::ListBoxRow {
    let InstalledRow::Command(command) = item else {
        let row = gtk::ListBoxRow::new();
        row.add_css_class("result-row");
        row.set_activatable(false);
        let heading = gtk::Label::new(Some(match item {
            InstalledRow::Section(label) => label,
            InstalledRow::Command(_) => unreachable!("handled above"),
        }));
        heading.add_css_class("section-heading");
        heading.set_xalign(0.0);
        heading.set_margin_top(14);
        heading.set_margin_bottom(4);
        heading.set_margin_start(12);
        row.set_child(Some(&heading));
        return row;
    };

    let row = gtk::ListBoxRow::new();
    row.add_css_class("result-row");

    let layout = gtk::Box::new(gtk::Orientation::Horizontal, 14);
    layout.set_margin_top(8);
    layout.set_margin_bottom(8);
    layout.set_margin_start(12);
    layout.set_margin_end(12);

    let name = gtk::Label::new(Some(&command.name));
    name.add_css_class("app-name");
    name.set_xalign(0.0);

    let description = gtk::Label::new(command.description.as_deref());
    description.add_css_class("app-detail");
    description.set_xalign(0.0);
    description.set_hexpand(true);
    description.set_ellipsize(gtk::pango::EllipsizeMode::End);

    let hint = gtk::Label::new(Some("Enter to run"));
    hint.add_css_class("metadata");

    layout.append(&name);
    layout.append(&description);
    layout.append(&hint);
    row.set_child(Some(&layout));
    row
}

fn app_row(app: &AppGroup) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.add_css_class("result-row");

    let layout = gtk::Box::new(gtk::Orientation::Horizontal, 14);
    layout.set_margin_top(8);
    layout.set_margin_bottom(8);
    layout.set_margin_start(12);
    layout.set_margin_end(12);

    let name = gtk::Label::new(Some(&app.display_name));
    name.add_css_class("app-name");
    name.set_xalign(0.0);

    let latest = app.latest_window();
    let title = latest
        .map(|window| {
            if window.title.trim().is_empty() {
                window.class.clone()
            } else {
                truncate(&window.title, 90)
            }
        })
        .unwrap_or_else(|| app.class.clone());
    let count = if app.windows.len() == 1 {
        "1 window".to_owned()
    } else {
        format!("{} windows", app.windows.len())
    };
    let workspace = latest
        .map(|window| format!(" · workspace {}", window.workspace))
        .unwrap_or_default();
    // A windowless app has nothing for Enter to focus, so its row advertises the
    // global hotkey that drives it instead of a window title.
    let (description, hint) = match &app.shortcut {
        Some(shortcut) => (shortcut.clone(), "display only"),
        None => (format!("{title}  ·  {count}{workspace}"), "Enter to focus"),
    };
    let detail = gtk::Label::new(Some(&description));
    detail.add_css_class("app-detail");
    detail.set_xalign(0.0);
    detail.set_hexpand(true);
    detail.set_ellipsize(gtk::pango::EllipsizeMode::End);

    let hint = gtk::Label::new(Some(hint));
    hint.add_css_class("metadata");

    layout.append(&name);
    layout.append(&detail);
    layout.append(&hint);
    row.set_child(Some(&layout));
    row
}

fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    let mut result: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    result.push('…');
    result
}

fn selected_row_index(list: &gtk::ListBox) -> Option<usize> {
    list.selected_row()
        .and_then(|row| usize::try_from(row.index()).ok())
}

fn resolve_list_index(
    preserved: Option<usize>,
    previous: Option<usize>,
    length: usize,
) -> Option<usize> {
    if length == 0 {
        return None;
    }

    preserved
        .or_else(|| previous.map(|index| index.min(length - 1)))
        .or(Some(0))
}

fn preserved_shortcut_index(shortcuts: &[Shortcut], selected: Option<&Shortcut>) -> Option<usize> {
    selected.and_then(|selected| {
        shortcuts.iter().position(|shortcut| {
            shortcut.modmask == selected.modmask && shortcut.key == selected.key
        })
    })
}

fn preserved_app_index(apps: &[AppGroup], selected_class: Option<&str>) -> Option<usize> {
    selected_class.and_then(|selected| apps.iter().position(|app| app.class == selected))
}

/// Same idea as `preserved_app_index`, but over rows that carry headings, so a
/// selected command stays selected when a heading appears above it.
fn preserved_installed_index(rows: &[InstalledRow], selected_name: Option<&str>) -> Option<usize> {
    selected_name.and_then(|selected| {
        rows.iter().position(|row| {
            row.command()
                .is_some_and(|command| command.name == selected)
        })
    })
}

/// First runnable row at or after `start`, skipping section headings.
fn command_row_at_or_after(rows: &[InstalledRow], start: usize) -> Option<usize> {
    (start..rows.len()).find(|&index| rows[index].command().is_some())
}

fn tab_switch_direction(key: gdk::Key, modifiers: gdk::ModifierType) -> Option<bool> {
    if modifiers.contains(gdk::ModifierType::ALT_MASK)
        || modifiers.contains(gdk::ModifierType::SUPER_MASK)
    {
        return None;
    }

    match key {
        gdk::Key::Tab => Some(!modifiers.contains(gdk::ModifierType::SHIFT_MASK)),
        gdk::Key::ISO_Left_Tab => Some(false),
        _ => None,
    }
}

fn is_super_focus_navigation_key(key: gdk::Key, modifiers: gdk::ModifierType) -> bool {
    modifiers.contains(gdk::ModifierType::SUPER_MASK)
        && matches!(
            key,
            gdk::Key::Left | gdk::Key::Right | gdk::Key::Up | gdk::Key::Down
        )
}

#[cfg(test)]
mod tests {
    use super::{
        ActiveTab, AppGroup, InstalledCommand, RefreshChanges, RefreshResult, Shortcut, State,
        command_row_at_or_after, installed, is_super_focus_navigation_key, merge_refresh_result,
        preserved_app_index, preserved_installed_index, preserved_shortcut_index,
        resolve_list_index, scroll_value_for_row, tab_switch_direction,
    };
    use gtk4::gdk;

    fn installed_command(name: &str, tui: bool) -> InstalledCommand {
        InstalledCommand {
            name: name.to_owned(),
            path: format!("/usr/bin/{name}"),
            description: None,
            tui,
        }
    }

    #[test]
    fn installed_selection_skips_the_section_heading() {
        let rows = installed::rows(&[
            installed_command("btop", true),
            installed_command("ls", false),
        ]);

        // Row 0 is the "TUI apps" heading; the first runnable row is `btop`.
        assert!(rows[0].command().is_none());
        assert_eq!(command_row_at_or_after(&rows, 0), Some(1));
        assert_eq!(command_row_at_or_after(&rows, 1), Some(1));
    }

    #[test]
    fn preserves_shortcut_selection_by_value() {
        let selected = Shortcut::new(64, "k", "Launcher", "exec", "launcher");
        let shortcuts = vec![
            Shortcut::new(0, "x", "Other", "exec", "other"),
            selected.clone(),
        ];

        assert_eq!(
            preserved_shortcut_index(&shortcuts, Some(&selected)),
            Some(1)
        );
        assert_eq!(
            preserved_shortcut_index(&shortcuts[..1], Some(&selected)),
            None
        );
    }

    #[test]
    fn preserves_shortcut_selection_when_metadata_changes() {
        let mut selected = Shortcut::new(64, "k", "Launcher", "exec", "launcher");
        let shortcuts = vec![
            Shortcut::new(0, "x", "Other", "exec", "other"),
            selected.clone(),
        ];
        selected.description = "Updated launcher description".to_owned();
        selected.repeat = true;

        assert_eq!(
            preserved_shortcut_index(&shortcuts, Some(&selected)),
            Some(1)
        );
    }

    #[test]
    fn resolves_missing_selection_to_previous_index() {
        assert_eq!(resolve_list_index(None, Some(7), 3), Some(2));
        assert_eq!(resolve_list_index(None, Some(1), 3), Some(1));
        assert_eq!(resolve_list_index(None, None, 3), Some(0));
        assert_eq!(resolve_list_index(None, Some(1), 0), None);
        assert_eq!(resolve_list_index(Some(2), Some(0), 3), Some(2));
    }

    #[test]
    fn preserves_app_selection_by_class() {
        let apps = vec![
            AppGroup {
                class: "first".to_owned(),
                display_name: "First".to_owned(),
                windows: Vec::new(),
                shortcut: None,
            },
            AppGroup {
                class: "selected".to_owned(),
                display_name: "Selected".to_owned(),
                windows: Vec::new(),
                shortcut: None,
            },
        ];

        assert_eq!(preserved_app_index(&apps, Some("selected")), Some(1));
        assert_eq!(preserved_app_index(&apps, Some("missing")), None);
    }

    #[test]
    fn recognizes_super_arrow_focus_changes() {
        let super_modifier = gdk::ModifierType::SUPER_MASK;
        for key in [
            gdk::Key::Left,
            gdk::Key::Right,
            gdk::Key::Up,
            gdk::Key::Down,
        ] {
            assert!(is_super_focus_navigation_key(key, super_modifier));
        }
        assert!(!is_super_focus_navigation_key(gdk::Key::k, super_modifier));
        assert!(!is_super_focus_navigation_key(
            gdk::Key::Left,
            gdk::ModifierType::empty()
        ));
    }

    #[test]
    fn recognizes_plain_tab_switch_shortcuts() {
        assert_eq!(
            tab_switch_direction(gdk::Key::Tab, gdk::ModifierType::empty()),
            Some(true)
        );
        assert_eq!(
            tab_switch_direction(gdk::Key::Tab, gdk::ModifierType::SHIFT_MASK),
            Some(false)
        );
        assert_eq!(
            tab_switch_direction(gdk::Key::ISO_Left_Tab, gdk::ModifierType::empty()),
            Some(false)
        );
        assert_eq!(
            tab_switch_direction(gdk::Key::Tab, gdk::ModifierType::ALT_MASK),
            None
        );
    }

    #[test]
    fn scrolls_only_when_selected_row_is_outside_view() {
        assert_eq!(scroll_value_for_row(0.0, 100.0, 180.0, 40.0), Some(120.0));
        assert_eq!(scroll_value_for_row(180.0, 100.0, 120.0, 40.0), Some(120.0));
        assert_eq!(scroll_value_for_row(0.0, 100.0, 20.0, 40.0), None);
        assert_eq!(scroll_value_for_row(0.0, 0.0, 180.0, 40.0), None);
    }

    #[test]
    fn identical_refresh_does_not_require_list_rebuild() {
        let shortcut = Shortcut::new(64, "k", "Launcher", "exec", "launcher");
        let app = AppGroup {
            class: "kitty".to_owned(),
            display_name: "Kitty".to_owned(),
            windows: Vec::new(),
            shortcut: None,
        };
        let mut state = State {
            shortcuts: vec![shortcut.clone()],
            apps: vec![app.clone()],
            installed: vec![installed_command("ls", false)],
            filtered_shortcuts: vec![shortcut.clone()],
            filtered_apps: vec![app.clone()],
            filtered_installed: vec![installed_command("ls", false)],
            installed_rows: Vec::new(),
            active_tab: ActiveTab::Shortcuts,
            error: None,
            has_refresh_result: true,
        };

        let changes = merge_refresh_result(
            &mut state,
            RefreshResult {
                shortcuts: Ok(vec![shortcut]),
                apps: Ok(vec![app]),
                installed: vec![installed_command("ls", false)],
            },
        );

        assert_eq!(changes, RefreshChanges::default());
    }

    #[test]
    fn refresh_reports_only_the_changed_list() {
        let shortcut = Shortcut::new(64, "k", "Launcher", "exec", "launcher");
        let old_app = AppGroup {
            class: "kitty".to_owned(),
            display_name: "Kitty".to_owned(),
            windows: Vec::new(),
            shortcut: None,
        };
        let new_app = AppGroup {
            class: "code".to_owned(),
            display_name: "Code".to_owned(),
            windows: Vec::new(),
            shortcut: None,
        };
        let mut state = State {
            shortcuts: vec![shortcut.clone()],
            apps: vec![old_app.clone()],
            installed: vec![installed_command("ls", false)],
            filtered_shortcuts: vec![shortcut.clone()],
            filtered_apps: vec![old_app.clone()],
            filtered_installed: vec![installed_command("ls", false)],
            installed_rows: Vec::new(),
            active_tab: ActiveTab::Shortcuts,
            error: None,
            has_refresh_result: true,
        };

        let changes = merge_refresh_result(
            &mut state,
            RefreshResult {
                shortcuts: Ok(vec![shortcut]),
                apps: Ok(vec![new_app]),
                installed: vec![installed_command("ls", false)],
            },
        );

        assert_eq!(
            changes,
            RefreshChanges {
                shortcuts: false,
                apps: true,
                installed: false,
                status: false,
            }
        );
    }

    #[test]
    fn tab_cycle_wraps_through_all_three_tabs() {
        let forward = |tab: ActiveTab| tab.next(true);
        let backward = |tab: ActiveTab| tab.next(false);

        assert_eq!(forward(ActiveTab::Shortcuts), ActiveTab::Apps);
        assert_eq!(forward(ActiveTab::Apps), ActiveTab::Installed);
        assert_eq!(forward(ActiveTab::Installed), ActiveTab::Shortcuts);
        assert_eq!(backward(ActiveTab::Shortcuts), ActiveTab::Installed);
        assert_eq!(backward(ActiveTab::Installed), ActiveTab::Apps);
        assert_eq!(backward(ActiveTab::Apps), ActiveTab::Shortcuts);
    }

    #[test]
    fn tab_index_matches_the_notebook_page() {
        for tab in [ActiveTab::Shortcuts, ActiveTab::Apps, ActiveTab::Installed] {
            assert_eq!(ActiveTab::from_index(tab.index()), tab);
        }
    }

    #[test]
    fn preserves_installed_selection_when_a_heading_is_inserted() {
        let rows = hypr_shortcut_launcher::installed::rows(&[
            installed_command("btop", true),
            installed_command("ls", false),
        ]);
        let selected = rows
            .iter()
            .position(|row| row.command().is_some_and(|command| command.name == "ls"))
            .unwrap();

        assert_eq!(preserved_installed_index(&rows, Some("ls")), Some(selected));
        assert!(rows[0].command().is_none());
        assert_eq!(preserved_installed_index(&rows, Some("missing")), None);
    }

    #[test]
    fn refresh_merge_keeps_previous_data_when_one_query_fails() {
        let original = Shortcut::new(64, "k", "Launcher", "exec", "launcher");
        let replacement = AppGroup {
            class: "replacement".to_owned(),
            display_name: "Replacement".to_owned(),
            windows: Vec::new(),
            shortcut: None,
        };
        let mut state = State {
            shortcuts: vec![original.clone()],
            apps: Vec::new(),
            installed: Vec::new(),
            filtered_shortcuts: vec![original.clone()],
            filtered_apps: Vec::new(),
            filtered_installed: Vec::new(),
            installed_rows: Vec::new(),
            active_tab: ActiveTab::Shortcuts,
            error: None,
            has_refresh_result: false,
        };

        merge_refresh_result(
            &mut state,
            RefreshResult {
                shortcuts: Err("shortcuts unavailable".to_owned()),
                apps: Ok(vec![replacement.clone()]),
                installed: Vec::new(),
            },
        );

        assert_eq!(state.shortcuts, vec![original]);
        assert_eq!(state.apps, vec![replacement]);
        assert_eq!(
            state.error.as_deref(),
            Some("shortcuts: shortcuts unavailable")
        );
    }
}
