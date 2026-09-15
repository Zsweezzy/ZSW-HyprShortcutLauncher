use hypr_shortcut_launcher::{
    CommandExecutor, HyprlandError, HyprlandService, Shortcut, filter_apps, filter_shortcuts,
    format_modmask, group_windows, is_focus_change_event, parse_bindings_json, parse_clients_json,
};

#[test]
fn parses_hyprland_bindings_and_formats_human_readable_keys() {
    let json = r#"
    [
      {
        "modmask": 65,
        "key": "c",
        "description": "Close window",
        "dispatcher": "killactive",
        "arg": "",
        "locked": true,
        "repeat": true
      },
      {
        "modmask": 64,
        "key": "left",
        "description": "",
        "dispatcher": "movefocus",
        "arg": "l"
      },
      {
        "modmask": 64,
        "key": "K",
        "description": "Open shortcuts and apps launcher",
        "dispatcher": "__lua",
        "arg": "8"
      }
    ]
    "#;

    let shortcuts = parse_bindings_json(json).expect("bindings should parse");

    assert_eq!(shortcuts.len(), 3);
    assert_eq!(format_modmask(shortcuts[0].modmask), "Super + Shift");
    assert_eq!(shortcuts[0].display_key(), "Super + Shift + C");
    assert_eq!(shortcuts[0].action_label(), "Close window");
    assert!(shortcuts[0].locked);
    assert!(shortcuts[0].repeat);
    assert_eq!(shortcuts[1].display_key(), "Super + Left");
    assert_eq!(shortcuts[1].action_label(), "movefocus l");
    assert_eq!(
        shortcuts[2].action_label(),
        "Open shortcuts and apps launcher"
    );

    let undescribed_lua = Shortcut::new(64, "K", "", "__lua", "8");
    assert_eq!(undescribed_lua.action_label(), "Custom Hyprland action");

    let unknown = Shortcut::new(0, "x", "", "", "");
    assert_eq!(unknown.action_label(), "Unknown action");
}

#[test]
fn parses_clients_and_groups_windows_by_class_using_focus_history() {
    let json = r#"
    [
      {
        "address": "0xaaa",
        "class": "firefox",
        "title": "First Firefox window",
        "workspace": {"name": "1"},
        "focusHistoryID": 4,
        "mapped": true,
        "acceptsInput": true
      },
      {
        "address": "0xbbb",
        "class": "firefox",
        "title": "Latest Firefox window",
        "workspace": {"name": "2"},
        "focusHistoryID": 1,
        "mapped": true,
        "acceptsInput": true
      },
      {
        "address": "0xccc",
        "class": "kitty",
        "title": "Terminal",
        "workspace": {"name": "1"},
        "focusHistoryID": 0,
        "mapped": true,
        "acceptsInput": true
      },
      {
        "address": "0xddd",
        "class": "hidden-app",
        "title": "Not visible",
        "workspace": {"name": "special:tray"},
        "focusHistoryID": 2,
        "mapped": false,
        "acceptsInput": true
      }
    ]
    "#;

    let clients = parse_clients_json(json).expect("clients should parse");
    let apps = group_windows(clients);

    assert_eq!(apps.len(), 2);
    assert_eq!(apps[0].class, "kitty");
    let firefox = apps.iter().find(|app| app.class == "firefox").unwrap();
    assert_eq!(firefox.windows.len(), 2);
    assert_eq!(firefox.latest_window().unwrap().address, "0xbbb");
    assert!(!apps.iter().any(|app| app.class == "hidden-app"));
}

#[test]
fn search_filters_shortcuts_and_apps_case_insensitively() {
    let shortcuts = vec![
        Shortcut::new(64, "k", "Open launcher", "exec", "launcher"),
        Shortcut::new(72, "left", "Move window", "movewindow", "left"),
    ];
    let clients = parse_clients_json(
        r#"[{"address":"0x1","class":"firefox","title":"Docs","workspace":{"name":"1"},"focusHistoryID":0,"mapped":true,"acceptsInput":true}]"#,
    )
    .unwrap();
    let apps = group_windows(clients);

    assert_eq!(filter_shortcuts(&shortcuts, "LAUNCH").len(), 1);
    assert_eq!(filter_shortcuts(&shortcuts, "move").len(), 1);
    assert_eq!(filter_apps(&apps, "fire").len(), 1);
    assert!(filter_apps(&apps, "nothing").is_empty());
}

