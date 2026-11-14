use crate::{
    memory::{self, Process, Snapshot, TrimResult},
    optimization::{self, Drive, PowerPolicy},
    preferences::{self, Preferences},
    sort::{Column, ProcessSort},
    startup::{self, StartupEntry, StartupState},
    window,
};
use eframe::egui::{
    self, Align, Align2, Color32, FontData, FontFamily, FontId, Layout, Pos2, Rect, RichText,
    Sense, Stroke, Vec2,
};
use std::{
    collections::{HashSet, VecDeque},
    sync::mpsc::{self, Receiver, Sender},
    time::{Duration, Instant},
};

const BG: Color32 = Color32::from_rgb(17, 24, 32);
const NAV: Color32 = Color32::from_rgb(12, 18, 25);
const PANEL: Color32 = Color32::from_rgb(29, 41, 53);
const INK: Color32 = Color32::from_rgb(237, 242, 247);
const MUTED: Color32 = Color32::from_rgb(167, 184, 201);
const LINE: Color32 = Color32::from_rgb(49, 64, 79);
const ACCENT: Color32 = Color32::from_rgb(125, 224, 209);
const WARN: Color32 = Color32::from_rgb(240, 193, 122);
const ERROR: Color32 = Color32::from_rgb(242, 155, 155);

#[derive(PartialEq, Clone, Copy)]
enum Page {
    Memory,
    Startup,
    Optimization,
}

impl Page {
    fn title(self) -> &'static str {
        match self {
            Self::Memory => "메모리",
            Self::Startup => "시작 프로그램",
            Self::Optimization => "최적화",
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum StartupFilter {
    All,
    Enabled,
    Disabled,
    Unknown,
}
impl StartupFilter {
    fn matches(self, state: StartupState) -> bool {
        match self {
            Self::All => true,
            Self::Enabled => state == StartupState::Enabled,
            Self::Disabled => matches!(
                state,
                StartupState::Disabled | StartupState::WindowsDisabled
            ),
            Self::Unknown => state == StartupState::Unknown,
        }
    }
}
enum Command {
    Refresh,
    Trim(Vec<Process>),
    Startup,
    Toggle(StartupEntry),
    ReadPower(Process),
    SetPower(Process, PowerPolicy, PowerPolicy),
    Storage,
}
enum Event {
    Memory(Result<Snapshot, String>),
    Trim(TrimResult),
    Startup(Result<Vec<StartupEntry>, String>),
    Changed(Result<String, String>),
    Power(Result<PowerPolicy, String>),
    Storage(Result<Vec<Drive>, String>),
}

pub struct App {
    preferences: Preferences,
    saved_preferences: Preferences,
    pending_position: Option<[i32; 2]>,
    position_changed_at: Option<Instant>,
    window: Option<windows_sys::Win32::Foundation::HWND>,
    drag_region: Option<window::DragRegion>,
    trim_finished_at: Option<Instant>,
    launch_at_login: bool,
    widget: bool,
    pinned: bool,
    last_trim: Option<(usize, u64, usize)>,
    page: Page,
    tx: Sender<Command>,
    rx: Receiver<Event>,
    snapshot: Option<Snapshot>,
    history: VecDeque<(Instant, f32)>,
    selected: HashSet<(u32, u64)>,
    entries: Vec<StartupEntry>,
    startup_loaded: bool,
    search: String,
    process_sort: ProcessSort,
    widget_sort: ProcessSort,
    startup_filter: StartupFilter,
    busy: bool,
    startup_busy: bool,
    power_target: Option<(u32, u64)>,
    power: Option<Result<PowerPolicy, String>>,
    power_busy: bool,
    storage: Option<Result<Vec<Drive>, String>>,
    storage_busy: bool,
    notice: Option<(String, bool)>,
    failures: Vec<String>,
    details: Option<String>,
    screenshot: Option<String>,
    capture_sent: bool,
    started: Instant,
}

pub fn starts_as_widget() -> bool {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .any(|a| matches!(a.as_str(), "--widget" | "--compact" | "--expanded"))
        || !args.iter().any(|a| {
            matches!(
                a.as_str(),
                "--full" | "--startup" | "--screenshot" | "--small-window"
            )
        })
}

fn bytes(n: u64) -> String {
    if n >= 1024 * 1024 * 1024 {
        format!("{:.1} GB", n as f64 / (1024.0 * 1024.0 * 1024.0))
    } else {
        format!("{:.0} MB", n as f64 / (1024.0 * 1024.0))
    }
}

fn cpu_usage(value: Option<f32>) -> String {
    value.map_or_else(|| "—".into(), |percent| format!("{percent:.1}%"))
}

const CPU_HELP: &str = "최근 측정 구간의 CPU 사용률 · 전체 CPU 용량 기준\n첫 측정이나 조회할 수 없는 경우 —로 표시합니다.";

fn button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(egui::Button::new(text).min_size(Vec2::new(0.0, 32.0)))
}

fn cell(
    ui: &mut egui::Ui,
    width: f32,
    height: f32,
    text: RichText,
    align: Align,
) -> egui::Response {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let layout = if align == Align::RIGHT {
        Layout::right_to_left(Align::Center)
    } else {
        Layout::left_to_right(Align::Center)
    };
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(layout));
    child.add(egui::Label::new(text).truncate())
}

fn sort_header(
    ui: &mut egui::Ui,
    sort: &mut ProcessSort,
    column: Column,
    label: &str,
    width: f32,
    align: Align,
) {
    let active = sort.column == column;
    let response = ui.add_sized([width, 30.0], egui::Button::new("").frame(false));
    let rect = response.rect;
    if active || response.hovered() || response.has_focus() {
        ui.painter().rect_filled(rect, 6, PANEL);
    }
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect,
            6,
            Stroke::new(1.0_f32, ACCENT),
            egui::StrokeKind::Inside,
        );
    }
    let color = if active { ACCENT } else { MUTED };
    let text_pos = Pos2::new(
        if align == Align::RIGHT {
            rect.right() - 22.0
        } else {
            rect.left() + 8.0
        },
        rect.center().y,
    );
    ui.painter().text(
        text_pos,
        if align == Align::RIGHT {
            Align2::RIGHT_CENTER
        } else {
            Align2::LEFT_CENTER
        },
        label,
        FontId::proportional(12.0),
        color,
    );
    let c = Pos2::new(rect.right() - 10.0, rect.center().y);
    let triangle = |y: f32, down: bool, color| {
        let sign = if down { 1.0 } else { -1.0 };
        ui.painter().add(egui::Shape::convex_polygon(
            vec![
                Pos2::new(c.x - 3.0, y - sign * 2.0),
                Pos2::new(c.x + 3.0, y - sign * 2.0),
                Pos2::new(c.x, y + sign * 2.0),
            ],
            color,
            Stroke::NONE,
        ));
    };
    if active {
        triangle(c.y, sort.descending, color);
    } else {
        triangle(c.y - 3.0, false, LINE);
        triangle(c.y + 3.0, true, LINE);
    }
    let direction = if active {
        if sort.descending {
            "내림차순"
        } else {
            "오름차순"
        }
    } else {
        "정렬 안 함"
    };
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            response.enabled(),
            format!("{label} · {direction} · 눌러 정렬"),
        )
    });
    let clicked = response.clicked();
    response.on_hover_text(format!(
        "{label} 기준으로 정렬합니다. 다시 누르면 방향이 바뀝니다."
    ));
    if clicked {
        sort.select(column);
    }
}

fn startup_badge(ui: &mut egui::Ui, state: StartupState) {
    let (label, color) = match state {
        StartupState::Enabled => ("켜짐", ACCENT),
        StartupState::Disabled => ("꺼짐 · 보관됨", MUTED),
        StartupState::WindowsDisabled => ("꺼짐 · Windows", MUTED),
        StartupState::Unknown => ("확인 필요", WARN),
    };
    let (cell, response) = ui.allocate_exact_size(Vec2::new(130.0, 32.0), Sense::hover());
    let galley = ui
        .painter()
        .layout_no_wrap(label.into(), FontId::proportional(11.0), color);
    let badge = Rect::from_min_size(
        Pos2::new(cell.left(), cell.center().y - 12.0),
        Vec2::new(galley.size().x + 26.0, 24.0),
    );
    ui.painter()
        .rect_filled(badge, 12, color.gamma_multiply(0.12));
    ui.painter()
        .circle_filled(Pos2::new(badge.left() + 9.0, badge.center().y), 2.5, color);
    ui.painter().galley(
        Pos2::new(
            badge.left() + 16.0,
            badge.center().y - galley.size().y / 2.0,
        ),
        galley,
        color,
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Label, true, format!("자동 실행 {label}"))
    });
}

