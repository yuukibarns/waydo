use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use gtk::{Application, ApplicationWindow, DrawingArea};

use std::cell::RefCell;
use std::env;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::process::Command;
use std::rc::Rc;
use std::thread;

use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

#[derive(Clone, Copy)]
enum ItemKind {
    Action(&'static str, bool),
    Submenu(&'static [MenuItem]),
}

#[derive(Clone, Copy)]
struct MenuItem {
    label: &'static str,
    kind: ItemKind,
}

// ---------- Submenus ----------

static APP_MENU: &[MenuItem] = &[
    MenuItem {
        label: "Neovide",
        kind: ItemKind::Action("spawn -- fish -c ~/.local/bin/neovide-focus", true),
    },
    MenuItem {
        label: "Zen Browser",
        kind: ItemKind::Action("spawn -- flatpak run app.zen_browser.zen", true),
    },
    MenuItem {
        label: "Files",
        kind: ItemKind::Action("spawn -- nautilus", true),
    },
    MenuItem {
        label: "Zotero",
        kind: ItemKind::Action("spawn -- flatpak run org.zotero.Zotero", true),
    },
    MenuItem {
        label: "Btop",
        kind: ItemKind::Action("spawn -- alacritty --title 'Btop' -e btop", true),
    },
];

static ACTION_MENU: &[MenuItem] = &[
    MenuItem {
        label: "Fullscreen",
        kind: ItemKind::Action("fullscreen-window", false),
    },
    MenuItem {
        label: "Maximize",
        kind: ItemKind::Action("maximize-window-to-edges", false),
    },
    MenuItem {
        label: "Toggle Float",
        kind: ItemKind::Action("toggle-window-floating", false),
    },
    MenuItem {
        label: "Close",
        kind: ItemKind::Action("close-window", false),
    },
    MenuItem {
        label: "Screenshot",
        kind: ItemKind::Action("screenshot -p false", true),
    },
];

static MOVEMENT_MENU: &[MenuItem] = &[
    MenuItem {
        label: "Up",
        kind: ItemKind::Action("move-window-to-workspace-up", false),
    },
    MenuItem {
        label: "Right",
        kind: ItemKind::Action("swap-window-right", false),
    },
    MenuItem {
        label: "Down",
        kind: ItemKind::Action("move-window-to-workspace-down", false),
    },
    MenuItem {
        label: "Left",
        kind: ItemKind::Action("swap-window-left", false),
    },
];

static FOCUS_MENU: &[MenuItem] = &[
    MenuItem {
        label: "Up",
        kind: ItemKind::Action("focus-workspace-up", false),
    },
    MenuItem {
        label: "Switch",
        kind: ItemKind::Action("switch-focus-between-floating-and-tiling", false),
    },
    MenuItem {
        label: "Right",
        kind: ItemKind::Action("focus-column-right", false),
    },
    MenuItem {
        label: "Move >",
        kind: ItemKind::Submenu(MOVEMENT_MENU),
    },
    MenuItem {
        label: "Down",
        kind: ItemKind::Action("focus-workspace-down", false),
    },
    MenuItem {
        label: "Move >",
        kind: ItemKind::Submenu(MOVEMENT_MENU),
    },
    MenuItem {
        label: "Left",
        kind: ItemKind::Action("focus-column-left", false),
    },
    MenuItem {
        label: "Switch",
        kind: ItemKind::Action("switch-focus-between-floating-and-tiling", false),
    },
];

static MISC_MENU: &[MenuItem] = &[
    MenuItem {
        label: "Page Up",
        kind: ItemKind::Action("key-pageup", false),
    },
    MenuItem {
        label: "Undo",
        kind: ItemKind::Action("key-ctrl-z", false),
    },
    MenuItem {
        label: "Redo",
        kind: ItemKind::Action("key-ctrl-shift-z", false),
    },
    MenuItem {
        label: "Zoom Out",
        kind: ItemKind::Action("key-ctrl-minus", false),
    },
    MenuItem {
        label: "Page Down",
        kind: ItemKind::Action("key-pagedown", false),
    },
    MenuItem {
        label: "Zoom In",
        kind: ItemKind::Action("key-ctrl-plus", false),
    },
    MenuItem {
        label: "Delete",
        kind: ItemKind::Action("key-delete", false),
    },
    MenuItem {
        label: "Duplicate",
        kind: ItemKind::Action("key-ctrl-d", false),
    },
];

static ROOT_MENU: &[MenuItem] = &[
    MenuItem {
        label: "Action >",
        kind: ItemKind::Submenu(ACTION_MENU),
    },
    MenuItem {
        label: "Workspace >",
        kind: ItemKind::Submenu(FOCUS_MENU),
    },
    MenuItem {
        label: "Misc >",
        kind: ItemKind::Submenu(MISC_MENU),
    },
    MenuItem {
        label: "Tools",
        kind: ItemKind::Action("key-ctrl-6", true),
    },
    MenuItem {
        label: "Selector",
        kind: ItemKind::Action("key-ctrl-5", true),
    },
    MenuItem {
        label: "Brush",
        kind: ItemKind::Action("key-ctrl-1", true),
    },
    MenuItem {
        label: "App >",
        kind: ItemKind::Submenu(APP_MENU),
    },
];

#[derive(Debug, Default)]
struct State {
    anchored: bool,
    visible: bool,

    // Pointer position
    px: f64,
    py: f64,

    // Current menu center (moves when entering submenu)
    cx: f64,
    cy: f64,

    // Root anchor (for reference)
    root_cx: f64,
    root_cy: f64,

    // Path root -> submenu
    path: Vec<usize>,

    // Stack of centers for back navigation (same depth as path)
    center_stack: Vec<(f64, f64)>,
}

fn current_items(path: &[usize]) -> &'static [MenuItem] {
    let mut items = ROOT_MENU;
    for &idx in path {
        if idx >= items.len() {
            break;
        }
        match items[idx].kind {
            ItemKind::Submenu(sub) => items = sub,
            ItemKind::Action(_, _) => break,
        }
    }
    items
}

fn breadcrumb(path: &[usize]) -> String {
    if path.is_empty() {
        return "Root".to_string();
    }

    let mut items = ROOT_MENU;
    let mut out = vec!["Root".to_string()];

    for &idx in path {
        if idx >= items.len() {
            break;
        }
        out.push(items[idx].label.trim_end_matches(" >").to_string());
        match items[idx].kind {
            ItemKind::Submenu(sub) => items = sub,
            ItemKind::Action(_, _) => break,
        }
    }

    out.join(" > ")
}

fn dist2(ax: f64, ay: f64, bx: f64, by: f64) -> f64 {
    let dx = ax - bx;
    let dy = ay - by;
    dx * dx + dy * dy
}

fn key_token_to_evdev(tok: &str) -> Option<u16> {
    match tok {
        // Modifiers
        "ctrl" => Some(29),            // KEY_LEFTCTRL
        "shift" => Some(42),           // KEY_LEFTSHIFT
        "alt" => Some(56),             // KEY_LEFTALT
        "meta" | "super" => Some(125), // KEY_LEFTMETA

        // Digits
        "1" => Some(2),
        "2" => Some(3),
        "3" => Some(4),
        "4" => Some(5),
        "5" => Some(6),
        "6" => Some(7),
        "7" => Some(8),
        "8" => Some(9),
        "9" => Some(10),
        "0" => Some(11),

        // Letters
        "a" => Some(30),
        "b" => Some(48),
        "c" => Some(46),
        "d" => Some(32),
        "e" => Some(18),
        "f" => Some(33),
        "g" => Some(34),
        "h" => Some(35),
        "i" => Some(23),
        "j" => Some(36),
        "k" => Some(37),
        "l" => Some(38),
        "m" => Some(50),
        "n" => Some(49),
        "o" => Some(24),
        "p" => Some(25),
        "q" => Some(16),
        "r" => Some(19),
        "s" => Some(31),
        "t" => Some(20),
        "u" => Some(22),
        "v" => Some(47),
        "w" => Some(17),
        "x" => Some(45),
        "y" => Some(21),
        "z" => Some(44),

        // Special keys
        "minus" => Some(12),
        "equal" | "plus" => Some(13),
        "delete" | "backspace" => Some(14),
        "pageup" => Some(104),
        "pagedown" => Some(109),

        _ => None,
    }
}

fn run_ydotool_combo(spec: &str) {
    // Supports both "delete" and combos like "ctrl-shift-z".
    let parts: Vec<&str> = spec.split('-').collect();
    if parts.is_empty() {
        return;
    }

    let (mods, main) = parts.split_at(parts.len() - 1);
    let main_code = match key_token_to_evdev(main[0]) {
        Some(c) => c,
        None => return,
    };

    let mut args: Vec<String> = vec!["key".to_string()];
    let mut mod_codes: Vec<u16> = Vec::new();

    for m in mods {
        let code = match key_token_to_evdev(m) {
            Some(c) => c,
            None => return,
        };
        mod_codes.push(code);
        args.push(format!("{code}:1"));
    }

    args.push(format!("{main_code}:1"));
    args.push(format!("{main_code}:0"));

    for code in mod_codes.iter().rev() {
        args.push(format!("{code}:0"));
    }

    let _ = Command::new("ydotool").args(&args).status();
}

fn run_niri_action(action: &str) {
    if let Some(spec) = action.strip_prefix("key-") {
        run_ydotool_combo(spec);
        return;
    }

    let mut cmd = Command::new("niri");
    cmd.arg("msg").arg("action");
    for part in action.split_whitespace() {
        cmd.arg(part);
    }
    let _ = cmd.status();
}

fn install_transparent_css() {
    let css = r#"
    window, .background {
        background-color: transparent;
    }
    "#;

    let provider = gtk::CssProvider::new();
    provider.load_from_data(css);

    gtk::style_context_add_provider_for_display(
        &gdk::Display::default().expect("No display"),
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

fn ring_layout(n: usize, cx: f64, cy: f64, dist: f64) -> Vec<(f64, f64)> {
    if n == 0 {
        return Vec::new();
    }
    let step = std::f64::consts::TAU / n as f64;
    (0..n)
        .map(|i| {
            let a = -std::f64::consts::FRAC_PI_2 + i as f64 * step;
            (cx + dist * a.cos(), cy + dist * a.sin())
        })
        .collect()
}

fn closest_index_for_pointer(
    px: f64,
    py: f64,
    cx: f64,
    cy: f64,
    points: &[(f64, f64)],
    deadzone: f64,
) -> Option<usize> {
    let pointer_r2 = dist2(px, py, cx, cy);
    if pointer_r2 < deadzone * deadzone {
        return None;
    }

    let mut best: Option<(usize, f64)> = None;
    for (i, (x, y)) in points.iter().enumerate() {
        let d = dist2(px, py, *x, *y);
        match best {
            None => best = Some((i, d)),
            Some((_, bd)) if d < bd => best = Some((i, d)),
            _ => {}
        }
    }
    best.map(|(i, _)| i)
}

fn draw_ui(cr: &gtk::cairo::Context, _w: i32, _h: i32, st: &State) {
    if !st.anchored || !st.visible {
        return;
    }

    cr.set_operator(gtk::cairo::Operator::Source);
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
    let _ = cr.paint();
    cr.set_operator(gtk::cairo::Operator::Over);

    let cx = st.cx;
    let cy = st.cy;

    if !st.path.is_empty() {
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.18);
        cr.arc(st.root_cx, st.root_cy, 6.0, 0.0, std::f64::consts::TAU);
        let _ = cr.fill();
    }

    let center_r = 24.0;
    if st.path.is_empty() {
        cr.set_source_rgba(0.75, 0.2, 0.2, 0.88);
    } else {
        cr.set_source_rgba(0.22, 0.48, 0.82, 0.92);
    }
    cr.arc(cx, cy, center_r, 0.0, std::f64::consts::TAU);
    let _ = cr.fill();

    cr.set_line_width(2.0);
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.76);
    cr.arc(cx, cy, center_r, 0.0, std::f64::consts::TAU);
    let _ = cr.stroke();

    cr.set_line_width(2.5);
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.95);
    if st.path.is_empty() {
        cr.move_to(cx - 7.0, cy - 7.0);
        cr.line_to(cx + 7.0, cy + 7.0);
        cr.move_to(cx + 7.0, cy - 7.0);
        cr.line_to(cx - 7.0, cy + 7.0);
    } else {
        cr.move_to(cx + 5.0, cy - 8.0);
        cr.line_to(cx - 5.0, cy);
        cr.line_to(cx + 5.0, cy + 8.0);
    }
    let _ = cr.stroke();

