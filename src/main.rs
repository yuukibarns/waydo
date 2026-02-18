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
struct Action {
    cmd: &'static str,
    close_on_click: bool,
}

#[derive(Clone, Copy)]
enum ItemKind {
    Action(Action),
    Submenu {
        items: &'static [MenuItem],
        on_click: Option<Action>,
    },
}

#[derive(Clone, Copy)]
struct Color {
    r: f64,
    g: f64,
    b: f64,
    a: f64,
}

const DEFAULT_ITEM_COLOR: Color = Color {
    r: 0.15,
    g: 0.15,
    b: 0.15,
    a: 0.80,
};

const SUBMENU_ITEM_COLOR: Color = Color {
    r: 0.21,
    g: 0.19,
    b: 0.25,
    a: 0.84,
};

#[derive(Clone, Copy)]
struct MenuItem {
    label: &'static str,
    kind: ItemKind,
    color: Color,
}

const CENTER_RADIUS: f64 = 18.0;
const ITEM_RING_DISTANCE: f64 = 86.0;
const ITEM_RADIUS: f64 = 35.0;
const FONT_SIZE: f64 = 13.0;

static APP_MENU: &[MenuItem] = &[
    MenuItem {
        label: "Neovide",
        kind: ItemKind::Action(Action {
            cmd: "spawn -- fish -c ~/.local/bin/neovide-focus",
            close_on_click: true,
        }),
        color: DEFAULT_ITEM_COLOR,
    },
    MenuItem {
        label: "Zen Browser",
        kind: ItemKind::Action(Action {
            cmd: "spawn -- flatpak run app.zen_browser.zen",
            close_on_click: true,
        }),
        color: DEFAULT_ITEM_COLOR,
    },
    MenuItem {
        label: "Files",
        kind: ItemKind::Action(Action {
            cmd: "spawn -- nautilus",
            close_on_click: true,
        }),
        color: DEFAULT_ITEM_COLOR,
    },
    MenuItem {
        label: "Zotero",
        kind: ItemKind::Action(Action {
            cmd: "spawn -- flatpak run org.zotero.Zotero",
            close_on_click: true,
        }),
        color: DEFAULT_ITEM_COLOR,
    },
    MenuItem {
        label: "Btop",
        kind: ItemKind::Action(Action {
            cmd: "spawn -- alacritty --title 'Btop' -e btop",
            close_on_click: true,
        }),
        color: DEFAULT_ITEM_COLOR,
    },
];

static ACTION_MENU: &[MenuItem] = &[
    MenuItem {
        label: "App >",
        kind: ItemKind::Submenu {
            items: APP_MENU,
            on_click: None,
        },
        color: SUBMENU_ITEM_COLOR,
    },
    MenuItem {
        label: "Fullscreen",
        kind: ItemKind::Action(Action {
            cmd: "fullscreen-window",
            close_on_click: false,
        }),
        color: DEFAULT_ITEM_COLOR,
    },
    MenuItem {
        label: "Maximize",
        kind: ItemKind::Action(Action {
            cmd: "maximize-window-to-edges",
            close_on_click: false,
        }),
        color: DEFAULT_ITEM_COLOR,
    },
    MenuItem {
        label: "Toggle Float",
        kind: ItemKind::Action(Action {
            cmd: "toggle-window-floating",
            close_on_click: false,
        }),
        color: DEFAULT_ITEM_COLOR,
    },
    MenuItem {
        label: "Close",
        kind: ItemKind::Action(Action {
            cmd: "close-window",
            close_on_click: true,
        }),
        color: DEFAULT_ITEM_COLOR,
    },
    MenuItem {
        label: "Screenshot",
        kind: ItemKind::Action(Action {
            cmd: "screenshot -p false",
            close_on_click: true,
        }),
        color: DEFAULT_ITEM_COLOR,
    },
];

static MOVEMENT_MENU: &[MenuItem] = &[
    MenuItem {
        label: "Up",
        kind: ItemKind::Action(Action {
            cmd: "move-window-to-workspace-up",
            close_on_click: false,
        }),
        color: DEFAULT_ITEM_COLOR,
    },
    MenuItem {
        label: "Right",
        kind: ItemKind::Action(Action {
            cmd: "swap-window-right",
            close_on_click: false,
        }),
        color: DEFAULT_ITEM_COLOR,
    },
    MenuItem {
        label: "Down",
        kind: ItemKind::Action(Action {
            cmd: "move-window-to-workspace-down",
            close_on_click: false,
        }),
        color: DEFAULT_ITEM_COLOR,
    },
    MenuItem {
        label: "Left",
        kind: ItemKind::Action(Action {
            cmd: "swap-window-left",
            close_on_click: false,
        }),
        color: DEFAULT_ITEM_COLOR,
    },
];