fn startup_switch(ui: &mut egui::Ui, on: bool, label: &str) -> egui::Response {
    let response = ui.add(
        egui::Button::new("")
            .fill(if on { ACCENT } else { PANEL })
            .stroke(Stroke::new(1.0_f32, if on { ACCENT } else { MUTED }))
            .corner_radius(12)
            .min_size(Vec2::new(44.0, 24.0)),
    );
    let r = response.rect;
    let thumb = if on { NAV } else { MUTED };
    ui.painter().circle_filled(
        Pos2::new(
            if on {
                r.right() - 12.0
            } else {
                r.left() + 12.0
            },
            r.center().y,
        ),
        8.0,
        if response.enabled() {
            thumb
        } else {
            thumb.gamma_multiply(0.45)
        },
    );
    if response.has_focus() {
        ui.painter().rect_stroke(
            r.expand(3.0),
            15,
            Stroke::new(1.0_f32, ACCENT),
            egui::StrokeKind::Inside,
        );
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, response.enabled(), on, label)
    });
    response
}

#[derive(Clone, Copy)]
enum Icon {
    Pin,
    Close,
    Collapse,
    Expand,
    Optimize,
    Check,
}

fn icon_button(ui: &mut egui::Ui, icon: Icon, selected: bool, label: &str) -> egui::Response {
    let primary = matches!(icon, Icon::Optimize | Icon::Check);
    let response = ui.add(
        egui::Button::new("")
            .fill(if primary {
                ACCENT
            } else if selected {
                PANEL
            } else {
                BG
            })
            .stroke(Stroke::NONE)
            .corner_radius(if primary { 11 } else { 8 })
            .min_size(Vec2::splat(if primary || matches!(icon, Icon::Expand) {
                32.0
            } else {
                28.0
            })),
    );
    response.widget_info(|| {
        if matches!(icon, Icon::Pin) {
            egui::WidgetInfo::selected(
                egui::WidgetType::Checkbox,
                response.enabled(),
                selected,
                label,
            )
        } else {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, response.enabled(), label)
        }
    });
    let c = response.rect.center();
    let stroke = Stroke::new(
        1.5_f32,
        if primary {
            NAV
        } else if selected || response.hovered() || response.has_focus() {
            ACCENT
        } else {
            MUTED
        },
    );
    let line = |a: [f32; 2], b: [f32; 2]| {
        ui.painter()
            .line_segment([c + Vec2::from(a), c + Vec2::from(b)], stroke);
    };
    match icon {
        Icon::Close => {
            line([-4.0, -4.0], [4.0, 4.0]);
            line([-4.0, 4.0], [4.0, -4.0]);
        }
        Icon::Pin => {
            ui.painter().rect_stroke(
                Rect::from_center_size(c, Vec2::new(12.0, 11.0)),
                2,
                stroke,
                egui::StrokeKind::Inside,
            );
            line([-5.0, -2.0], [5.0, -2.0]);
        }
        Icon::Collapse => {
            line([-5.0, 0.0], [5.0, 0.0]);
        }
        Icon::Expand => {
            line([-5.0, -1.0], [-5.0, -5.0]);
            line([-5.0, -5.0], [-1.0, -5.0]);
            line([5.0, 1.0], [5.0, 5.0]);
            line([5.0, 5.0], [1.0, 5.0]);
        }
        Icon::Optimize => {
            line([-1.0, -2.0], [5.0, -8.0]);
            ui.painter().add(egui::Shape::closed_line(
                vec![
                    c + Vec2::new(-6.0, 0.0),
                    c + Vec2::new(-2.0, -4.0),
                    c + Vec2::new(3.0, 1.0),
                    c + Vec2::new(-1.0, 5.0),
                ],
                stroke,
            ));
            line([-4.0, 2.0], [-2.0, 0.0]);
            line([-2.0, 4.0], [0.0, 2.0]);
        }
        Icon::Check => {
            line([-5.0, 0.0], [-1.0, 4.0]);
            line([-1.0, 4.0], [6.0, -4.0]);
        }
    }
    response.on_hover_text(label)
}

pub fn widget_size(compact: bool) -> Vec2 {
    if compact {
        Vec2::new(248.0, 82.0)
    } else {
        Vec2::new(380.0, 608.0)
    }
}

fn centered_position(left: i32, top: i32, right: i32, bottom: i32, scale: f32, size: Vec2) -> Pos2 {
    Pos2::new(
        left as f32 / scale + (((right - left) as f32 / scale - size.x) / 2.0).max(0.0),
        top as f32 / scale + (((bottom - top) as f32 / scale - size.y) / 2.0).max(0.0),
    )
}

fn management_position(ctx: &egui::Context) -> Option<Pos2> {
    use windows_sys::Win32::{
        Graphics::Gdi::{
            GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
        },
        UI::WindowsAndMessaging::GetForegroundWindow,
    };
    unsafe {
        let monitor = MonitorFromWindow(GetForegroundWindow(), MONITOR_DEFAULTTONEAREST);
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if monitor.is_null() || GetMonitorInfoW(monitor, &mut info) == 0 {
            return None;
        }
        let work = info.rcWork;
        Some(centered_position(
            work.left,
            work.top,
            work.right,
            work.bottom,
            ctx.pixels_per_point(),
            Vec2::new(1120.0, 790.0),
        ))
    }
}

fn open_windows_settings(uri: &str) -> bool {
    use windows_sys::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL};
    let uri: Vec<u16> = uri.encode_utf16().chain(Some(0)).collect();
    let action: Vec<u16> = "open\0".encode_utf16().collect();
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            action.as_ptr(),
            uri.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    result as usize > 32
}