#[derive(Clone)]
struct FakeExecutor {
    responses: Vec<String>,
    calls: std::sync::Arc<std::sync::Mutex<Vec<Vec<String>>>>,
}

impl FakeExecutor {
    fn new(responses: Vec<&str>) -> Self {
        Self {
            responses: responses.into_iter().map(str::to_owned).collect(),
            calls: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }
}

impl CommandExecutor for FakeExecutor {
    fn run(&self, args: &[&str]) -> Result<String, HyprlandError> {
        self.calls
            .lock()
            .expect("calls mutex should not be poisoned")
            .push(args.iter().map(|arg| (*arg).to_owned()).collect());
        self.responses
            .get(
                self.calls
                    .lock()
                    .expect("calls mutex should not be poisoned")
                    .len()
                    - 1,
            )
            .cloned()
            .ok_or_else(|| HyprlandError::CommandFailed("no fake response".into()))
    }
}

#[test]
fn hyprland_service_uses_json_commands_and_focuses_an_address() {
    let executor = FakeExecutor::new(vec![
        r#"[{"modmask":0,"key":"print","dispatcher":"screenshot","arg":""}]"#,
        r#"[{"address":"0xabc","class":"kitty","title":"Shell","workspace":{"name":"1"},"focusHistoryID":0,"mapped":true,"acceptsInput":true}]"#,
        "ok",
    ]);
    let service = HyprlandService::new(executor.clone());

    assert_eq!(service.bindings().unwrap().len(), 1);
    assert_eq!(service.clients().unwrap()[0].class, "kitty");
    service.focus_window("0xabc").unwrap();

    let calls = executor.calls.lock().unwrap();
    assert_eq!(calls[0], vec!["binds", "-j"]);
    assert_eq!(calls[1], vec!["clients", "-j"]);
    assert_eq!(
        calls[2],
        vec!["dispatch", "hl.dsp.focus({ window = \"address:0xabc\" })"]
    );
}

#[derive(Clone, Default)]
struct FailingLuaExecutor {
    calls: std::sync::Arc<std::sync::Mutex<Vec<Vec<String>>>>,
}

impl CommandExecutor for FailingLuaExecutor {
    fn run(&self, args: &[&str]) -> Result<String, HyprlandError> {
        let mut calls = self.calls.lock().unwrap();
        calls.push(args.iter().map(|arg| (*arg).to_owned()).collect());
        if calls.len() == 1 {
            Err(HyprlandError::CommandFailed(
                "Lua dispatcher unavailable".into(),
            ))
        } else {
            Ok("ok".into())
        }
    }
}

#[test]
fn focus_window_falls_back_to_native_dispatch_when_lua_is_unavailable() {
    let executor = FailingLuaExecutor::default();
    let service = HyprlandService::new(executor.clone());

    service.focus_window("0xabc").unwrap();

    let calls = executor.calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[1], vec!["dispatch", "focuswindow", "address:0xabc"]);
}

#[test]
fn invalid_focus_addresses_are_rejected_before_running_a_command() {
    let executor = FakeExecutor::new(Vec::new());
    let service = HyprlandService::new(executor.clone());

    let error = service.focus_window("not-an-address").unwrap_err();
    assert!(matches!(error, HyprlandError::InvalidAddress(_)));
    assert!(executor.calls.lock().unwrap().is_empty());
}

#[test]
fn recognizes_hyprland_events_that_change_focus() {
    for event in [
        "activewindow>>kitty,Shell",
        "activewindowv2>>55ed19e6b5a0",
        "focusedmon>>DP-1,2",
        "focusedmonv2>>DP-1,2",
        "workspace>>2",
        "workspacev2>>2",
    ] {
        assert!(
            is_focus_change_event(event),
            "event should close launcher: {event}"
        );
    }

    for event in ["openwindow>>0xabc,kitty", "closewindow>>0xabc", "submap>>1"] {
        assert!(
            !is_focus_change_event(event),
            "unrelated event should not close launcher: {event}"
        );
    }
}