static FOCUS_MENU: &[MenuItem] = &[
    MenuItem {
        label: "Up",
        kind: ItemKind::Action(Action {
            cmd: "focus-workspace-up",
            close_on_click: false,
        }),
        color: DEFAULT_ITEM_COLOR,
    },
    MenuItem {
        label: "Switch",
        kind: ItemKind::Action(Action {
            cmd: "switch-focus-between-floating-and-tiling",
            close_on_click: false,
        }),
        color: DEFAULT_ITEM_COLOR,
    },
    MenuItem {
        label: "Right",
        kind: ItemKind::Action(Action {
            cmd: "focus-column-right",
            close_on_click: false,
        }),
        color: DEFAULT_ITEM_COLOR,
    },
    MenuItem {
        label: "Move >",
        kind: ItemKind::Submenu {
            items: MOVEMENT_MENU,
            on_click: None,
        },
        color: SUBMENU_ITEM_COLOR,
    },
    MenuItem {
        label: "Down",
        kind: ItemKind::Action(Action {
            cmd: "focus-workspace-down",
            close_on_click: false,
        }),
        color: DEFAULT_ITEM_COLOR,
    },
    MenuItem {
        label: "Move >",
        kind: ItemKind::Submenu {
            items: MOVEMENT_MENU,
            on_click: None,
        },
        color: SUBMENU_ITEM_COLOR,
    },
    MenuItem {
        label: "Left",
        kind: ItemKind::Action(Action {
            cmd: "focus-column-left",
            close_on_click: false,
        }),
        color: DEFAULT_ITEM_COLOR,
    },
    MenuItem {
        label: "Switch",
        kind: ItemKind::Action(Action {
            cmd: "switch-focus-between-floating-and-tiling",
            close_on_click: false,
        }),
        color: DEFAULT_ITEM_COLOR,
    },
];

static MISC_MENU: &[MenuItem] = &[
    MenuItem {
        label: "Page Up",
        kind: ItemKind::Action(Action {
            cmd: "key-pageup",
            close_on_click: false,
        }),
        color: DEFAULT_ITEM_COLOR,
    },
    MenuItem {
        label: "Undo",
        kind: ItemKind::Action(Action {
            cmd: "key-ctrl-z",
            close_on_click: false,
        }),
        color: DEFAULT_ITEM_COLOR,
    },
    MenuItem {
        label: "Redo",
        kind: ItemKind::Action(Action {
            cmd: "key-ctrl-shift-z",
            close_on_click: false,
        }),
        color: DEFAULT_ITEM_COLOR,
    },
    MenuItem {
        label: "Delete",
        kind: ItemKind::Action(Action {
            cmd: "key-delete",
            close_on_click: true,
        }),
        color: DEFAULT_ITEM_COLOR,
    },
    MenuItem {
        label: "Page Down",
        kind: ItemKind::Action(Action {
            cmd: "key-pagedown",
            close_on_click: false,
        }),
        color: DEFAULT_ITEM_COLOR,
    },
    MenuItem {
        label: "Copy",
        kind: ItemKind::Action(Action {
            cmd: "key-ctrl-c",
            close_on_click: true,
        }),
        color: DEFAULT_ITEM_COLOR,
    },
    MenuItem {
        label: "Paste",
        kind: ItemKind::Action(Action {
            cmd: "key-ctrl-v",
            close_on_click: true,
        }),
        color: DEFAULT_ITEM_COLOR,
    },
    MenuItem {
        label: "Duplicate",
        kind: ItemKind::Action(Action {
            cmd: "key-ctrl-d",
            close_on_click: true,
        }),
        color: DEFAULT_ITEM_COLOR,
    },
];

