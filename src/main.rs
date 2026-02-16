use gtk::gdk;
use gtk::prelude::*;
use gtk::{Application, ApplicationWindow, DrawingArea};

use std::cell::RefCell;
use std::process::Command;
use std::rc::Rc;

use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Choice {
    Up,
    Down,
    Left,
    Right,
}

impl Choice {
    fn label(self) -> &'static str {
        match self {
            Choice::Up => "Workspace Up",
            Choice::Down => "Workspace Down",
            Choice::Left => "Column Left",
            Choice::Right => "Column Right",
        }
    }

    fn niri_action(self) -> &'static str {
        match self {
            Choice::Up => "focus-workspace-up",
            Choice::Down => "focus-workspace-down",
            Choice::Left => "focus-column-left",
            Choice::Right => "focus-column-right",
        }
    }
}

#[derive(Debug, Default)]
struct State {
    // Set once
    cx: f64,
    cy: f64,
    anchored: bool,

    // Latest pointer pos
    px: f64,
    py: f64,

    choice: Option<Choice>,
}

fn compute_choice(dx: f64, dy: f64, deadzone: f64) -> Option<Choice> {
    let r2 = dx * dx + dy * dy;
    if r2 < deadzone * deadzone {
        return None;
    }

    if dx.abs() > dy.abs() {
        if dx > 0.0 {
            Some(Choice::Right)
        } else {
            Some(Choice::Left)
        }
    } else if dy > 0.0 {
        Some(Choice::Down)
    } else {
        Some(Choice::Up)
    }
}

fn run_niri_action(action: &str) {
    let _ = Command::new("niri")
        .args(["msg", "action", action])
        .status();
}

fn dist2(ax: f64, ay: f64, bx: f64, by: f64) -> f64 {
    let dx = ax - bx;
    let dy = ay - by;
    dx * dx + dy * dy
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

fn draw_ui(cr: &gtk::cairo::Context, w: i32, h: i32, st: &State) {
    let w = w as f64;
    let h = h as f64;

    // Fully transparent clear
    cr.set_operator(gtk::cairo::Operator::Source);
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
    let _ = cr.paint();
    cr.set_operator(gtk::cairo::Operator::Over);

    let cx = st.cx;
    let cy = st.cy;

    // Crosshair
    cr.set_line_width(2.0);
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.8);
    cr.move_to(cx - 12.0, cy);
    cr.line_to(cx + 12.0, cy);
    cr.move_to(cx, cy - 12.0);
    cr.line_to(cx, cy + 12.0);
    let _ = cr.stroke();

    // Center close button
    let center_r = 22.0;
    cr.set_source_rgba(0.75, 0.2, 0.2, 0.85);
    cr.arc(cx, cy, center_r, 0.0, std::f64::consts::TAU);
    let _ = cr.fill();

    cr.set_line_width(2.0);
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.7);
    cr.arc(cx, cy, center_r, 0.0, std::f64::consts::TAU);
    let _ = cr.stroke();

    // "X"
    cr.set_line_width(2.5);
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.9);
    cr.move_to(cx - 7.0, cy - 7.0);
    cr.line_to(cx + 7.0, cy + 7.0);
    cr.move_to(cx + 7.0, cy - 7.0);
    cr.line_to(cx - 7.0, cy + 7.0);
    let _ = cr.stroke();

    // Direction buttons
    let dist = 120.0;
    let radius = 46.0;

    let items = [
        (Choice::Up, cx, cy - dist),
        (Choice::Down, cx, cy + dist),
        (Choice::Left, cx - dist, cy),
        (Choice::Right, cx + dist, cy),
    ];

    for (choice, bx, by) in items {
        let selected = st.choice == Some(choice);

        if selected {
            cr.set_source_rgba(0.2, 0.6, 1.0, 0.90);
        } else {
            cr.set_source_rgba(0.15, 0.15, 0.15, 0.75);
        }
        cr.arc(bx, by, radius, 0.0, std::f64::consts::TAU);
        let _ = cr.fill();

        cr.set_line_width(2.0);
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.65);
        cr.arc(bx, by, radius, 0.0, std::f64::consts::TAU);
        let _ = cr.stroke();

        cr.set_source_rgba(1.0, 1.0, 1.0, 0.92);
        cr.select_font_face(
            "Sans",
            gtk::cairo::FontSlant::Normal,
            gtk::cairo::FontWeight::Normal,
        );
        cr.set_font_size(13.0);

        let text = choice.label();
        let ext = cr.text_extents(text).unwrap();
        cr.move_to(
            bx - ext.width() / 2.0 - ext.x_bearing(),
            by + ext.height() / 2.0,
        );
        let _ = cr.show_text(text);
    }

    // Pointer line
    cr.set_line_width(2.0);
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.45);
    cr.move_to(cx, cy);
    cr.line_to(st.px, st.py);
    let _ = cr.stroke();
}

fn main() {
    let app = Application::builder()
        .application_id("waydo")
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

        // Layer-shell fullscreen overlay
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

        // Motion: anchor only once.
        let motion = gtk::EventControllerMotion::new();

        {
            let state = state.clone();
            let da2 = da.clone();
            motion.connect_enter(move |_, x, y| {
                let mut st = state.borrow_mut();
                st.px = x;
                st.py = y;

                if !st.anchored {
                    st.anchored = true;
                    st.cx = x;
                    st.cy = y;
                }

                st.choice = compute_choice(st.px - st.cx, st.py - st.cy, 25.0);
                da2.queue_draw();
            });
        }

        {
            let state = state.clone();
            let da2 = da.clone();
            motion.connect_motion(move |_, x, y| {
                let mut st = state.borrow_mut();
                st.px = x;
                st.py = y;

                if st.anchored {
                    st.choice = compute_choice(st.px - st.cx, st.py - st.cy, 25.0);
                }
                da2.queue_draw();
            });
        }

        da.add_controller(motion);

        // Click:
        // - click center: close
        // - click elsewhere: trigger action, keep open
        let click = gtk::GestureClick::new();
        click.set_button(0);
        {
            let state = state.clone();
            let win2 = win.clone();
            let da2 = da.clone();
            click.connect_released(move |_, _n_press, x, y| {
                let st = state.borrow();

                if !st.anchored {
                    return;
                }

                let center_r = 22.0;
                if dist2(x, y, st.cx, st.cy) <= center_r * center_r {
                    win2.close();
                    return;
                }

                if let Some(c) = st.choice {
                    run_niri_action(c.niri_action());
                }

                da2.queue_draw();
            });
        }
        da.add_controller(click);

        win.present();
        win.grab_focus();
    });

    app.run();
}