    let bc = breadcrumb(&st.path);
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.90);
    cr.select_font_face(
        "Sans",
        gtk::cairo::FontSlant::Normal,
        gtk::cairo::FontWeight::Normal,
    );
    cr.set_font_size(13.0);
    if let Ok(ext) = cr.text_extents(&bc) {
        cr.move_to(cx - ext.width() / 2.0 - ext.x_bearing(), cy - 42.0);
        let _ = cr.show_text(&bc);
    }

    let items = current_items(&st.path);
    let n = items.len();
    if n == 0 {
        return;
    }

    let dist = if st.path.is_empty() { 120.0 } else { 108.0 };
    let radius = if st.path.is_empty() { 43.0 } else { 44.0 };
    let points = ring_layout(n, cx, cy, dist);

    for i in 0..n {
        let (bx, by) = points[i];

        cr.set_source_rgba(0.15, 0.15, 0.15, 0.80);
        cr.arc(bx, by, radius, 0.0, std::f64::consts::TAU);
        let _ = cr.fill();

        cr.set_line_width(2.0);
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.70);
        cr.arc(bx, by, radius, 0.0, std::f64::consts::TAU);
        let _ = cr.stroke();

        cr.set_source_rgba(1.0, 1.0, 1.0, 0.95);
        cr.select_font_face(
            "Sans",
            gtk::cairo::FontSlant::Normal,
            gtk::cairo::FontWeight::Normal,
        );
        cr.set_font_size(12.5);

        let text = items[i].label;
        if let Ok(ext) = cr.text_extents(text) {
            cr.move_to(
                bx - ext.width() / 2.0 - ext.x_bearing(),
                by + ext.height() / 2.0,
            );
            let _ = cr.show_text(text);
        }
    }
}