static BRUSH_MENU: &[MenuItem] = &[
    MenuItem {
        label: "Blue",
        kind: ItemKind::Action(Action {
            cmd: "key-ctrl-f5",
            close_on_click: true,
        }),
        color: Color {
            r: 0.20,
            g: 0.45,
            b: 0.95,
            a: 0.90,
        },
    },
    MenuItem {
        label: "Green",
        kind: ItemKind::Action(Action {
            cmd: "key-ctrl-f6",
            close_on_click: true,
        }),
        color: Color {
            r: 0.18,
            g: 0.72,
            b: 0.30,
            a: 0.90,
        },
    },
    MenuItem {
        label: "Yellow",
        kind: ItemKind::Action(Action {
            cmd: "key-ctrl-f7",
            close_on_click: true,
        }),
        color: Color {
            r: 0.95,
            g: 0.83,
            b: 0.20,
            a: 0.90,
        },
    },
    MenuItem {
        label: "Orange",
        kind: ItemKind::Action(Action {
            cmd: "key-ctrl-f8",
            close_on_click: true,
        }),
        color: Color {
            r: 0.96,
            g: 0.56,
            b: 0.18,
            a: 0.90,
        },
    },
    MenuItem {
        label: "Red",
        kind: ItemKind::Action(Action {
            cmd: "key-ctrl-f9",
            close_on_click: true,
        }),
        color: Color {
            r: 0.88,
            g: 0.24,
            b: 0.24,
            a: 0.90,
        },
    },
];

static SELECTOR_MENU: &[MenuItem] = &[
    MenuItem {
        label: "Polygon",
        kind: ItemKind::Action(Action {
            cmd: "key-f1",
            close_on_click: true,
        }),
        color: DEFAULT_ITEM_COLOR,
    },
    MenuItem {
        label: "Single",
        kind: ItemKind::Action(Action {
            cmd: "key-f3",
            close_on_click: true,
        }),
        color: DEFAULT_ITEM_COLOR,
    },
    MenuItem {
        label: "Intersecting",
        kind: ItemKind::Action(Action {
            cmd: "key-f4",
            close_on_click: true,
        }),
        color: DEFAULT_ITEM_COLOR,
    },
];

static TOOLS_MENU: &[MenuItem] = &[
    MenuItem {
        label: "Vertical Space",
        kind: ItemKind::Action(Action {
            cmd: "key-f5",
            close_on_click: true,
        }),
        color: DEFAULT_ITEM_COLOR,
    },
    MenuItem {
        label: "Zoom",
        kind: ItemKind::Action(Action {
            cmd: "key-f7",
            close_on_click: true,
        }),
        color: DEFAULT_ITEM_COLOR,
    },
    MenuItem {
        label: "Laser",
        kind: ItemKind::Action(Action {
            cmd: "key-f8",
            close_on_click: true,
        }),
        color: DEFAULT_ITEM_COLOR,
    },
];

static ROOT_MENU: &[MenuItem] = &[
    MenuItem {
        label: "Action >",
        kind: ItemKind::Submenu {
            items: ACTION_MENU,
            on_click: None,
        },
        color: SUBMENU_ITEM_COLOR,
    },
    MenuItem {
        label: "Workspace >",
        kind: ItemKind::Submenu {
            items: FOCUS_MENU,
            on_click: None,
        },
        color: SUBMENU_ITEM_COLOR,
    },
    MenuItem {
        label: "Tools >",
        kind: ItemKind::Submenu {
            items: TOOLS_MENU,
            on_click: Some(Action {
                cmd: "key-ctrl-6 f6",
                close_on_click: false,
            }),
        },
        color: SUBMENU_ITEM_COLOR,
    },
    MenuItem {
        label: "Selector >",
        kind: ItemKind::Submenu {
            items: SELECTOR_MENU,
            on_click: Some(Action {
                cmd: "key-ctrl-5 f2",
                close_on_click: false,
            }),
        },
        color: SUBMENU_ITEM_COLOR,
    },
    MenuItem {
        label: "Brush >",
        kind: ItemKind::Submenu {
            items: BRUSH_MENU,
            on_click: Some(Action {
                cmd: "key-ctrl-1 ctrl-f1",
                close_on_click: false,
            }),
        },
        color: SUBMENU_ITEM_COLOR,
    },
    MenuItem {
        label: "Misc >",
        kind: ItemKind::Submenu {
            items: MISC_MENU,
            on_click: None,
        },
        color: SUBMENU_ITEM_COLOR,
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
}

fn current_items(path: &[usize]) -> &'static [MenuItem] {
    let mut items = ROOT_MENU;
    for &idx in path {
        if idx >= items.len() {
            break;
        }
        match items[idx].kind {
            ItemKind::Submenu { items: sub, .. } => items = sub,
            ItemKind::Action(_) => break,
        }
    }
    items
}