impl App {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        preferences: Preferences,
        load_error: Option<String>,
    ) -> Self {
        let ctx = &cc.egui_ctx;
        let mut fonts = egui::FontDefinitions::default();
        let font_path = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into())
            + "\\Fonts\\malgun.ttf";
        if let Ok(data) = std::fs::read(font_path) {
            fonts
                .font_data
                .insert("korean".into(), FontData::from_owned(data).into());
            fonts
                .families
                .get_mut(&FontFamily::Proportional)
                .unwrap()
                .insert(0, "korean".into());
        }
        ctx.set_fonts(fonts);
        let mut style = (*ctx.style()).clone();
        style.visuals = egui::Visuals::dark();
        style.visuals.panel_fill = BG;
        style.visuals.window_fill = BG;
        style.visuals.extreme_bg_color = NAV;
        style.visuals.faint_bg_color = PANEL;
        style.visuals.override_text_color = Some(INK);
        style.visuals.selection.bg_fill = ACCENT.gamma_multiply(0.25);
        style.visuals.selection.stroke = Stroke::new(1.0_f32, ACCENT);
        style.visuals.widgets.inactive.bg_fill = PANEL;
        style.visuals.widgets.inactive.weak_bg_fill = PANEL;
        style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, LINE);
        style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(43, 61, 76);
        style.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(43, 61, 76);
        style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, ACCENT);
        style.visuals.widgets.active.bg_stroke = Stroke::new(1.5_f32, ACCENT);
        style.spacing.item_spacing = Vec2::new(10.0, 8.0);
        style.spacing.button_padding = Vec2::new(14.0, 7.0);
        style.spacing.interact_size.y = 28.0;
        style
            .text_styles
            .insert(egui::TextStyle::Body, FontId::proportional(14.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, FontId::proportional(13.0));
        style
            .text_styles
            .insert(egui::TextStyle::Small, FontId::proportional(12.0));
        style
            .text_styles
            .insert(egui::TextStyle::Heading, FontId::proportional(24.0));
        ctx.set_style(style);
        let (tx, commands) = mpsc::channel();
        let (events, rx) = mpsc::channel();
        let repaint = ctx.clone();
        std::thread::Builder::new()
            .name("memori-worker".into())
            .spawn(move || {
                let mut sampler = memory::Sampler::default();
                let send = |e| {
                    let _ = events.send(e);
                    repaint.request_repaint();
                };
                send(Event::Memory(sampler.sample()));
                loop {
                    match commands.recv_timeout(Duration::from_secs(2)) {
                        Ok(Command::Refresh) | Err(mpsc::RecvTimeoutError::Timeout) => {
                            send(Event::Memory(sampler.sample()))
                        }
                        Ok(Command::Trim(processes)) => {
                            send(Event::Trim(memory::trim(&processes)));
                            send(Event::Memory(sampler.sample()));
                        }
                        Ok(Command::Startup) => send(Event::Startup(startup::scan())),
                        Ok(Command::Toggle(entry)) => {
                            send(Event::Changed(startup::toggle(&entry)));
                            send(Event::Startup(startup::scan()));
                        }
                        Ok(Command::ReadPower(process)) => {
                            send(Event::Power(optimization::power_policy(&process)));
                        }
                        Ok(Command::SetPower(process, expected, desired)) => {
                            send(Event::Power(optimization::set_power_policy(
                                &process, expected, desired,
                            )));
                        }
                        Ok(Command::Storage) => send(Event::Storage(optimization::storage())),
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                }
            })
            .expect("메모리 조회 스레드를 시작할 수 없습니다");
        let args: Vec<String> = std::env::args().collect();
        let screenshot = args
            .windows(2)
            .find(|a| a[0] == "--screenshot")
            .map(|a| a[1].clone());
        let page = if args.iter().any(|a| a == "--startup") {
            Page::Startup
        } else {
            Page::Memory
        };
        if page == Page::Startup {
            let _ = tx.send(Command::Startup);
        }
        let login = preferences::launch_at_login();
        let error = load_error.or_else(|| login.as_ref().err().cloned());
        let pending_position = if screenshot.is_none() && starts_as_widget() {
            preferences.position
        } else {
            None
        };
        Self {
            pinned: preferences.pinned,
            saved_preferences: preferences.clone(),
            preferences,
            pending_position,
            position_changed_at: None,
            window: window::handle(cc),
            drag_region: window::DragRegion::install(window::handle(cc)),
            trim_finished_at: None,
            launch_at_login: login.unwrap_or(false),
            widget: starts_as_widget(),
            last_trim: None,
            page,
            tx,
            rx,
            snapshot: None,
            history: VecDeque::new(),
            selected: HashSet::new(),
            entries: Vec::new(),
            startup_loaded: false,
            search: String::new(),
            process_sort: ProcessSort::default(),
            widget_sort: ProcessSort::default(),
            startup_filter: StartupFilter::All,
            busy: false,
            startup_busy: page == Page::Startup,
            power_target: None,
            power: None,
            power_busy: false,
            storage: None,
            storage_busy: false,
            notice: error.map(|e| (e, true)),
            failures: Vec::new(),
            details: None,
            screenshot,
            capture_sent: false,
            started: Instant::now(),
        }
    }

    fn receive(&mut self) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Event::Memory(Ok(s)) => {
                    let now = Instant::now();
                    self.history.push_back((now, s.percent()));
                    while self
                        .history
                        .front()
                        .is_some_and(|(t, _)| now.duration_since(*t).as_secs() > 120)
                    {
                        self.history.pop_front();
                    }
                    self.selected.retain(|id| {
                        s.processes
                            .iter()
                            .any(|p| (p.pid, p.created) == *id && p.eligible)
                    });
                    self.snapshot = Some(s);
                }
                Event::Memory(Err(e)) => self.notice = Some((e, true)),
                Event::Trim(result) => {
                    self.trim_finished_at = Some(Instant::now());
                    self.last_trim = Some((result.count, result.reclaimed, result.errors.len()));
                    self.busy = false;
                    self.selected.clear();
                    self.notice = Some((
                        format!(
                            "{}개 앱 · 작업 집합 {} 감소. 앱을 사용하면 다시 늘어날 수 있습니다.{}",
                            result.count,
                            bytes(result.reclaimed),
                            if result.errors.is_empty() {
                                ""
                            } else {
                                " 일부 앱은 정리하지 못했습니다."
                            }
                        ),
                        result.count == 0,
                    ));
                    self.failures = result.errors;
                }
                Event::Startup(result) => {
                    self.startup_busy = false;
                    match result {
                        Ok(entries) => {
                            self.entries = entries;
                            self.startup_loaded = true;
                            if let Ok(enabled) = preferences::launch_at_login() {
                                self.launch_at_login = enabled;
                            }
                        }
                        Err(e) => self.notice = Some((e, true)),
                    }
                }
                Event::Changed(result) => {
                    self.notice = Some(match result {
                        Ok(s) => (s, false),
                        Err(e) => (e, true),
                    });
                }
                Event::Power(result) => {
                    self.power_busy = false;
                    self.power = Some(result);
                }
                Event::Storage(result) => {
                    self.storage_busy = false;
                    self.storage = Some(result);
                }
            }
        }
    }

    fn send(&mut self, command: Command) {
        if self.tx.send(command).is_err() {
            self.notice = Some((
                "백그라운드 작업이 중단되었습니다. 앱을 다시 열어 주세요.".into(),
                true,
            ));
            self.busy = false;
            self.startup_busy = false;
            self.power_busy = false;
            self.storage_busy = false;
        }
    }

    fn settings(&mut self) {
        self.open_settings(
            "ms-settings:startupapps",
            "설정 > 앱 > 시작 프로그램을 열어 주세요.",
        );
    }

    fn open_settings(&mut self, uri: &str, fallback: &str) {
        if !open_windows_settings(uri) {
            self.failures.clear();
            self.notice = Some((format!("Windows 설정을 열지 못했습니다. {fallback}"), true));
        }
    }

    fn navigate(&mut self, page: Page) {
        self.page = page;
        self.search.clear();
        self.details = None;
        match page {
            Page::Startup if !self.startup_loaded && !self.startup_busy => {
                self.startup_busy = true;
                self.send(Command::Startup);
            }
            Page::Optimization if self.storage.is_none() && !self.storage_busy => {
                self.storage_busy = true;
                self.send(Command::Storage);
            }
            _ => {}
        }
    }

    fn persist_preferences(&mut self) -> bool {
        if self.screenshot.is_some() || self.preferences == self.saved_preferences {
            return true;
        }
        self.position_changed_at = None;
        match preferences::save(&self.preferences) {
            Ok(()) => {
                self.saved_preferences = self.preferences.clone();
                true
            }
            Err(error) => {
                self.last_trim = None;
                self.notice = Some((error, true));
                false
            }
        }
    }

    fn observe_widget_position(&mut self, ctx: &egui::Context) {
        if !self.widget || self.screenshot.is_some() {
            return;
        }
        let position = if let Some(position) = self.pending_position.take() {
            match window::restore(self.window, position) {
                Ok(position) => Some(position),
                Err(error) => {
                    self.last_trim = None;
                    self.notice = Some((error, true));
                    None
                }
            }
        } else {
            window::position(self.window)
        };
        if let Some(position) = position
            && self.preferences.position != Some(position)
        {
            self.preferences.position = Some(position);
            self.position_changed_at = Some(Instant::now());
        }
        if let Some(changed) = self.position_changed_at {
            if changed.elapsed() >= Duration::from_millis(400) {
                self.persist_preferences();
            } else {
                ctx.request_repaint_after(Duration::from_millis(450));
            }
        }
    }

    fn set_widget(&mut self, ctx: &egui::Context, widget: bool) {
        if self.widget && !widget {
            if let Some(position) = window::position(self.window) {
                self.preferences.position = Some(position);
            }
            self.persist_preferences();
        }
        if let Some(region) = &self.drag_region {
            region.set(None, ctx.pixels_per_point());
        }
        self.widget = widget;
        if widget {
            self.pending_position = self.preferences.position;
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(false));
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(!widget));
        ctx.send_viewport_cmd(egui::ViewportCommand::Resizable(!widget));
        ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(if widget {
            widget_size(self.preferences.compact)
        } else {
            Vec2::new(880.0, 650.0)
        }));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(if widget {
            widget_size(self.preferences.compact)
        } else {
            Vec2::new(1120.0, 790.0)
        }));
        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
            if widget && self.pinned {
                egui::WindowLevel::AlwaysOnTop
            } else {
                egui::WindowLevel::Normal
            },
        ));
        if !widget {
            if let Some(position) = management_position(ctx) {
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(position));
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
            }
        }
    }

    fn set_compact(&mut self, ctx: &egui::Context, compact: bool) {
        let previous = self.preferences.compact;
        self.preferences.compact = compact;
        if self.persist_preferences() {
            self.set_widget(ctx, true);
        } else {
            self.preferences.compact = previous;
        }
    }

    fn widget_drag(
        &self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        header: Rect,
        inset: f32,
    ) -> egui::Response {
        let caption = window::caption_rect(ui.max_rect().min, header, inset);
        let response = ui.interact(caption, ui.id().with("widget_caption"), Sense::drag());
        if let Some(region) = &self.drag_region {
            region.set(Some(caption), ctx.pixels_per_point());
            response
        } else {
            if response.is_pointer_button_down_on() && ui.input(|i| i.pointer.primary_down()) {
                ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
            response.on_hover_cursor(egui::CursorIcon::Grab)
        }
    }

    fn compact_widget(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(BG)
                    .stroke(Stroke::new(1.0_f32, LINE))
                    .corner_radius(24)
                    .inner_margin(12)
                    .outer_margin(8),
            )
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing = Vec2::new(8.0, 0.0);
                ui.style_mut().interaction.selectable_labels = false;
                ui.horizontal(|ui| {
                    let (header, _) = ui.allocate_exact_size(
                        Vec2::new(ui.available_width() - 80.0, 40.0),
                        Sense::hover(),
                    );
                    let drag = self.widget_drag(ui, ctx, header, 12.0);
                    let stale = self.snapshot.as_ref().is_none_or(|s| {
                        s.at.elapsed()
                            .map_or(true, |age| age > Duration::from_secs(10))
                    });
                    let failed = self.notice.as_ref().is_some_and(|(_, error)| *error);
                    let percent = self
                        .snapshot
                        .as_ref()
                        .map_or("…".into(), |s| format!("{:.0}%", s.percent() * 100.0));
                    ui.painter().text(
                        Pos2::new(header.left(), header.center().y),
                        Align2::LEFT_CENTER,
                        "RAM",
                        FontId::proportional(11.0),
                        MUTED,
                    );
                    ui.painter().text(
                        Pos2::new(header.left() + 34.0, header.center().y),
                        Align2::LEFT_CENTER,
                        &percent,
                        FontId::proportional(22.0),
                        if failed {
                            ERROR
                        } else if stale {
                            WARN
                        } else {
                            INK
                        },
                    );
                    drag.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::WidgetType::Other,
                            true,
                            format!("RAM {percent}, 잡아서 위젯 이동"),
                        )
                    });
                    let tooltip = if let Some((notice, _)) = &self.notice {
                        notice.clone()
                    } else if let Some(s) = &self.snapshot {
                        format!(
                            "사용 {} / {} · 여유 {}\n잡아서 위젯 이동",
                            bytes(s.total - s.available),
                            bytes(s.total),
                            bytes(s.available)
                        )
                    } else {
                        "메모리를 읽고 있습니다".into()
                    };
                    drag.on_hover_text(tooltip);
                    let targets: Vec<_> = self
                        .snapshot
                        .iter()
                        .flat_map(|s| s.processes.iter())
                        .filter(|p| p.eligible)
                        .cloned()
                        .collect();
                    let recent = self
                        .trim_finished_at
                        .is_some_and(|at| at.elapsed() < Duration::from_secs(2));
                    let icon = if recent && self.last_trim.is_some_and(|(count, _, _)| count > 0) {
                        Icon::Check
                    } else {
                        Icon::Optimize
                    };
                    let label = if self.busy {
                        "메모리 정리 중".into()
                    } else {
                        format!("메모리 정리 · {}개 앱, 효과는 일시적입니다", targets.len())
                    };
                    if ui
                        .add_enabled_ui(!self.busy && !stale && !targets.is_empty(), |ui| {
                            icon_button(ui, icon, false, &label)
                        })
                        .inner
                        .clicked()
                    {
                        self.busy = true;
                        self.notice = None;
                        self.last_trim = None;
                        self.failures.clear();
                        self.send(Command::Trim(targets));
                    }
                    if icon_button(ui, Icon::Expand, false, "위젯 펼치기").clicked() {
                        self.set_compact(ctx, false);
                    }
                });
            });
    }

    fn widget(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG).stroke(Stroke::new(1.0_f32,LINE))
                .corner_radius(18).inner_margin(22).outer_margin(8))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing = Vec2::new(8.0,6.0);
                ui.style_mut().interaction.selectable_labels = false;
                ui.horizontal(|ui| {
                    let (header,_) = ui.allocate_exact_size(Vec2::new(ui.available_width()-108.0,28.0),Sense::hover());
                    let drag = self.widget_drag(ui, ctx, header, 22.0);
                    drag.widget_info(||egui::WidgetInfo::labeled(egui::WidgetType::Other,true,"위젯 이동 영역"));
                    for (i,h) in [10.0,20.0,14.0].iter().enumerate() {
                        ui.painter().rect_filled(Rect::from_min_size(Pos2::new(header.left()+i as f32*7.0,header.center().y+10.0-h),Vec2::new(4.0,*h)),2.0,ACCENT);
                    }
                    ui.painter().text(Pos2::new(header.left()+31.0,header.center().y),Align2::LEFT_CENTER,"memori",FontId::proportional(20.0),INK);
                    drag.on_hover_text("상단 여백·로고·이름을 잡아 위젯 이동");
                    if icon_button(ui,Icon::Collapse,false,"컴팩트 모드로 접기").clicked() {self.set_compact(ctx,true);}
                    if icon_button(ui, Icon::Pin, self.pinned, "항상 위에 표시").clicked() {
                        self.preferences.pinned = !self.pinned;
                        if self.persist_preferences() {
                            self.pinned = self.preferences.pinned;
                            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(if self.pinned {egui::WindowLevel::AlwaysOnTop} else {egui::WindowLevel::Normal}));
                        } else { self.preferences.pinned = self.pinned; }
                    }
                    if icon_button(ui, Icon::Close, false, "Memori 종료").clicked() { ctx.send_viewport_cmd(egui::ViewportCommand::Close); }
                });
                ui.add_space(10.0);
                let Some(s) = self.snapshot.clone() else { ui.label(RichText::new("메모리를 읽고 있습니다…").color(MUTED)); return; };
                let targets: Vec<_> = s.processes.iter().filter(|p| p.eligible).cloned().collect();
                let count = targets.len();
                ui.horizontal(|ui| {
                    ui.label(RichText::new("사용 가능한 메모리").size(12.0).color(MUTED));
                    ui.with_layout(Layout::right_to_left(Align::Center),|ui| {
                        ui.label(RichText::new(format!("{:.0}% 사용 중",s.percent()*100.0)).size(12.0).color(MUTED));
                    });
                });
                ui.horizontal(|ui| {
                    ui.label(RichText::new(bytes(s.available)).size(29.0).strong().color(INK));
                    ui.label(RichText::new(format!("/ {}",bytes(s.total))).size(13.0).color(MUTED));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(format!("CPU {}",cpu_usage(s.cpu_percent))).size(12.0).color(INK)).on_hover_text(CPU_HELP);
                    });
                });
                let (strip,_) = ui.allocate_exact_size(Vec2::new(ui.available_width(),18.0),Sense::hover());
                let used = (s.percent()*40.0).round() as usize;
                let cell = (strip.width()-3.0*39.0)/40.0;
                for i in 0..40 {
                    let r = Rect::from_min_size(Pos2::new(strip.left()+i as f32*(cell+3.0),strip.top()),Vec2::new(cell,strip.height()));
                    ui.painter().rect_filled(r,1.5,if i < used {LINE} else {ACCENT});
                }
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("정리 가능 {count}개 앱")).size(11.0).color(MUTED));
                    ui.with_layout(Layout::right_to_left(Align::Center),|ui| { ui.label(RichText::new("자동 정리 없음").size(11.0).color(MUTED)); });
                });
                ui.add_space(5.0);
                let stale = s.at.elapsed().map_or(true, |age|age > Duration::from_secs(10));
                let label = if self.busy { "정리 중…" } else { "메모리 정리" };
                if ui.add_enabled(count > 0 && !self.busy && !stale,egui::Button::new(RichText::new(label).size(14.0).strong().color(NAV)).fill(ACCENT).corner_radius(12).min_size(Vec2::new(ui.available_width(),42.0)))
                    .on_hover_text(format!("내 계정의 정리 가능한 앱 {count}개 작업 집합을 줄입니다. 앱을 종료하지 않으며 다음 사용 시 잠깐 느려질 수 있습니다.")).clicked() {
                    self.busy = true; self.notice = None; self.last_trim = None; self.failures.clear(); self.send(Command::Trim(targets));
                }
                let feedback = if let Some((count,reclaimed,failed)) = self.last_trim {
                    format!("{count}개 · {} 감소{}",bytes(reclaimed),if failed > 0 {format!(" · 제외 {failed}")} else {String::new()})
                } else if self.notice.as_ref().is_some_and(|(_,error)|*error) { "조회 실패 · 관리 화면에서 확인하세요".into() }
                else if stale { "조회 지연 · 잠시 후 다시 시도하세요".into() }
                else { "앱 사용 시 메모리가 다시 늘어날 수 있어요".into() };
                ui.add(egui::Label::new(RichText::new(&feedback).size(11.0).color(if stale {WARN} else {MUTED})).truncate()).on_hover_text(feedback);
                ui.add_space(7.0);
                ui.separator();
                ui.add_space(5.0);
                self.widget_processes(ui, &s);
                ui.add_space(3.0);
                ui.horizontal(|ui| {
                    if ui.add(egui::Button::new("전체 프로세스").frame(false)).clicked() {
                        self.navigate(Page::Memory);
                        self.process_sort = self.widget_sort; self.set_widget(ctx,false);
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center),|ui| {
                        if ui.add(egui::Button::new("시작 프로그램").frame(false)).clicked() {
                            self.navigate(Page::Startup); self.set_widget(ctx,false);
                        }
                        if ui.add(egui::Button::new("최적화").frame(false)).clicked() {
                            self.navigate(Page::Optimization); self.set_widget(ctx,false);
                        }
                    });
                });
            });
    }

    fn widget_processes(&mut self, ui: &mut egui::Ui, snapshot: &Snapshot) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("프로세스").size(14.0).strong());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(if self.widget_sort.descending {
                        "상위 5개 · 내 계정"
                    } else {
                        "하위 5개 · 내 계정"
                    })
                    .size(11.0)
                    .color(MUTED),
                );
            });
        });
        let name_width = (ui.available_width() - 156.0).max(80.0);
        ui.horizontal(|ui| {
            cell(
                ui,
                name_width,
                30.0,
                RichText::new("앱 이름").size(11.0).color(MUTED),
                Align::LEFT,
            );
            sort_header(
                ui,
                &mut self.widget_sort,
                Column::Cpu,
                "CPU",
                58.0,
                Align::RIGHT,
            );
            sort_header(
                ui,
                &mut self.widget_sort,
                Column::Memory,
                "RAM",
                82.0,
                Align::RIGHT,
            );
        });
        let mut rows: Vec<_> = snapshot.processes.iter().collect();
        self.widget_sort.sort(&mut rows);
        if rows.is_empty() {
            ui.add_sized(
                [ui.available_width(), 154.0],
                egui::Label::new(
                    RichText::new("조회할 수 있는 프로세스가 없습니다")
                        .size(12.0)
                        .color(MUTED),
                ),
            );
        } else {
            ui.scope(|ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                for process in rows.iter().take(5) {
                    ui.push_id((process.pid, process.created), |ui| {
                        ui.horizontal(|ui| {
                            cell(
                                ui,
                                name_width,
                                28.0,
                                RichText::new(&process.name).size(12.0).color(INK),
                                Align::LEFT,
                            )
                            .on_hover_text(format!(
                                "{} · PID {}\n{}",
                                process.name, process.pid, process.path
                            ));
                            cell(
                                ui,
                                58.0,
                                28.0,
                                RichText::new(cpu_usage(process.cpu_percent))
                                    .size(12.0)
                                    .color(if process.cpu_percent.is_some() {
                                        INK
                                    } else {
                                        MUTED
                                    }),
                                Align::RIGHT,
                            )
                            .on_hover_text(CPU_HELP);
                            cell(
                                ui,
                                82.0,
                                28.0,
                                RichText::new(bytes(process.bytes))
                                    .size(12.0)
                                    .strong()
                                    .color(if self.widget_sort.column == Column::Memory {
                                        ACCENT
                                    } else {
                                        INK
                                    }),
                                Align::RIGHT,
                            )
                            .on_hover_text("RAM 작업 집합 · 공유 페이지를 포함합니다");
                        });
                    });
                }
                if rows.len() < 5 {
                    ui.add_space((5 - rows.len()) as f32 * 32.0);
                }
            });
        }
    }

    fn sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("navigation")
            .exact_width(174.0)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(NAV)
                    .inner_margin(egui::Margin::symmetric(18, 26)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(26.0), Sense::hover());
                    for (i, h) in [13.0, 23.0, 17.0].iter().enumerate() {
                        let r = Rect::from_min_size(
                            Pos2::new(rect.left() + i as f32 * 8.0, rect.bottom() - h),
                            Vec2::new(5.0, *h),
                        );
                        ui.painter().rect_filled(r, 2.0, ACCENT);
                    }
                    ui.label(RichText::new("memori").size(22.0).strong());
                });
                ui.add_space(42.0);
                for page in [Page::Memory, Page::Startup, Page::Optimization] {
                    let selected = self.page == page;
                    let response = ui.add_sized(
                        [ui.available_width(), 42.0],
                        egui::Button::new(
                            RichText::new(page.title()).size(14.0).color(if selected {
                                ACCENT
                            } else {
                                MUTED
                            }),
                        )
                        .fill(if selected { PANEL } else { NAV })
                        .stroke(Stroke::NONE),
                    );
                    if response.clicked() {
                        self.navigate(page);
                    }
                    ui.add_space(4.0);
                }
                ui.with_layout(Layout::bottom_up(Align::LEFT), |ui| {
                    ui.label(
                        RichText::new(concat!("v", env!("CARGO_PKG_VERSION"), " · Windows"))
                            .size(11.0)
                            .color(MUTED),
                    );
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new("필요할 때만, 가볍게.")
                            .size(12.0)
                            .color(MUTED),
                    );
                    ui.separator();
                    if button(ui, "작은 위젯으로").clicked() {
                        self.set_widget(ctx, true);
                    }
                    if ui
                        .add(egui::Button::new("Windows 시작 앱 설정").frame(false))
                        .clicked()
                    {
                        self.settings();
                    }
                });
            });
    }

    fn optimization(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .id_salt("optimization")
            .show(ui, |ui| {
                self.power_controls(ui);
                ui.add_space(16.0);
                ui.separator();
                ui.add_space(8.0);
                self.storage_controls(ui);
            });
    }

    fn power_controls(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("앱별 전력 관리").size(19.0).strong());
            ui.label(
                RichText::new("절전 우선으로 백그라운드 작업의 전력 사용을 줄일 수 있습니다.")
                    .small()
                    .color(MUTED),
            );
        });
        ui.add_space(6.0);
        ui.add(
            egui::TextEdit::singleline(&mut self.search)
                .hint_text("앱 이름 또는 PID 검색")
                .desired_width(235.0)
                .margin(Vec2::splat(7.0)),
        );
        let query = self.search.to_lowercase();
        let mut rows: Vec<_> = self
            .snapshot
            .iter()
            .flat_map(|s| &s.processes)
            .filter(|p| {
                p.name.to_lowercase().contains(&query) || p.pid.to_string().contains(&query)
            })
            .collect();
        ProcessSort {
            column: Column::Cpu,
            descending: true,
        }
        .sort(&mut rows);
        let mut picked = None;
        let row_height = (ui.text_style_height(&egui::TextStyle::Button)
            + 2.0 * ui.spacing().button_padding.y)
            .max(29.0);
        egui::ScrollArea::vertical()
            .id_salt("power-apps")
            .max_height(124.0)
            .auto_shrink([false, true])
            .show_rows(ui, row_height, rows.len(), |ui, range| {
                for process in &rows[range] {
                    let id = (process.pid, process.created);
                    let label = format!(
                        "{}   ·   PID {}   ·   CPU {}",
                        process.name,
                        process.pid,
                        cpu_usage(process.cpu_percent)
                    );
                    ui.push_id(id, |ui| {
                        if ui
                            .add_enabled(
                                !self.power_busy && !self.busy,
                                egui::Button::selectable(self.power_target == Some(id), label)
                                    .min_size(Vec2::new(ui.available_width(), row_height))
                                    .truncate(),
                            )
                            .on_hover_text(&process.path)
                            .clicked()
                        {
                            picked = Some((*process).clone());
                        }
                    });
                }
                if rows.is_empty() {
                    ui.label(
                        RichText::new(if self.snapshot.is_some() {
                            "일치하는 앱이 없습니다."
                        } else {
                            "프로세스 목록을 읽고 있습니다…"
                        })
                        .color(MUTED),
                    );
                }
            });
        if let Some(process) = picked {
            self.power_target = Some((process.pid, process.created));
            self.power = None;
            self.power_busy = true;
            self.send(Command::ReadPower(process));
        }
        let target = self.snapshot.as_ref().and_then(|s| {
            s.processes
                .iter()
                .find(|p| Some((p.pid, p.created)) == self.power_target)
        });
        let mut power_command = None;
        if let Some(process) = target {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(format!("{} · {}", process.name, process.pid)).strong());
                if self.power_busy {
                    ui.label(RichText::new("전력 정책을 확인하고 있습니다…").color(MUTED));
                } else if let Some(Ok(policy)) = self.power.as_ref() {
                    let state = match policy {
                        PowerPolicy::Automatic => "Windows 자동 관리",
                        PowerPolicy::Eco => "절전 우선 적용 중",
                        PowerPolicy::Performance => "절전 제한 해제 상태",
                    };
                    ui.label(RichText::new(state).color(if *policy == PowerPolicy::Eco {
                        ACCENT
                    } else {
                        MUTED
                    }));
                    for (desired, label) in [
                        (PowerPolicy::Eco, "절전 우선 적용"),
                        (PowerPolicy::Automatic, "Windows 자동 관리로 전환"),
                    ] {
                        if ui
                            .add_enabled(!self.busy && *policy != desired, egui::Button::new(label))
                            .clicked()
                        {
                            power_command =
                                Some(Command::SetPower(process.clone(), *policy, desired));
                        }
                    }
                } else if let Some(Err(error)) = self.power.as_ref() {
                    ui.label(RichText::new(error).color(WARN));
                } else {
                    ui.label(RichText::new("상태를 읽으려면 앱을 다시 선택하세요.").color(MUTED));
                }
            });
        } else {
            ui.label(
                RichText::new(if self.power_target.is_some() {
                    "선택한 앱이 종료되었습니다. 다른 앱을 선택하세요."
                } else {
                    "전력 정책을 확인할 앱을 선택하세요."
                })
                .color(MUTED),
            );
        }
        if let Some(command) = power_command {
            self.power_busy = true;
            self.send(command);
        }
        ui.label(
            RichText::new(
                "앱 실행 중에 적용됩니다. 속도가 중요한 작업은 Windows 자동 관리를 사용하세요.",
            )
            .small()
            .color(MUTED),
        );
    }

    fn storage_controls(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("저장소 여유 공간").size(19.0).strong());
            if button(ui, "Windows 저장소 열기").clicked() {
                self.open_settings(
                    "ms-settings:storagesense",
                    "설정 > 시스템 > 저장소를 열어 주세요.",
                );
            }
        });
        match self.storage.as_ref() {
            Some(Ok(drives)) if drives.is_empty() => {
                ui.label(RichText::new("조회할 고정 드라이브가 없습니다.").color(MUTED));
            }
            Some(Ok(drives)) => {
                for drive in drives {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("{}:", drive.letter)).strong());
                        if let Some(space) = &drive.space {
                            let free = space.available as f32 / space.total as f32;
                            ui.add(
                                egui::ProgressBar::new(1.0 - free)
                                    .desired_width(124.0)
                                    .fill(if free < 0.1 { WARN } else { ACCENT }),
                            )
                            .on_hover_text(format!("{:.0}% 사용 중", (1.0 - free) * 100.0));
                            ui.label(format!(
                                "{} 여유 / {}",
                                bytes(space.available),
                                bytes(space.total)
                            ));
                            if free < 0.1 {
                                ui.label(RichText::new("여유 10% 미만").small().color(WARN));
                            }
                        } else {
                            ui.label(RichText::new("용량 조회 불가").color(WARN));
                        }
                    });
                }
            }
            Some(Err(error)) => {
                ui.label(RichText::new(error).color(WARN));
            }
            None => {
                ui.label(
                    RichText::new(if self.storage_busy {
                        "드라이브를 확인하고 있습니다…"
                    } else {
                        "새로 고침으로 드라이브를 확인하세요."
                    })
                    .color(MUTED),
                );
            }
        }
    }

    fn notice(&mut self, ui: &mut egui::Ui) {
        if let Some((message, is_error)) = self.notice.clone() {
            egui::Frame::new()
                .fill(PANEL)
                .inner_margin(12)
                .corner_radius(6)
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.set_min_width(ui.available_width());
                        ui.label(RichText::new(message).color(if is_error {
                            ERROR
                        } else {
                            ACCENT
                        }));
                        if ui.small_button("닫기").clicked() {
                            self.notice = None;
                            self.failures.clear();
                        }
                    });
                    if !self.failures.is_empty() {
                        ui.collapsing(
                            format!("정리하지 못한 앱 {}개", self.failures.len()),
                            |ui| {
                                egui::ScrollArea::vertical()
                                    .max_height(90.0)
                                    .show(ui, |ui| {
                                        for error in &self.failures {
                                            ui.label(RichText::new(error).size(12.0).color(MUTED));
                                        }
                                    });
                            },
                        );
                    }
                });
            ui.add_space(10.0);
        }
    }

    fn memory(&mut self, ui: &mut egui::Ui) {
        let Some(s) = self.snapshot.clone() else {
            ui.add_space(24.0);
            ui.label(RichText::new("Windows 메모리 정보를 읽고 있습니다…").color(MUTED));
            return;
        };
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!(
                    "물리 메모리  {} / {}",
                    bytes(s.total - s.available),
                    bytes(s.total)
                ))
                .size(16.0)
                .strong(),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(format!("{:.0}% 사용 중", s.percent() * 100.0))
                        .size(16.0)
                        .color(ACCENT),
                );
            });
        });
        ui.add_space(4.0);
        self.chart(ui);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(format!("사용 가능  {}", bytes(s.available))).color(ACCENT));
            ui.label(RichText::new("   /   ").color(MUTED));
            ui.label(
                RichText::new(format!(
                    "커밋  {} / {}",
                    bytes(s.commit),
                    bytes(s.commit_limit)
                ))
                .color(MUTED),
            )
            .on_hover_text(
                "Windows가 프로세스에 약속한 메모리 / RAM과 페이지 파일을 합친 커밋 한도",
            );
            ui.label(RichText::new(format!("  /  CPU {}", cpu_usage(s.cpu_percent))).color(INK))
                .on_hover_text(CPU_HELP);
        });
        ui.add_space(20.0);
        ui.separator();
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("프로세스").size(19.0).strong());
            ui.label(
                RichText::new(format!("{}개 프로세스", s.processes.len()))
                    .small()
                    .color(MUTED),
            );
            ui.label(
                RichText::new(format!(
                    "{} · {}",
                    match self.process_sort.column {
                        Column::Name => "앱 이름",
                        Column::Pid => "PID",
                        Column::Cpu => "CPU",
                        Column::Memory => "작업 집합",
                        Column::Status => "정리 상태",
                    },
                    if self.process_sort.descending {
                        "내림차순"
                    } else {
                        "오름차순"
                    }
                ))
                .small()
                .color(MUTED),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.search)
                        .hint_text("앱 이름 또는 PID 검색")
                        .desired_width(200.0)
                        .margin(Vec2::splat(7.0)),
                );
            });
        });
        let filter = self.search.to_lowercase();
        let mut processes: Vec<_> = s
            .processes
            .iter()
            .filter(|p| {
                p.name.to_lowercase().contains(&filter) || p.pid.to_string().contains(&filter)
            })
            .collect();
        ui.add_space(5.0);
        let narrow = ui.available_width() < 720.0;
        ui.horizontal(|ui| {
            let mut all = processes.iter().any(|p| p.eligible)
                && processes
                    .iter()
                    .filter(|p| p.eligible)
                    .all(|p| self.selected.contains(&(p.pid, p.created)));
            let all_response = ui
                .add_enabled(
                    !self.busy && processes.iter().any(|p| p.eligible),
                    egui::Checkbox::without_text(&mut all),
                )
                .on_hover_text("검색 결과의 정리 가능한 앱 모두 선택");
            all_response.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::Checkbox,
                    all_response.enabled(),
                    all,
                    "검색 결과의 정리 가능한 앱 모두 선택",
                )
            });
            if all_response.changed() {
                for p in &processes {
                    if p.eligible {
                        if all {
                            self.selected.insert((p.pid, p.created));
                        } else {
                            self.selected.remove(&(p.pid, p.created));
                        }
                    }
                }
            }
            let name_width = (ui.available_width() - if narrow { 278.0 } else { 428.0 }).max(80.0);
            sort_header(
                ui,
                &mut self.process_sort,
                Column::Name,
                "앱 이름",
                name_width,
                Align::LEFT,
            );
            sort_header(
                ui,
                &mut self.process_sort,
                Column::Pid,
                "PID",
                72.0,
                Align::RIGHT,
            );
            sort_header(
                ui,
                &mut self.process_sort,
                Column::Cpu,
                "CPU",
                76.0,
                Align::RIGHT,
            );
            sort_header(
                ui,
                &mut self.process_sort,
                Column::Memory,
                "작업 집합",
                100.0,
                Align::RIGHT,
            );
            if !narrow {
                sort_header(
                    ui,
                    &mut self.process_sort,
                    Column::Status,
                    "정리 상태",
                    140.0,
                    Align::LEFT,
                );
            }
        });
        self.process_sort.sort(&mut processes);
        ui.separator();
        let list_height = (ui.available_height() - 120.0).max(55.0);
        egui::ScrollArea::vertical()
            .id_salt("processes")
            .max_height(list_height)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if processes.is_empty() {
                    ui.add_space(18.0);
                    ui.label(
                        RichText::new("일치하는 앱이 없습니다. 이름이나 PID를 바꿔 보세요.")
                            .color(MUTED),
                    );
                }
                for p in processes {
                    ui.push_id((p.pid, p.created), |ui| {
                        ui.horizontal(|ui| {
                            ui.set_min_height(34.0);
                            let mut selected = self.selected.contains(&(p.pid, p.created));
                            let check_response = ui
                                .add_enabled(
                                    p.eligible && !self.busy,
                                    egui::Checkbox::without_text(&mut selected),
                                )
                                .on_hover_text(&p.reason);
                            check_response.widget_info(|| {
                                egui::WidgetInfo::selected(
                                    egui::WidgetType::Checkbox,
                                    check_response.enabled(),
                                    selected,
                                    format!("{} PID {} 메모리 정리 선택", p.name, p.pid),
                                )
                            });
                            if check_response.changed() {
                                if selected {
                                    self.selected.insert((p.pid, p.created));
                                } else {
                                    self.selected.remove(&(p.pid, p.created));
                                }
                            }
                            let reserved = if narrow { 278.0 } else { 428.0 };
                            cell(
                                ui,
                                (ui.available_width() - reserved).max(80.0),
                                28.0,
                                RichText::new(&p.name).color(if p.eligible { INK } else { MUTED }),
                                Align::LEFT,
                            )
                            .on_hover_text(&p.path);
                            cell(
                                ui,
                                72.0,
                                28.0,
                                RichText::new(p.pid.to_string()).color(MUTED),
                                Align::RIGHT,
                            );
                            cell(
                                ui,
                                76.0,
                                28.0,
                                RichText::new(cpu_usage(p.cpu_percent))
                                    .color(if p.cpu_percent.is_some() { INK } else { MUTED }),
                                Align::RIGHT,
                            )
                            .on_hover_text(CPU_HELP);
                            cell(
                                ui,
                                100.0,
                                28.0,
                                RichText::new(bytes(p.bytes)).color(INK),
                                Align::RIGHT,
                            );
                            if !narrow {
                                cell(
                                    ui,
                                    140.0,
                                    28.0,
                                    RichText::new(&p.reason).small().color(if p.eligible {
                                        MUTED
                                    } else {
                                        WARN
                                    }),
                                    Align::LEFT,
                                );
                            }
                        });
                        ui.painter().hline(
                            ui.min_rect().x_range(),
                            ui.cursor().top(),
                            Stroke::new(0.5_f32, LINE),
                        );
                    });
                }
            });
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            let count = self.selected.len();
            let text = if self.busy {
                "정리 중…".into()
            } else if count == 0 {
                "앱을 선택해 메모리 정리".into()
            } else {
                format!("선택한 {count}개 정리")
            };
            let primary = egui::Button::new(RichText::new(text).color(NAV).strong())
                .fill(ACCENT)
                .min_size(Vec2::new(200.0, 38.0));
            if ui.add_enabled(count > 0 && !self.busy, primary).clicked() {
                let targets = s
                    .processes
                    .iter()
                    .filter(|p| self.selected.contains(&(p.pid, p.created)))
                    .cloned()
                    .collect();
                self.busy = true;
                self.notice = None;
                self.failures.clear();
                self.send(Command::Trim(targets));
            }
            if count > 0 && !self.busy && ui.small_button("선택 해제").clicked() {
                self.selected.clear();
            }
        });
        ui.add_space(5.0);
        ui.label(RichText::new("앱을 종료하지 않고 작업 집합을 줄입니다. 다시 사용할 때 잠깐 느려지거나 메모리가 늘 수 있습니다.").small().color(MUTED));
    }

    fn chart(&self, ui: &mut egui::Ui) {
        let height = if ui.available_height() < 540.0 {
            105.0
        } else {
            150.0
        };
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
        let plot = Rect::from_min_max(
            rect.min + Vec2::new(0.0, 8.0),
            rect.max - Vec2::new(38.0, 23.0),
        );
        let painter = ui.painter();
        for (y, text) in [
            (plot.top(), "100%"),
            (plot.center().y, "50%"),
            (plot.bottom(), "0% "),
        ] {
            painter.hline(plot.x_range(), y, Stroke::new(0.7_f32, LINE));
            painter.text(
                Pos2::new(rect.right(), y),
                Align2::RIGHT_CENTER,
                text,
                FontId::proportional(11.0),
                MUTED,
            );
        }
        let now = Instant::now();
        let points: Vec<Pos2> = self
            .history
            .iter()
            .map(|(t, v)| {
                Pos2::new(
                    plot.right() - (now.duration_since(*t).as_secs_f32() / 120.0) * plot.width(),
                    plot.bottom() - v * plot.height(),
                )
            })
            .collect();
        if points.len() >= 2 {
            // A mesh avoids treating a non-convex history as a convex polygon.
            let mut mesh = egui::Mesh::default();
            for pair in points.windows(2) {
                let start = mesh.vertices.len() as u32;
                for p in [
                    pair[0],
                    pair[1],
                    Pos2::new(pair[1].x, plot.bottom()),
                    Pos2::new(pair[0].x, plot.bottom()),
                ] {
                    mesh.colored_vertex(p, ACCENT.gamma_multiply(0.08));
                }
                mesh.add_triangle(start, start + 1, start + 2);
                mesh.add_triangle(start, start + 2, start + 3);
            }
            painter.add(egui::Shape::mesh(mesh));
            painter.add(egui::Shape::line(
                points.clone(),
                Stroke::new(2.0_f32, ACCENT),
            ));
        }
        if let Some(p) = points.last() {
            painter.circle_filled(*p, 3.5, ACCENT);
        }
        painter.text(
            Pos2::new(plot.left(), rect.bottom()),
            Align2::LEFT_BOTTOM,
            "120초 전",
            FontId::proportional(11.0),
            MUTED,
        );
        painter.text(
            Pos2::new(plot.right(), rect.bottom()),
            Align2::RIGHT_BOTTOM,
            "지금",
            FontId::proportional(11.0),
            MUTED,
        );
        if self.history.len() < 3 {
            painter.text(
                plot.center(),
                Align2::CENTER_CENTER,
                "기록을 수집하고 있습니다 · 2초마다 갱신",
                FontId::proportional(12.0),
                MUTED,
            );
        }
    }

    fn startup(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("로그인할 때 실행되는 앱을 고르세요.").size(16.0));
        ui.add_space(4.0);
        ui.label(
            RichText::new("내 계정 항목은 원본을 보관한 뒤 끄고, 언제든 복원할 수 있습니다.")
                .color(MUTED),
        );
        ui.add_space(8.0);
        let mut launch_at_login = self.launch_at_login;
        if ui.add_enabled(!self.startup_busy, egui::Checkbox::new(&mut launch_at_login,"Windows 로그인 시 Memori 위젯 자동 실행"))
            .on_hover_text("현재 계정의 시작 항목에 Memori만 등록합니다. Windows가 차단한 항목은 Windows 시작 앱 설정에서 확인하세요.").changed() {
            match preferences::set_launch_at_login(launch_at_login) {
                Ok(()) => { self.launch_at_login = launch_at_login; self.startup_busy = true; self.send(Command::Startup); }
                Err(error) => { self.notice = Some((error,true)); }
            }
        }
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            for (filter, label) in [
                (StartupFilter::All, "전체"),
                (StartupFilter::Enabled, "켜짐"),
                (StartupFilter::Disabled, "꺼짐"),
                (StartupFilter::Unknown, "확인 필요"),
            ] {
                let count = self
                    .entries
                    .iter()
                    .filter(|e| filter.matches(e.state))
                    .count();
                let selected = self.startup_filter == filter;
                if ui
                    .add(
                        egui::Button::selectable(
                            selected,
                            RichText::new(format!("{label} {count}"))
                                .size(12.0)
                                .color(if selected { ACCENT } else { MUTED }),
                        )
                        .corner_radius(8)
                        .min_size(Vec2::new(0.0, 30.0)),
                    )
                    .clicked()
                {
                    self.startup_filter = filter;
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.search)
                        .hint_text("시작 앱 검색")
                        .desired_width(if ui.available_width() < 230.0 {
                            160.0
                        } else {
                            220.0
                        })
                        .margin(Vec2::splat(7.0)),
                );
            });
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let name_width = (ui.available_width() - 286.0).max(120.0);
            cell(
                ui,
                name_width,
                24.0,
                RichText::new("프로그램 / 등록 위치").small().color(MUTED),
                Align::LEFT,
            );
            cell(
                ui,
                130.0,
                24.0,
                RichText::new("자동 실행").small().color(MUTED),
                Align::LEFT,
            );
            cell(
                ui,
                136.0,
                24.0,
                RichText::new("관리").small().color(MUTED),
                Align::LEFT,
            );
        });
        ui.separator();
        if self.startup_busy && !self.startup_loaded {
            ui.add_space(24.0);
            ui.label(RichText::new("시작 프로그램을 읽고 있습니다…").color(MUTED));
            return;
        }
        let query = self.search.to_lowercase();
        let entries: Vec<_> = self
            .entries
            .iter()
            .filter(|e| {
                self.startup_filter.matches(e.state)
                    && (e.name.to_lowercase().contains(&query)
                        || e.command.to_lowercase().contains(&query))
            })
            .cloned()
            .collect();
        let compact_details = self.details.is_some() && ui.available_height() < 360.0;
        let reserve = if self.details.is_some() {
            if compact_details { 216.0 } else { 260.0 }
        } else {
            112.0
        };
        let max_height = (ui.available_height() - reserve).max(if self.details.is_some() {
            66.0
        } else {
            100.0
        });
        egui::ScrollArea::vertical().id_salt("startup-list").max_height(max_height).auto_shrink([false,false]).show(ui,|ui| {
            if entries.is_empty() {
                ui.add_space(24.0);
                ui.label(RichText::new(if self.entries.is_empty() {"등록된 Run / 시작 폴더 항목이 없습니다. 다른 시작 앱은 Windows 설정에서 확인하세요."} else {"조건에 맞는 시작 앱이 없습니다. 필터나 검색어를 바꿔 보세요."}).color(MUTED));
            }
            let row_width=ui.available_width();
            for entry in entries {
                ui.push_id(&entry.id,|ui| {
                    let (row,hover)=ui.allocate_exact_size(Vec2::new(row_width,66.0),Sense::hover());
                    let selected=self.details.as_deref()==Some(entry.id.as_str());
                    if selected {
                        ui.painter().rect_filled(row,6,PANEL);
                        ui.painter().rect_stroke(row,6,Stroke::new(1.0_f32,ACCENT),egui::StrokeKind::Inside);
                    } else if hover.hovered() {ui.painter().rect_filled(row,6,PANEL.gamma_multiply(0.5));}
                    let name_width=(row_width-286.0).max(120.0);
                    let name_rect=Rect::from_min_size(row.min+Vec2::new(0.0,7.0),Vec2::new(name_width,52.0));
                    let mut name_ui=ui.new_child(egui::UiBuilder::new().max_rect(name_rect).layout(Layout::top_down(Align::LEFT)));
                    let off=matches!(entry.state,StartupState::Disabled|StartupState::WindowsDisabled);
                    let name_response=name_ui.add(egui::Button::new(RichText::new(&entry.name).size(15.0).color(if off && !selected {MUTED} else {INK})).frame(false).truncate());
                    name_response.widget_info(||egui::WidgetInfo::selected(egui::WidgetType::Button,true,selected,format!("{} 상세 정보 {}",entry.name,if selected {"닫기"} else {"열기"})));
                    if name_response.on_hover_text(&entry.command).clicked() {
                        self.details=if selected {None} else {Some(entry.id.clone())};
                        ui.ctx().request_repaint();
                    }
                    name_ui.add(egui::Label::new(RichText::new(&entry.source).small().color(MUTED)).truncate());
                    let mut state_ui=ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_size(row.min+Vec2::new(name_width+10.0,0.0),Vec2::new(130.0,66.0))).layout(Layout::left_to_right(Align::Center)));
                    startup_badge(&mut state_ui,entry.state);
                    let mut action_ui=ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_size(Pos2::new(row.right()-136.0,row.top()),Vec2::new(136.0,66.0))).layout(Layout::left_to_right(Align::Center)));
                    if entry.manageable {
                        let on=entry.state==StartupState::Enabled;
                        let response=action_ui.add_enabled_ui(!self.startup_busy,|ui|startup_switch(ui,on,&format!("{} 자동 실행",entry.name))).inner;
                        if response.on_hover_text(format!("{} · {}\n{}",entry.name,if on {"눌러 끄기: 원본을 보관합니다"} else {"눌러 켜기: 원본을 복원합니다"},entry.detail)).clicked() {
                            self.startup_busy=true; self.send(Command::Toggle(entry.clone()));
                        }
                    } else if action_ui.add_sized([112.0,32.0],egui::Button::new("Windows 설정")).on_hover_text(&entry.detail).clicked() {self.settings();}
                    ui.painter().hline(row.x_range(),row.bottom(),Stroke::new(0.5_f32,LINE));
                });
            }
        });
        ui.add_space(12.0);
        if let Some(entry) = self
            .details
            .as_ref()
            .and_then(|id| self.entries.iter().find(|e| &e.id == id))
            .cloned()
        {
            egui::Frame::new()
                .fill(PANEL)
                .stroke(Stroke::new(1.0_f32, LINE))
                .inner_margin(if compact_details { 12 } else { 16 })
                .corner_radius(12)
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("상세 정보").size(16.0).strong());
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let close = button(ui, "닫기");
                            close.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::WidgetType::Button,
                                    close.enabled(),
                                    "상세 정보 닫기",
                                )
                            });
                            if close.on_hover_text("상세 정보 닫기 · Esc").clicked() {
                                self.details = None;
                                ui.ctx().request_repaint();
                            }
                        });
                    });
                    ui.add(egui::Label::new(RichText::new(&entry.name).strong()).truncate())
                        .on_hover_text(&entry.name);
                    egui::ScrollArea::vertical()
                        .id_salt("details")
                        .max_height(if compact_details { 40.0 } else { 76.0 })
                        .show(ui, |ui| {
                            ui.label(RichText::new("실행 명령").small().color(MUTED));
                            ui.label(RichText::new(&entry.command).small());
                            ui.label(RichText::new(&entry.detail).small().color(MUTED));
                        });
                });
        } else {
            ui.label(
                RichText::new("앱 이름을 누르면 실행 명령과 관리 범위를 확인할 수 있습니다.")
                    .small()
                    .color(MUTED),
            );
        }
        ui.add_space(12.0);
        ui.label(RichText::new("공용 항목·패키지 앱은 Windows 설정에서 관리하세요. 예약 작업과 서비스는 이 목록에 포함되지 않습니다.").small().color(MUTED));
        ui.label(
            RichText::new("변경 사항은 다음 로그인부터 적용됩니다.")
                .small()
                .color(MUTED),
        );
    }

    fn capture(&mut self, ctx: &egui::Context) {
        let Some(path) = self.screenshot.clone() else {
            return;
        };
        for event in ctx.input(|i| i.events.clone()) {
            if let egui::Event::Screenshot { image, .. } = event {
                let mut pixels = Vec::with_capacity(image.pixels.len() * 4);
                for pixel in &image.pixels {
                    pixels.extend_from_slice(&pixel.to_array());
                }
                match image::save_buffer(
                    &path,
                    &pixels,
                    image.size[0] as u32,
                    image.size[1] as u32,
                    image::ColorType::Rgba8,
                ) {
                    Ok(()) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                    Err(e) => {
                        self.notice = Some((format!("스크린샷 저장 실패: {e}"), true));
                        self.screenshot = None;
                    }
                }
            }
        }
        if !self.capture_sent
            && self.snapshot.is_some()
            && (self.page != Page::Startup || self.startup_loaded)
            && self.started.elapsed() > Duration::from_secs(4)
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            self.capture_sent = true;
        }
    }
}