fn hide_menu(st: &mut State, win: &ApplicationWindow, da: &DrawingArea) {
    st.visible = false;
    st.anchored = false;
    st.path.clear();
    st.center_stack.clear();
    win.hide();
}

fn show_menu(st: &mut State, win: &ApplicationWindow, da: &DrawingArea) {
    st.visible = true;
    st.anchored = false;
    st.path.clear();
    st.center_stack.clear();
    win.present();
    da.queue_draw();
}

fn send_toggle() -> std::io::Result<()> {
    let mut stream = UnixStream::connect("/tmp/waydo.sock")?;
    stream.write_all(b"TOGGLE\n")?;
    Ok(())
}

fn run_daemon() {
    let app = Application::builder()
        .application_id("io.github.waydo")
        .build();

    app.connect_activate(|app| {
        install_transparent_css();

        let state = Rc::new(RefCell::new(State::default()));

        let win = ApplicationWindow::builder()
            .application(app)
            .title("waydo")
            .decorated(false)
            .resizable(true)
            .build();

        win.init_layer_shell();
        win.set_namespace(Some("waydo"));
        win.set_layer(Layer::Overlay);
        win.set_keyboard_mode(KeyboardMode::None);

        win.set_anchor(Edge::Top, true);
        win.set_anchor(Edge::Bottom, true);
        win.set_anchor(Edge::Left, true);
        win.set_anchor(Edge::Right, true);
        win.set_exclusive_zone(-1);

        let da = DrawingArea::builder().hexpand(true).vexpand(true).build();

        {
            let state = state.clone();
            da.set_draw_func(move |_, cr, w, h| {
                let st = state.borrow();
                draw_ui(cr, w, h, &st);
            });
        }

        win.set_child(Some(&da));
        win.hide();

        let motion = gtk::EventControllerMotion::new();
        {
            let state = state.clone();
            let da2 = da.clone();
            motion.connect_motion(move |_, x, y| {
                let mut st = state.borrow_mut();

                if st.visible && !st.anchored {
                    st.anchored = true;
                    st.px = x;
                    st.py = y;
                    st.cx = x;
                    st.cy = y;
                    st.root_cx = x;
                    st.root_cy = y;
                    da2.queue_draw();
                }
            });
        }
        da.add_controller(motion);

        let click = gtk::GestureClick::new();
        click.set_button(0);

        {
            let state = state.clone();
            let win2 = win.clone();
            let da2 = da.clone();

            click.connect_released(move |_, _n_press, x, y| {
                let mut st = state.borrow_mut();
                if !st.visible {
                    return;
                }

                if !st.anchored {
                    st.anchored = true;
                    st.cx = x;
                    st.cy = y;
                    st.root_cx = x;
                    st.root_cy = y;
                    da2.queue_draw();
                    return;
                }

                let center_r = 24.0;
                if dist2(x, y, st.cx, st.cy) <= center_r * center_r {
                    if st.path.is_empty() {
                        hide_menu(&mut st, &win2, &da2);
                    } else {
                        st.path.pop();
                        if let Some((pcx, pcy)) = st.center_stack.pop() {
                            st.cx = pcx;
                            st.cy = pcy;
                        } else {
                            st.cx = st.root_cx;
                            st.cy = st.root_cy;
                        }
                        da2.queue_draw();
                    }
                    return;
                }

                let items = current_items(&st.path);
                let n = items.len();
                if n == 0 {
                    return;
                }

                let dist = if st.path.is_empty() { 120.0 } else { 108.0 };
                let deadzone = if st.path.is_empty() { 25.0 } else { 30.0 };
                let points = ring_layout(n, st.cx, st.cy, dist);
                let idx = match closest_index_for_pointer(x, y, st.cx, st.cy, &points, deadzone) {
                    Some(i) if i < n => i,
                    _ => return,
                };

                match items[idx].kind {
                    ItemKind::Action(action, close_on_click) => {
                        if close_on_click {
                            hide_menu(&mut st, &win2, &da2);
                        }

                        // HACK
                        if action.starts_with("screenshot") {
                            let action_owned = action.to_string();
                            glib::timeout_add_local_once(
                                std::time::Duration::from_millis(80),
                                move || {
                                    run_niri_action(&action_owned);
                                },
                            );
                        } else {
                            run_niri_action(action);
                        }
                    }
                    ItemKind::Submenu(_) => {
                        let (next_cx, next_cy) = points[idx];
                        let prev_cx = st.cx;
                        let prev_cy = st.cy;
                        st.center_stack.push((prev_cx, prev_cy));
                        st.path.push(idx);
                        st.cx = next_cx;
                        st.cy = next_cy;
                        da2.queue_draw();
                    }
                }
            });
        }

        da.add_controller(click);

        let (tx, rx) = std::sync::mpsc::channel::<String>();

        {
            let state = state.clone();
            let win2 = win.clone();
            let da2 = da.clone();
            glib::timeout_add_local(std::time::Duration::from_millis(16), move || {
                while let Ok(msg) = rx.try_recv() {
                    if msg == "TOGGLE" {
                        let mut st = state.borrow_mut();
                        if st.visible {
                            hide_menu(&mut st, &win2, &da2);
                        } else {
                            show_menu(&mut st, &win2, &da2);
                        }
                    }
                }
                glib::ControlFlow::Continue
            });
        }

        thread::spawn(move || {
            let socket_path = "/tmp/waydo.sock";
            if Path::new(socket_path).exists() {
                let _ = std::fs::remove_file(socket_path);
            }

            let listener = match UnixListener::bind(socket_path) {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("waydo: failed to bind {}: {}", socket_path, e);
                    return;
                }
            };

            for stream in listener.incoming() {
                if let Ok(stream) = stream {
                    let mut reader = BufReader::new(stream);
                    let mut line = String::new();
                    if reader.read_line(&mut line).is_ok() {
                        if line.trim() == "TOGGLE" {
                            let _ = tx.send("TOGGLE".to_string());
                        }
                    }
                }
            }
        });
    });

    app.run_with_args(&["waydo"]);
}

fn main() {
    let arg = env::args().nth(1).unwrap_or_else(|| "toggle".to_string());

    match arg.as_str() {
        "daemon" => run_daemon(),
        "toggle" => {
            if let Err(e) = send_toggle() {
                eprintln!("waydo: toggle failed: {}", e);
                std::process::exit(1);
            }
        }
        _ => {
            eprintln!("usage: waydo [daemon|toggle]");
            std::process::exit(2);
        }
    }
}