fn dist2(ax: f64, ay: f64, bx: f64, by: f64) -> f64 {
    let dx = ax - bx;
    let dy = ay - by;
    dx * dx + dy * dy
}

fn key_token_to_evdev(tok: &str) -> Option<u16> {
    match tok {
        "ctrl" => Some(29),
        "shift" => Some(42),
        "alt" => Some(56),
        "meta" | "super" => Some(125),
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
        "f1" => Some(59),
        "f2" => Some(60),
        "f3" => Some(61),
        "f4" => Some(62),
        "f5" => Some(63),
        "f6" => Some(64),
        "f7" => Some(65),
        "f8" => Some(66),
        "f9" => Some(67),
        "f10" => Some(68),
        "f11" => Some(87),
        "f12" => Some(88),
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
        "minus" => Some(12),
        "equal" | "plus" => Some(13),
        "delete" | "backspace" => Some(14),
        "pageup" => Some(104),
        "pagedown" => Some(109),
        _ => None,
    }
}

fn run_ydotool_sequence(spec: &str) {
    for combo in spec.split_whitespace() {
        run_ydotool_combo(combo);
        // Small spacing helps tools/apps register successive synthetic keys reliably.
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

fn run_ydotool_combo(spec: &str) {
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
        run_ydotool_sequence(spec);
        return;
    }

    let mut cmd = Command::new("niri");
    cmd.arg("msg").arg("action");
    for part in action.split_whitespace() {
        cmd.arg(part);
    }
    let _ = cmd.status();
}

fn run_action(action: Action, st: &mut State, win: &ApplicationWindow, da: &DrawingArea) {
    if action.close_on_click {
        hide_menu(st, win, da);
    }

    if action.cmd.starts_with("screenshot") {
        let action_owned = action.cmd.to_string();
        glib::timeout_add_local_once(std::time::Duration::from_millis(80), move || {
            run_niri_action(&action_owned);
        });
    } else {
        run_niri_action(action.cmd);
    }
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

    let center_r = CENTER_RADIUS;
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

    let items = current_items(&st.path);
    let n = items.len();
    if n == 0 {
        return;
    }

    let dist = ITEM_RING_DISTANCE;
    let radius = ITEM_RADIUS;

    let points = ring_layout(n, cx, cy, dist);

    for i in 0..n {
        let (bx, by) = points[i];
        let item = items[i];
        cr.set_source_rgba(item.color.r, item.color.g, item.color.b, item.color.a);
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
        cr.set_font_size(FONT_SIZE);

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

fn hide_menu(st: &mut State, win: &ApplicationWindow, _da: &DrawingArea) {
    st.visible = false;
    st.anchored = false;
    st.path.clear();
    win.hide();
}

fn show_menu(st: &mut State, win: &ApplicationWindow, da: &DrawingArea) {
    st.visible = true;
    st.anchored = false;
    st.path.clear();
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

                let center_r = CENTER_RADIUS;
                if dist2(x, y, st.cx, st.cy) <= center_r * center_r {
                    if st.path.is_empty() {
                        hide_menu(&mut st, &win2, &da2);
                    } else {
                        st.path.pop();
                        st.cx = x;
                        st.cy = y;
                        da2.queue_draw();
                    }
                    return;
                }

                let items = current_items(&st.path);
                let n = items.len();
                if n == 0 {
                    return;
                }

                let dist = ITEM_RING_DISTANCE;
                let deadzone = CENTER_RADIUS;
                let points = ring_layout(n, st.cx, st.cy, dist);
                let idx = match closest_index_for_pointer(x, y, st.cx, st.cy, &points, deadzone) {
                    Some(i) if i < n => i,
                    _ => return,
                };

                let radius = ITEM_RADIUS;
                let inner_ring = dist - radius;
                let quick_click = dist2(x, y, st.cx, st.cy) <= inner_ring * inner_ring;

                match items[idx].kind {
                    ItemKind::Action(action) => {
                        run_action(action, &mut st, &win2, &da2);
                    }
                    ItemKind::Submenu { on_click, .. } => {
                        if let Some(mut action) = on_click {
                            if quick_click {
                                action.close_on_click = true;
                                run_action(action, &mut st, &win2, &da2);
                                return;
                            }
                            run_action(action, &mut st, &win2, &da2);
                        }
                        st.path.push(idx);
                        st.cx = x;
                        st.cy = y;
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
                    if reader.read_line(&mut line).is_ok() && line.trim() == "TOGGLE" {
                        let _ = tx.send("TOGGLE".to_string());
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