impl eframe::App for App {
    fn on_exit(&mut self, _: Option<&eframe::glow::Context>) {
        if self.screenshot.is_none() {
            if self.widget
                && let Some(position) = window::position(self.window)
            {
                self.preferences.position = Some(position);
            }
            self.persist_preferences();
        }
    }
    fn clear_color(&self, _: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.receive();
        self.observe_widget_position(ctx);
        if !self.widget
            && self.page == Page::Startup
            && self.details.is_some()
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            self.details = None;
        }
        if self.widget {
            if self.preferences.compact {
                self.compact_widget(ctx);
            } else {
                self.widget(ctx);
            }
            self.capture(ctx);
            ctx.request_repaint_after(Duration::from_secs(1));
            return;
        }
        self.sidebar(ctx);
        egui::TopBottomPanel::bottom("status")
            .frame(
                egui::Frame::new()
                    .fill(NAV)
                    .inner_margin(egui::Margin::symmetric(22, 8)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let age = self
                        .snapshot
                        .as_ref()
                        .and_then(|s| s.at.elapsed().ok())
                        .map_or(0, |d| d.as_secs());
                    ui.label(
                        RichText::new(if age > 10 {
                            format!("조회 지연 · {age}초 전 데이터")
                        } else {
                            "로컬에서 실행 중 · 2초마다 갱신".into()
                        })
                        .small()
                        .color(if age > 10 { WARN } else { MUTED }),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new("자동 정리 없음").small().color(MUTED));
                    });
                });
            });
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(BG)
                    .inner_margin(egui::Margin::symmetric(30, 24)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(self.page.title()).size(27.0).strong());
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add_enabled(
                                !self.busy
                                    && !self.startup_busy
                                    && !self.power_busy
                                    && !self.storage_busy,
                                egui::Button::new("새로 고침").min_size(Vec2::new(85.0, 32.0)),
                            )
                            .clicked()
                        {
                            match self.page {
                                Page::Memory => self.send(Command::Refresh),
                                Page::Startup => {
                                    self.startup_busy = true;
                                    self.send(Command::Startup);
                                }
                                Page::Optimization => {
                                    self.storage_busy = true;
                                    self.send(Command::Storage);
                                    if let Some(process) = self
                                        .snapshot
                                        .as_ref()
                                        .and_then(|s| {
                                            s.processes.iter().find(|p| {
                                                Some((p.pid, p.created)) == self.power_target
                                            })
                                        })
                                        .cloned()
                                    {
                                        self.power_busy = true;
                                        self.send(Command::ReadPower(process));
                                    }
                                }
                            }
                        }
                    });
                });
                ui.add_space(19.0);
                self.notice(ui);
                match self.page {
                    Page::Memory => self.memory(ui),
                    Page::Startup => self.startup(ui),
                    Page::Optimization => self.optimization(ui),
                }
            });
        self.capture(ctx);
        // Slow repaint keeps idle CPU low while allowing relative timestamps to advance.
        ctx.request_repaint_after(Duration::from_secs(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn startup_filters_include_both_disabled_sources_without_hiding_unknown() {
        let states = [
            StartupState::Enabled,
            StartupState::Disabled,
            StartupState::WindowsDisabled,
            StartupState::Unknown,
        ];
        for (filter, count) in [
            (StartupFilter::All, 4),
            (StartupFilter::Enabled, 1),
            (StartupFilter::Disabled, 2),
            (StartupFilter::Unknown, 1),
        ] {
            assert_eq!(
                states
                    .iter()
                    .filter(|state| filter.matches(**state))
                    .count(),
                count
            );
        }
    }
    #[test]
    fn binary_memory_units() {
        assert_eq!(bytes(1024 * 1024), "1 MB");
        assert_eq!(bytes(1024 * 1024 * 1024), "1.0 GB");
    }

    #[test]
    fn centering_preserves_monitor_origin_and_dpi() {
        let size = Vec2::new(1120.0, 790.0);
        assert_eq!(
            centered_position(0, 0, 1920, 1080, 1.0, size),
            Pos2::new(400.0, 145.0)
        );
        assert_eq!(
            centered_position(1920, -420, 3054, 1500, 1.0, size),
            Pos2::new(1927.0, 145.0)
        );
        assert_eq!(
            centered_position(-2560, 0, 0, 1440, 2.0, size),
            Pos2::new(-1200.0, 0.0)
        );
    }
}
