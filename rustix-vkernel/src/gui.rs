use crate::fixed::InlineString;
use crate::services::{DirEntries, VfsService};
use crate::sync::{IrqSpinMutex, StaticCell};
use crate::wallpaper::{WALLPAPER_COLORS, WALLPAPER_PIXELS};

const WINDOW_WHITE: u8 = 64;
const TITLE_BLUE: u8 = 65;
const INK: u8 = 66;
const MINIMIZE_YELLOW: u8 = 67;
const ORANGE: u8 = 68;
const CLOSE_RED: u8 = 69;
const TASKBAR_CYAN: u8 = 70;
const SOFT_GRAY: u8 = 71;
const SELECTION: u8 = 72;
const TERMINAL_BLACK: u8 = 73;
const MUTED_TEXT: u8 = 74;
const EDITOR_PAPER: u8 = 75;
const START_GREEN: u8 = 76;
const VIDEO_PANEL: u8 = 77;
const AUDIO_PANEL: u8 = 78;
const SETTINGS_PANEL: u8 = 79;
const GLASS_TINT: u8 = 80;
const GLASS_HIGHLIGHT: u8 = 81;
const GLASS_EDGE: u8 = 82;
const GLASS_SHADOW: u8 = 83;
const PANEL_PAPER: u8 = 84;
const PANEL_INSET: u8 = 85;
const TITLE_MIST: u8 = 86;
const TASKBAR_DEEP: u8 = 87;
const ACTIVE_GLOW: u8 = 88;
const COPPER_ACCENT: u8 = 89;
const LOCK_VEIL: u8 = 90;
const AURORA_BLUE: u8 = 91;
const AURORA_GOLD: u8 = 92;
const AURORA_MINT: u8 = 93;
const NIGHT_INK: u8 = 94;

const ICON_COL_LEFT_X: i32 = 30;
const ICON_COL_RIGHT_X: i32 = 548;
const FILE_ICON_Y: i32 = 30;
const CMD_ICON_Y: i32 = 122;
const EDIT_ICON_Y: i32 = 214;
const SETTINGS_ICON_Y: i32 = 30;
const VIDEO_ICON_Y: i32 = 122;
const AUDIO_ICON_Y: i32 = 214;

const TASKBAR_W: i32 = 520;
const TASKBAR_H: i32 = 42;
const TASKBAR_SLOT_W: i32 = 36;
const TASKBAR_SLOT_H: i32 = 28;
const DOUBLE_CLICK_TICKS: u32 = 28;
const TITLE_BAR_HEIGHT: i32 = 24;
const FILE_ROW_HEIGHT: i32 = 24;
const MAX_BROWSER_ENTRIES: usize = 12;
const EDITOR_TEXT_CAPACITY: usize = 2048;
const CONTEXT_MENU_WIDTH: i32 = 126;
const CONTEXT_MENU_PADDING: i32 = 6;
const CONTEXT_MENU_ROW_HEIGHT: i32 = 24;

#[derive(Clone, Copy, PartialEq, Eq)]
enum WindowId {
    Files,
    Cmd,
    Editor,
    Settings,
    Video,
    Audio,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum IconId {
    Files,
    Cmd,
    Editor,
    Settings,
    Video,
    Audio,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BrowserKind {
    Directory,
    File,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ContextMenuKind {
    Desktop,
    Files,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ContextAction {
    DesktopFiles,
    DesktopCommand,
    DesktopSettings,
    FilesNewDirectory,
    FilesNewFile,
}

#[derive(Clone, Copy)]
struct BrowserEntry {
    name: InlineString<32>,
    kind: BrowserKind,
}

impl BrowserEntry {
    const fn new() -> Self {
        Self {
            name: InlineString::new(),
            kind: BrowserKind::File,
        }
    }
}

#[derive(Clone, Copy)]
struct SettingsConfig {
    brightness: u8,
    volume: u8,
    ui_scale: u8,
    theme: Theme,
}

#[derive(Clone, Copy)]
enum Theme {
    Graphite,
    Sunrise,
    Glacier,
}

impl SettingsConfig {
    const fn defaults() -> Self {
        Self {
            brightness: 72,
            volume: 58,
            ui_scale: 1,
            theme: Theme::Graphite,
        }
    }
}

impl Theme {
    const fn as_str(self) -> &'static str {
        match self {
            Theme::Graphite => "graphite",
            Theme::Sunrise => "sunrise",
            Theme::Glacier => "glacier",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum HitTarget {
    None,
    DesktopIcon(IconId),
    StartButton,
    StartMenuPanel,
    StartMenuItem(IconId),
    TaskbarIcon(WindowId),
    ClockChip,
    WindowTitle(WindowId),
    WindowBody(WindowId),
    Minimize(WindowId),
    Close(WindowId),
    FilesBack,
    FilesEntry(usize),
    ContextMenuPanel,
    ContextMenuItem(ContextAction),
    EditorSave,
    SettingsBrightnessDown,
    SettingsBrightnessUp,
    SettingsVolumeDown,
    SettingsVolumeUp,
    SettingsTheme,
    VideoPrev,
    VideoPlay,
    VideoNext,
    AudioPlay,
}

#[derive(Clone, Copy)]
struct WindowState {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    open: bool,
    minimized: bool,
}

#[derive(Clone, Copy)]
struct DragState {
    target: WindowId,
    offset_x: i32,
    offset_y: i32,
}

struct ContextMenuState {
    kind: Option<ContextMenuKind>,
    x: i32,
    y: i32,
    mouse_right: bool,
}

impl ContextMenuState {
    const fn new() -> Self {
        Self {
            kind: None,
            x: 0,
            y: 0,
            mouse_right: false,
        }
    }
}

#[derive(Clone, Copy)]
struct DesktopState {
    mouse_x: i32,
    mouse_y: i32,
    presented_mouse_x: i32,
    presented_mouse_y: i32,
    mouse_left: bool,
    cursor_dirty: bool,
    scene_dirty: bool,
    locked: bool,
    packet: [u8; 3],
    packet_len: usize,
    tick: u32,
    ambient_phase: u16,
    last_clock_poll_ns: u64,
    clock_time: Option<crate::arch::ClockTime>,
    active: WindowId,
    hover: HitTarget,
    pressed: HitTarget,
    dragging: Option<DragState>,
    start_menu_open: bool,
    selected_icon: Option<IconId>,
    last_icon_click: Option<(IconId, u32)>,
    selected_file_entry: Option<usize>,
    last_file_click: Option<(usize, u32)>,
    files_path: InlineString<64>,
    file_status: InlineString<64>,
    files_entries: [BrowserEntry; MAX_BROWSER_ENTRIES],
    files_len: usize,
    editor_path: InlineString<64>,
    editor_status: InlineString<64>,
    editor_text: InlineString<EDITOR_TEXT_CAPACITY>,
    settings_status: InlineString<64>,
    video_path: InlineString<64>,
    video_status: InlineString<64>,
    video_frame: usize,
    audio_path: InlineString<64>,
    audio_status: InlineString<64>,
    files: WindowState,
    cmd: WindowState,
    editor: WindowState,
    settings: WindowState,
    video: WindowState,
    audio: WindowState,
}

impl DesktopState {
    const fn new() -> Self {
        Self {
            mouse_x: 320,
            mouse_y: 240,
            presented_mouse_x: 320,
            presented_mouse_y: 240,
            mouse_left: false,
            cursor_dirty: false,
            scene_dirty: false,
            locked: true,
            packet: [0; 3],
            packet_len: 0,
            tick: 0,
            ambient_phase: 0,
            last_clock_poll_ns: 0,
            clock_time: None,
            active: WindowId::Cmd,
            hover: HitTarget::None,
            pressed: HitTarget::None,
            dragging: None,
            start_menu_open: false,
            selected_icon: None,
            last_icon_click: None,
            selected_file_entry: None,
            last_file_click: None,
            files_path: InlineString::new(),
            file_status: InlineString::new(),
            files_entries: [BrowserEntry::new(); MAX_BROWSER_ENTRIES],
            files_len: 0,
            editor_path: InlineString::new(),
            editor_status: InlineString::new(),
            editor_text: InlineString::new(),
            settings_status: InlineString::new(),
            video_path: InlineString::new(),
            video_status: InlineString::new(),
            video_frame: 0,
            audio_path: InlineString::new(),
            audio_status: InlineString::new(),
            files: WindowState {
                x: 254,
                y: 100,
                w: 356,
                h: 262,
                open: false,
                minimized: false,
            },
            cmd: WindowState {
                x: 110,
                y: 92,
                w: 286,
                h: 282,
                open: true,
                minimized: false,
            },
            editor: WindowState {
                x: 164,
                y: 70,
                w: 364,
                h: 324,
                open: false,
                minimized: false,
            },
            settings: WindowState {
                x: 378,
                y: 62,
                w: 234,
                h: 194,
                open: false,
                minimized: false,
            },
            video: WindowState {
                x: 196,
                y: 66,
                w: 382,
                h: 274,
                open: false,
                minimized: false,
            },
            audio: WindowState {
                x: 224,
                y: 108,
                w: 322,
                h: 234,
                open: false,
                minimized: false,
            },
        }
    }
}

static DESKTOP: StaticCell<DesktopState> = StaticCell::new(DesktopState::new());
static CONTEXT_MENU: StaticCell<ContextMenuState> = StaticCell::new(ContextMenuState::new());
static GUI_LOCK: IrqSpinMutex<(), 40> = IrqSpinMutex::new(());

unsafe fn desktop_ptr() -> *mut DesktopState {
    DESKTOP.get()
}

unsafe fn desktop_ref() -> &'static DesktopState {
    &*desktop_ptr()
}

unsafe fn desktop_mut() -> &'static mut DesktopState {
    &mut *desktop_ptr()
}

unsafe fn context_menu_ptr() -> *mut ContextMenuState {
    CONTEXT_MENU.get()
}

unsafe fn context_menu_ref() -> &'static ContextMenuState {
    &*context_menu_ptr()
}

pub fn init() {
    if !crate::arch::graphics_mode_enabled() {
        return;
    }

    let _guard = GUI_LOCK.lock();
    configure_palette();
    unsafe {
        let desktop = desktop_mut();
        if desktop.files_path.is_empty() {
            desktop.files_path.set("/");
        }
        if desktop.video_path.is_empty() {
            desktop.video_path.set("/usr/share/media/demo-video.rvf");
        }
        if desktop.audio_path.is_empty() {
            desktop.audio_path.set("/usr/share/media/demo-audio.rta");
        }
        if desktop.settings_status.is_empty() {
            desktop.settings_status.set("Settings");
        }
        if desktop.video_status.is_empty() {
            desktop.video_status.set("Ready");
        }
        if desktop.audio_status.is_empty() {
            desktop.audio_status.set("Ready");
        }
        desktop.clock_time = crate::arch::read_clock_time();
    }
    let _ = VfsService::new().create_file("/home/notes.txt");
    unsafe {
        refresh_files_view();
        let desktop = desktop_mut();
        desktop.start_menu_open = true;
        desktop.files.open = true;
        desktop.files.minimized = false;
        desktop.settings.open = true;
        desktop.settings.minimized = false;
    }

    render_desktop_locked();
}

pub fn sync_console() {
    if !crate::arch::graphics_mode_enabled() {
        return;
    }

    let _guard = GUI_LOCK.lock();
    render_desktop_locked();
}

pub fn tick_idle() {
    if !crate::arch::graphics_mode_enabled() {
        return;
    }

    let _guard = GUI_LOCK.lock();
    let mut needs_render = false;
    unsafe {
        let desktop = desktop_mut();
        let now_ns = crate::arch::monotonic_time_ns();
        if now_ns.wrapping_sub(desktop.last_clock_poll_ns) < 100_000_000 {
            return;
        }
        desktop.last_clock_poll_ns = now_ns;
        if let Some(time) = crate::arch::read_clock_time() {
            let minute_changed = desktop
                .clock_time
                .is_none_or(|old| old.hour != time.hour || old.minute != time.minute);
            desktop.clock_time = Some(time);
            if minute_changed {
                desktop.ambient_phase = desktop.ambient_phase.wrapping_add(1);
                needs_render = true;
            }
        }
    }

    if needs_render {
        render_desktop_locked();
    }
}

pub fn poll_input() -> bool {
    if !crate::arch::graphics_mode_enabled() {
        return false;
    }

    let mut processed = false;
    while let Some(input) = crate::arch::poll_console_input() {
        processed = true;
        match input {
            crate::arch::ConsoleInput::MouseByte(byte) => handle_mouse_byte(byte),
            crate::arch::ConsoleInput::Key { code, translated } => {
                let _ = handle_key(code, translated);
            }
            crate::arch::ConsoleInput::Byte(byte) => {
                let _ = handle_key(0, Some(byte));
            }
        }
    }
    flush_mouse();
    if !processed {
        tick_idle();
    }
    processed
}

pub fn poll_locked_input() -> bool {
    if !crate::arch::graphics_mode_enabled() {
        return false;
    }

    let locked = {
        let _guard = GUI_LOCK.lock();
        unsafe { (*desktop_ptr()).locked }
    };
    if locked {
        poll_input()
    } else {
        false
    }
}

pub fn handle_mouse_byte(byte: u8) {
    if !crate::arch::graphics_mode_enabled() {
        return;
    }

    let _guard = GUI_LOCK.lock();
    unsafe {
        if (*desktop_ptr()).packet_len == 0 && byte & 0x08 == 0 {
            return;
        }

        (*desktop_ptr()).packet[(*desktop_ptr()).packet_len] = byte;
        (*desktop_ptr()).packet_len += 1;
        if (*desktop_ptr()).packet_len < 3 {
            return;
        }

        let packet = (*desktop_ptr()).packet;
        (*desktop_ptr()).packet_len = 0;
        handle_mouse_packet(packet);
    }
}

pub fn flush_mouse() {
    if !crate::arch::graphics_mode_enabled() {
        return;
    }

    let _guard = GUI_LOCK.lock();
    unsafe {
        let desktop = desktop_mut();
        if desktop.scene_dirty {
            render_desktop_locked();
            return;
        }
        if desktop.cursor_dirty {
            crate::arch::gfx_move_cursor(
                desktop.presented_mouse_x,
                desktop.presented_mouse_y,
                desktop.mouse_x,
                desktop.mouse_y,
                INK,
                WINDOW_WHITE,
            );
            desktop.presented_mouse_x = desktop.mouse_x;
            desktop.presented_mouse_y = desktop.mouse_y;
        }
        desktop.cursor_dirty = false;
    }
}

pub fn handle_key(code: u8, translated: Option<u8>) -> bool {
    if !crate::arch::graphics_mode_enabled() {
        return false;
    }

    let _guard = GUI_LOCK.lock();
    unsafe {
        let desktop = desktop_mut();
        if desktop.locked {
            if translated.is_some()
                || matches!(code, 0x01 | 0x0E | 0x0F | 0x1C | 0x39 | 0x58 | 0x5A)
            {
                unlock_desktop();
                render_desktop_locked();
            }
            return true;
        }

        if code == 0x58 {
            lock_desktop();
            render_desktop_locked();
            return true;
        }

        if !desktop.editor.open || desktop.editor.minimized || desktop.active != WindowId::Editor {
            return false;
        }

        match code {
            0x01 => {
                close_window(WindowId::Editor);
                render_desktop_locked();
                return true;
            }
            0x3C => {
                save_editor();
                render_desktop_locked();
                return true;
            }
            _ => {}
        }

        let Some(byte) = translated else {
            return true;
        };

        match byte {
            0x08 => {
                let _ = desktop.editor_text.pop();
            }
            b'\n' => {
                let _ = desktop.editor_text.push_byte(b'\n');
            }
            b'\t' => {
                let _ = desktop.editor_text.push_str("    ");
            }
            b if (32..=126).contains(&b) => {
                let _ = desktop.editor_text.push_byte(b);
            }
            _ => return false,
        }

        desktop.editor_status.set("Modified");
        render_desktop_locked();
        true
    }
}

pub fn open_editor_for_path(path: &str) -> bool {
    if !crate::arch::graphics_mode_enabled() {
        return false;
    }

    let _guard = GUI_LOCK.lock();
    unsafe {
        let opened = open_editor_internal(path);
        if opened {
            render_desktop_locked();
        }
        opened
    }
}

pub fn render_desktop() {
    if !crate::arch::graphics_mode_enabled() {
        return;
    }

    let _guard = GUI_LOCK.lock();
    render_desktop_locked();
}

fn render_desktop_locked() {
    unsafe {
        let desktop = desktop_ref();
        crate::arch::gfx_blit_fullscreen(WALLPAPER_PIXELS);
        draw_desktop_backdrop(&desktop);

        if desktop.locked {
            crate::arch::gfx_set_console_visible(false);
            draw_lock_screen(&desktop);
            crate::arch::gfx_present();
            crate::arch::gfx_move_cursor(
                -1,
                -1,
                desktop.mouse_x,
                desktop.mouse_y,
                INK,
                WINDOW_WHITE,
            );
            let desktop = desktop_mut();
            desktop.presented_mouse_x = desktop.mouse_x;
            desktop.presented_mouse_y = desktop.mouse_y;
            desktop.cursor_dirty = false;
            desktop.scene_dirty = false;
            return;
        }

        draw_desktop_icon(&desktop, IconId::Files, ICON_COL_LEFT_X, FILE_ICON_Y);
        draw_desktop_icon(&desktop, IconId::Cmd, ICON_COL_LEFT_X, CMD_ICON_Y);
        draw_desktop_icon(&desktop, IconId::Editor, ICON_COL_LEFT_X, EDIT_ICON_Y);
        draw_desktop_icon(
            &desktop,
            IconId::Settings,
            ICON_COL_RIGHT_X,
            SETTINGS_ICON_Y,
        );
        draw_desktop_icon(&desktop, IconId::Video, ICON_COL_RIGHT_X, VIDEO_ICON_Y);
        draw_desktop_icon(&desktop, IconId::Audio, ICON_COL_RIGHT_X, AUDIO_ICON_Y);

        let cmd_visible = desktop.cmd.open && !desktop.cmd.minimized;
        let files_visible = desktop.files.open && !desktop.files.minimized;
        let editor_visible = desktop.editor.open && !desktop.editor.minimized;
        let settings_visible = desktop.settings.open && !desktop.settings.minimized;
        let video_visible = desktop.video.open && !desktop.video.minimized;
        let audio_visible = desktop.audio.open && !desktop.audio.minimized;

        crate::arch::gfx_set_console_visible(cmd_visible);

        for id in ordered_windows(desktop.active) {
            match id {
                WindowId::Files if files_visible => {
                    draw_files_window(&desktop, desktop.files, desktop.active == WindowId::Files)
                }
                WindowId::Cmd if cmd_visible => {
                    draw_cmd_window(&desktop, desktop.cmd, desktop.active == WindowId::Cmd)
                }
                WindowId::Editor if editor_visible => {
                    draw_editor_window(&desktop, desktop.editor, desktop.active == WindowId::Editor)
                }
                WindowId::Settings if settings_visible => draw_settings_window(
                    &desktop,
                    desktop.settings,
                    desktop.active == WindowId::Settings,
                ),
                WindowId::Video if video_visible => {
                    draw_video_window(&desktop, desktop.video, desktop.active == WindowId::Video)
                }
                WindowId::Audio if audio_visible => {
                    draw_audio_window(&desktop, desktop.audio, desktop.active == WindowId::Audio)
                }
                _ => {}
            }
        }
        draw_taskbar(&desktop);
        if desktop.start_menu_open {
            draw_start_menu(&desktop);
        }
        if context_menu_ref().kind.is_some() {
            draw_context_menu(&desktop);
        }
        crate::arch::gfx_present();
        crate::arch::gfx_move_cursor(-1, -1, desktop.mouse_x, desktop.mouse_y, INK, WINDOW_WHITE);
        let desktop = desktop_mut();
        desktop.presented_mouse_x = desktop.mouse_x;
        desktop.presented_mouse_y = desktop.mouse_y;
        desktop.cursor_dirty = false;
        desktop.scene_dirty = false;
    }
}

unsafe fn handle_mouse_packet(packet: [u8; 3]) {
    (*desktop_ptr()).tick = (*desktop_ptr()).tick.wrapping_add(1);

    if packet[0] & 0xC0 != 0 {
        return;
    }

    let dx = packet[1] as i8 as i32;
    let dy = packet[2] as i8 as i32;
    let left = packet[0] & 0x01 != 0;
    let right = packet[0] & 0x02 != 0;
    let was_left = (*desktop_ptr()).mouse_left;
    let was_right = (*context_menu_ptr()).mouse_right;
    let old_x = (*desktop_ptr()).mouse_x;
    let old_y = (*desktop_ptr()).mouse_y;

    let max_x = crate::arch::screen_width() - 1;
    let max_y = crate::arch::screen_height() - 1;
    (*desktop_ptr()).mouse_x = ((*desktop_ptr()).mouse_x + dx).clamp(0, max_x);
    (*desktop_ptr()).mouse_y = ((*desktop_ptr()).mouse_y - dy).clamp(0, max_y);
    (*desktop_ptr()).hover = hit_test((*desktop_ptr()).mouse_x, (*desktop_ptr()).mouse_y);
    let moved = (*desktop_ptr()).mouse_x != old_x || (*desktop_ptr()).mouse_y != old_y;
    let mut scene_dirty = false;

    if !was_left && left {
        on_mouse_press();
        scene_dirty = true;
    } else if was_left && left {
        on_mouse_hold();
        scene_dirty |= (*desktop_ptr()).dragging.is_some();
    } else if was_left && !left {
        on_mouse_release();
        scene_dirty = true;
    }

    if !was_right && right {
        on_right_mouse_press();
        scene_dirty = true;
    }

    (*desktop_ptr()).mouse_left = left;
    (*context_menu_ptr()).mouse_right = right;
    (*desktop_ptr()).cursor_dirty |= moved;
    (*desktop_ptr()).scene_dirty |= scene_dirty;
}

unsafe fn on_mouse_press() {
    if (*desktop_ptr()).locked {
        unlock_desktop();
        (*desktop_ptr()).pressed = HitTarget::None;
        (*desktop_ptr()).dragging = None;
        return;
    }

    if (*desktop_ptr()).start_menu_open
        && !matches!(
            (*desktop_ptr()).hover,
            HitTarget::StartButton | HitTarget::StartMenuPanel | HitTarget::StartMenuItem(_)
        )
    {
        (*desktop_ptr()).start_menu_open = false;
    }

    if (*context_menu_ptr()).kind.is_some()
        && !matches!(
            (*desktop_ptr()).hover,
            HitTarget::ContextMenuPanel | HitTarget::ContextMenuItem(_)
        )
    {
        (*context_menu_ptr()).kind = None;
    }

    (*desktop_ptr()).pressed = (*desktop_ptr()).hover;

    match (*desktop_ptr()).hover {
        HitTarget::WindowTitle(id) => {
            activate_window(id);
            let window = get_window(id);
            (*desktop_ptr()).dragging = Some(DragState {
                target: id,
                offset_x: (*desktop_ptr()).mouse_x - window.x,
                offset_y: (*desktop_ptr()).mouse_y - window.y,
            });
            (*desktop_ptr()).selected_icon = None;
        }
        HitTarget::WindowBody(id)
        | HitTarget::Minimize(id)
        | HitTarget::Close(id)
        | HitTarget::TaskbarIcon(id) => {
            activate_window(id);
            (*desktop_ptr()).selected_icon = None;
        }
        HitTarget::FilesBack
        | HitTarget::FilesEntry(_)
        | HitTarget::EditorSave
        | HitTarget::SettingsBrightnessDown
        | HitTarget::SettingsBrightnessUp
        | HitTarget::SettingsVolumeDown
        | HitTarget::SettingsVolumeUp
        | HitTarget::SettingsTheme
        | HitTarget::VideoPrev
        | HitTarget::VideoPlay
        | HitTarget::VideoNext
        | HitTarget::AudioPlay => {
            activate_window(match (*desktop_ptr()).hover {
                HitTarget::EditorSave => WindowId::Editor,
                HitTarget::FilesBack | HitTarget::FilesEntry(_) => WindowId::Files,
                HitTarget::SettingsBrightnessDown
                | HitTarget::SettingsBrightnessUp
                | HitTarget::SettingsVolumeDown
                | HitTarget::SettingsVolumeUp
                | HitTarget::SettingsTheme => WindowId::Settings,
                HitTarget::VideoPrev | HitTarget::VideoPlay | HitTarget::VideoNext => {
                    WindowId::Video
                }
                HitTarget::AudioPlay => WindowId::Audio,
                _ => WindowId::Cmd,
            });
            (*desktop_ptr()).selected_icon = None;
        }
        HitTarget::ContextMenuItem(_) | HitTarget::ContextMenuPanel => {}
        HitTarget::ClockChip => {
            (*desktop_ptr()).selected_icon = None;
        }
        HitTarget::StartButton => {}
        HitTarget::StartMenuPanel => {}
        HitTarget::StartMenuItem(icon) => {
            (*desktop_ptr()).selected_icon = Some(icon);
        }
        HitTarget::DesktopIcon(icon) => {
            (*desktop_ptr()).selected_icon = Some(icon);
        }
        HitTarget::None => {
            (*desktop_ptr()).selected_icon = None;
        }
    }
}

unsafe fn on_mouse_hold() {
    let Some(drag) = (*desktop_ptr()).dragging else {
        return;
    };

    let window = get_window_mut(drag.target);
    let max_x = crate::arch::screen_width() - window.w - 12;
    let max_y = taskbar_y() - window.h - 16;
    window.x = ((*desktop_ptr()).mouse_x - drag.offset_x).clamp(16, max_x);
    window.y = ((*desktop_ptr()).mouse_y - drag.offset_y).clamp(20, max_y);
}

unsafe fn on_mouse_release() {
    let released_over = (*desktop_ptr()).hover;
    let pressed = (*desktop_ptr()).pressed;

    if pressed == released_over {
        match released_over {
            HitTarget::StartButton => {
                (*desktop_ptr()).start_menu_open = !(*desktop_ptr()).start_menu_open
            }
            HitTarget::StartMenuItem(icon) => {
                open_window(icon);
                (*desktop_ptr()).start_menu_open = false;
            }
            HitTarget::DesktopIcon(icon) => click_desktop_icon(icon),
            HitTarget::TaskbarIcon(id) => toggle_taskbar_window(id),
            HitTarget::ClockChip => lock_desktop(),
            HitTarget::Minimize(id) => minimize_window(id),
            HitTarget::Close(id) => close_window(id),
            HitTarget::FilesBack => navigate_parent(),
            HitTarget::FilesEntry(index) => click_files_entry(index),
            HitTarget::ContextMenuItem(action) => perform_context_action(action),
            HitTarget::ContextMenuPanel => {}
            HitTarget::EditorSave => save_editor(),
            HitTarget::SettingsBrightnessDown => adjust_setting("brightness", -5),
            HitTarget::SettingsBrightnessUp => adjust_setting("brightness", 5),
            HitTarget::SettingsVolumeDown => adjust_setting("volume", -5),
            HitTarget::SettingsVolumeUp => adjust_setting("volume", 5),
            HitTarget::SettingsTheme => cycle_theme(),
            HitTarget::VideoPrev => step_video_frame(-1),
            HitTarget::VideoPlay => play_video_window(),
            HitTarget::VideoNext => step_video_frame(1),
            HitTarget::AudioPlay => play_audio_window(),
            HitTarget::WindowBody(id) | HitTarget::WindowTitle(id) => activate_window(id),
            HitTarget::StartMenuPanel => {}
            HitTarget::None => {}
        }
    }

    (*desktop_ptr()).dragging = None;
    (*desktop_ptr()).pressed = HitTarget::None;
}

unsafe fn on_right_mouse_press() {
    if (*desktop_ptr()).locked {
        unlock_desktop();
        return;
    }

    let kind = match (*desktop_ptr()).hover {
        HitTarget::FilesEntry(_) | HitTarget::WindowBody(WindowId::Files) => {
            Some(ContextMenuKind::Files)
        }
        HitTarget::None | HitTarget::DesktopIcon(_) => Some(ContextMenuKind::Desktop),
        _ => None,
    };

    (*desktop_ptr()).start_menu_open = false;
    (*desktop_ptr()).pressed = HitTarget::None;
    (*desktop_ptr()).dragging = None;
    (*context_menu_ptr()).kind = kind;
    if let Some(kind) = kind {
        let height = context_menu_height(kind);
        (*context_menu_ptr()).x = (*desktop_ptr())
            .mouse_x
            .clamp(4, crate::arch::screen_width() - CONTEXT_MENU_WIDTH - 4);
        (*context_menu_ptr()).y = (*desktop_ptr())
            .mouse_y
            .clamp(4, crate::arch::screen_height() - height - 4);
        (*desktop_ptr()).hover = hit_test((*desktop_ptr()).mouse_x, (*desktop_ptr()).mouse_y);
    }
}

unsafe fn lock_desktop() {
    (*desktop_ptr()).locked = true;
    (*desktop_ptr()).start_menu_open = false;
    (*context_menu_ptr()).kind = None;
    (*desktop_ptr()).dragging = None;
    (*desktop_ptr()).pressed = HitTarget::None;
    (*desktop_ptr()).hover = HitTarget::None;
}

unsafe fn unlock_desktop() {
    (*desktop_ptr()).locked = false;
    (*desktop_ptr()).hover = hit_test((*desktop_ptr()).mouse_x, (*desktop_ptr()).mouse_y);
}

unsafe fn click_desktop_icon(icon: IconId) {
    let tick = (*desktop_ptr()).tick;
    let double_click = match (*desktop_ptr()).last_icon_click {
        Some((last_icon, last_tick)) => {
            last_icon == icon && tick.wrapping_sub(last_tick) <= DOUBLE_CLICK_TICKS
        }
        None => false,
    };

    (*desktop_ptr()).selected_icon = Some(icon);
    (*desktop_ptr()).last_icon_click = Some((icon, tick));

    if double_click {
        open_window(icon);
    }
}

unsafe fn click_files_entry(index: usize) {
    if index >= (*desktop_ptr()).files_len {
        return;
    }

    let tick = (*desktop_ptr()).tick;
    let double_click = match (*desktop_ptr()).last_file_click {
        Some((last_index, last_tick)) => {
            last_index == index && tick.wrapping_sub(last_tick) <= DOUBLE_CLICK_TICKS
        }
        None => false,
    };

    (*desktop_ptr()).selected_file_entry = Some(index);
    (*desktop_ptr()).last_file_click = Some((index, tick));

    if double_click {
        open_files_entry(index);
    } else {
        let entry = (*desktop_ptr()).files_entries[index];
        desktop_mut().file_status.set(entry.name.as_str());
    }
}

unsafe fn open_files_entry(index: usize) {
    if index >= (*desktop_ptr()).files_len {
        return;
    }

    let entry = (*desktop_ptr()).files_entries[index];
    let mut path = InlineString::<64>::new();
    let desktop = desktop_ref();
    set_child_path(&mut path, desktop.files_path.as_str(), entry.name.as_str());

    match entry.kind {
        BrowserKind::Directory => navigate_to(path.as_str()),
        BrowserKind::File => {
            let _ = open_editor_internal(path.as_str());
        }
    }
}

unsafe fn perform_context_action(action: ContextAction) {
    (*context_menu_ptr()).kind = None;
    match action {
        ContextAction::DesktopFiles => open_window(IconId::Files),
        ContextAction::DesktopCommand => open_window(IconId::Cmd),
        ContextAction::DesktopSettings => open_window(IconId::Settings),
        ContextAction::FilesNewDirectory => create_files_entry(BrowserKind::Directory),
        ContextAction::FilesNewFile => create_files_entry(BrowserKind::File),
    }
}

unsafe fn create_files_entry(kind: BrowserKind) {
    let mut parent = InlineString::<64>::new();
    parent.set(desktop_ref().files_path.as_str());
    let vfs = VfsService::new();

    for suffix in 1u8..=99 {
        let mut name = InlineString::<32>::new();
        let _ = name.push_str(match kind {
            BrowserKind::Directory => "NewFolder",
            BrowserKind::File => "NewFile",
        });
        if suffix > 1 {
            if suffix >= 10 {
                let _ = name.push_byte(b'0' + suffix / 10);
            }
            let _ = name.push_byte(b'0' + suffix % 10);
        }
        if kind == BrowserKind::File {
            let _ = name.push_str(".txt");
        }

        let mut path = InlineString::<64>::new();
        set_child_path(&mut path, parent.as_str(), name.as_str());
        if vfs.path_exists(path.as_str()) {
            continue;
        }

        let result = match kind {
            BrowserKind::Directory => vfs.create_dir(path.as_str()),
            BrowserKind::File => vfs.create_file(path.as_str()),
        };
        match result {
            Ok(()) => {
                refresh_files_view();
                desktop_mut().file_status.set(path.as_str());
            }
            Err(err) => desktop_mut().file_status.set(err.message()),
        }
        return;
    }

    desktop_mut().file_status.set("No available name");
}

unsafe fn open_window(icon: IconId) {
    match icon {
        IconId::Files => {
            (*desktop_ptr()).files.open = true;
            (*desktop_ptr()).files.minimized = false;
            (*desktop_ptr()).active = WindowId::Files;
            refresh_files_view();
        }
        IconId::Cmd => {
            (*desktop_ptr()).cmd.open = true;
            (*desktop_ptr()).cmd.minimized = false;
            (*desktop_ptr()).active = WindowId::Cmd;
        }
        IconId::Editor => {
            let _ = open_editor_internal("/home/notes.txt");
        }
        IconId::Settings => {
            (*desktop_ptr()).settings.open = true;
            (*desktop_ptr()).settings.minimized = false;
            (*desktop_ptr()).active = WindowId::Settings;
            desktop_mut().settings_status.set("Editing settings");
        }
        IconId::Video => {
            (*desktop_ptr()).video.open = true;
            (*desktop_ptr()).video.minimized = false;
            (*desktop_ptr()).active = WindowId::Video;
            desktop_mut().video_status.set("Ready");
        }
        IconId::Audio => {
            (*desktop_ptr()).audio.open = true;
            (*desktop_ptr()).audio.minimized = false;
            (*desktop_ptr()).active = WindowId::Audio;
            desktop_mut().audio_status.set("Ready");
        }
    }
}

unsafe fn toggle_taskbar_window(id: WindowId) {
    let window = get_window_mut(id);
    if !window.open {
        window.open = true;
        window.minimized = false;
        (*desktop_ptr()).active = id;
        return;
    }

    if window.minimized {
        window.minimized = false;
        (*desktop_ptr()).active = id;
        return;
    }

    if (*desktop_ptr()).active == id {
        window.minimized = true;
        (*desktop_ptr()).active = fallback_active(id);
    } else {
        (*desktop_ptr()).active = id;
    }
}

unsafe fn minimize_window(id: WindowId) {
    let window = get_window_mut(id);
    window.minimized = true;
    (*desktop_ptr()).active = fallback_active(id);
}

unsafe fn close_window(id: WindowId) {
    let window = get_window_mut(id);
    window.open = false;
    window.minimized = false;
    (*desktop_ptr()).active = fallback_active(id);
}

unsafe fn fallback_active(closed: WindowId) -> WindowId {
    for id in [
        WindowId::Audio,
        WindowId::Video,
        WindowId::Settings,
        WindowId::Editor,
        WindowId::Files,
        WindowId::Cmd,
    ] {
        if id == closed {
            continue;
        }
        let window = get_window(id);
        if window.open && !window.minimized {
            return id;
        }
    }
    closed
}

unsafe fn activate_window(id: WindowId) {
    let window = get_window_mut(id);
    if window.open && !window.minimized {
        (*desktop_ptr()).active = id;
    }
}

unsafe fn get_window(id: WindowId) -> WindowState {
    match id {
        WindowId::Files => (*desktop_ptr()).files,
        WindowId::Cmd => (*desktop_ptr()).cmd,
        WindowId::Editor => (*desktop_ptr()).editor,
        WindowId::Settings => (*desktop_ptr()).settings,
        WindowId::Video => (*desktop_ptr()).video,
        WindowId::Audio => (*desktop_ptr()).audio,
    }
}

unsafe fn get_window_mut(id: WindowId) -> &'static mut WindowState {
    match id {
        WindowId::Files => core::ptr::addr_of_mut!((*desktop_ptr()).files)
            .as_mut()
            .unwrap(),
        WindowId::Cmd => core::ptr::addr_of_mut!((*desktop_ptr()).cmd)
            .as_mut()
            .unwrap(),
        WindowId::Editor => core::ptr::addr_of_mut!((*desktop_ptr()).editor)
            .as_mut()
            .unwrap(),
        WindowId::Settings => core::ptr::addr_of_mut!((*desktop_ptr()).settings)
            .as_mut()
            .unwrap(),
        WindowId::Video => core::ptr::addr_of_mut!((*desktop_ptr()).video)
            .as_mut()
            .unwrap(),
        WindowId::Audio => core::ptr::addr_of_mut!((*desktop_ptr()).audio)
            .as_mut()
            .unwrap(),
    }
}

unsafe fn hit_test(x: i32, y: i32) -> HitTarget {
    if (*desktop_ptr()).locked {
        return HitTarget::None;
    }

    if let Some(kind) = (*context_menu_ptr()).kind {
        let menu_x = (*context_menu_ptr()).x;
        let menu_y = (*context_menu_ptr()).y;
        let height = context_menu_height(kind);
        if point_in_rect(x, y, menu_x, menu_y, CONTEXT_MENU_WIDTH, height) {
            let row = (y - menu_y - CONTEXT_MENU_PADDING) / CONTEXT_MENU_ROW_HEIGHT;
            if y >= menu_y + CONTEXT_MENU_PADDING {
                if let Some(action) = context_menu_action(kind, row as usize) {
                    return HitTarget::ContextMenuItem(action);
                }
            }
            return HitTarget::ContextMenuPanel;
        }
    }

    if (*desktop_ptr()).start_menu_open
        && point_in_rect(
            x,
            y,
            start_menu_x(),
            start_menu_y(),
            start_menu_w(),
            start_menu_h(),
        )
    {
        for (index, icon) in start_menu_items().iter().enumerate() {
            let row_y = start_menu_y() + 16 + index as i32 * 28;
            if point_in_rect(x, y, start_menu_x() + 10, row_y, start_menu_w() - 20, 22) {
                return HitTarget::StartMenuItem(*icon);
            }
        }
        return HitTarget::StartMenuPanel;
    }

    if point_in_rect(x, y, start_button_x(), taskbar_y() + 7, 48, TASKBAR_SLOT_H) {
        return HitTarget::StartButton;
    }
    if point_in_rect(
        x,
        y,
        file_dock_x(),
        taskbar_y() + 7,
        TASKBAR_SLOT_W,
        TASKBAR_SLOT_H,
    ) {
        return HitTarget::TaskbarIcon(WindowId::Files);
    }
    if point_in_rect(
        x,
        y,
        cmd_dock_x(),
        taskbar_y() + 7,
        TASKBAR_SLOT_W,
        TASKBAR_SLOT_H,
    ) {
        return HitTarget::TaskbarIcon(WindowId::Cmd);
    }
    if point_in_rect(
        x,
        y,
        editor_dock_x(),
        taskbar_y() + 7,
        TASKBAR_SLOT_W,
        TASKBAR_SLOT_H,
    ) {
        return HitTarget::TaskbarIcon(WindowId::Editor);
    }
    if point_in_rect(
        x,
        y,
        clock_chip_x(),
        clock_chip_y(),
        clock_chip_w(),
        clock_chip_h(),
    ) {
        return HitTarget::ClockChip;
    }

    for window_id in topmost_windows() {
        let window = get_window(window_id);
        if !window.open || window.minimized {
            continue;
        }

        if point_in_rect(x, y, close_button_x(window), window.y + 5, 14, 14) {
            return HitTarget::Close(window_id);
        }
        if point_in_rect(x, y, min_button_x(window), window.y + 5, 14, 14) {
            return HitTarget::Minimize(window_id);
        }

        if window_id == WindowId::Files {
            if point_in_rect(x, y, files_back_x(window), files_back_y(window), 30, 18) {
                return HitTarget::FilesBack;
            }

            let origin_x = files_list_x(window);
            let origin_y = files_list_y(window);
            for index in 0..(*desktop_ptr()).files_len {
                let row_y = origin_y + index as i32 * FILE_ROW_HEIGHT;
                if point_in_rect(
                    x,
                    y,
                    origin_x,
                    row_y,
                    files_list_w(window),
                    FILE_ROW_HEIGHT - 2,
                ) {
                    return HitTarget::FilesEntry(index);
                }
            }
        }

        if window_id == WindowId::Editor
            && point_in_rect(x, y, editor_save_x(window), editor_save_y(window), 44, 16)
        {
            return HitTarget::EditorSave;
        }

        if window_id == WindowId::Settings {
            if point_in_rect(
                x,
                y,
                settings_minus_x(window),
                settings_row1_y(window),
                18,
                18,
            ) {
                return HitTarget::SettingsBrightnessDown;
            }
            if point_in_rect(
                x,
                y,
                settings_plus_x(window),
                settings_row1_y(window),
                18,
                18,
            ) {
                return HitTarget::SettingsBrightnessUp;
            }
            if point_in_rect(
                x,
                y,
                settings_minus_x(window),
                settings_row2_y(window),
                18,
                18,
            ) {
                return HitTarget::SettingsVolumeDown;
            }
            if point_in_rect(
                x,
                y,
                settings_plus_x(window),
                settings_row2_y(window),
                18,
                18,
            ) {
                return HitTarget::SettingsVolumeUp;
            }
            if point_in_rect(
                x,
                y,
                settings_theme_x(window),
                settings_theme_y(window),
                88,
                18,
            ) {
                return HitTarget::SettingsTheme;
            }
        }

        if window_id == WindowId::Video {
            if point_in_rect(x, y, video_prev_x(window), video_controls_y(window), 30, 18) {
                return HitTarget::VideoPrev;
            }
            if point_in_rect(x, y, video_play_x(window), video_controls_y(window), 48, 18) {
                return HitTarget::VideoPlay;
            }
            if point_in_rect(x, y, video_next_x(window), video_controls_y(window), 30, 18) {
                return HitTarget::VideoNext;
            }
        }

        if window_id == WindowId::Audio
            && point_in_rect(x, y, audio_play_x(window), audio_controls_y(window), 54, 18)
        {
            return HitTarget::AudioPlay;
        }

        if point_in_rect(x, y, window.x, window.y, window.w, TITLE_BAR_HEIGHT) {
            return HitTarget::WindowTitle(window_id);
        }
        if point_in_rect(x, y, window.x, window.y, window.w, window.h) {
            return HitTarget::WindowBody(window_id);
        }
    }

    if point_in_rect(x, y, ICON_COL_LEFT_X, FILE_ICON_Y, 54, 64) {
        return HitTarget::DesktopIcon(IconId::Files);
    }
    if point_in_rect(x, y, ICON_COL_LEFT_X, CMD_ICON_Y, 54, 64) {
        return HitTarget::DesktopIcon(IconId::Cmd);
    }
    if point_in_rect(x, y, ICON_COL_LEFT_X, EDIT_ICON_Y, 54, 64) {
        return HitTarget::DesktopIcon(IconId::Editor);
    }
    if point_in_rect(x, y, ICON_COL_RIGHT_X, SETTINGS_ICON_Y, 54, 64) {
        return HitTarget::DesktopIcon(IconId::Settings);
    }
    if point_in_rect(x, y, ICON_COL_RIGHT_X, VIDEO_ICON_Y, 54, 64) {
        return HitTarget::DesktopIcon(IconId::Video);
    }
    if point_in_rect(x, y, ICON_COL_RIGHT_X, AUDIO_ICON_Y, 54, 64) {
        return HitTarget::DesktopIcon(IconId::Audio);
    }

    HitTarget::None
}

fn ordered_windows(active: WindowId) -> [WindowId; 6] {
    let mut order = all_windows();
    let mut write = 0usize;
    for window in all_windows() {
        if window != active {
            order[write] = window;
            write += 1;
        }
    }
    order[5] = active;
    order
}

unsafe fn topmost_windows() -> [WindowId; 6] {
    let draw_order = ordered_windows((*desktop_ptr()).active);
    [
        draw_order[5],
        draw_order[4],
        draw_order[3],
        draw_order[2],
        draw_order[1],
        draw_order[0],
    ]
}

unsafe fn refresh_files_view() {
    let desktop = desktop_mut();
    desktop.files_len = 0;
    desktop.file_status.set(desktop.files_path.as_str());

    let vfs = VfsService::new();
    let mut entries = DirEntries::new();
    if vfs
        .list_dir(desktop.files_path.as_str(), &mut entries)
        .is_err()
    {
        desktop.file_status.set("Folder unavailable");
        desktop.selected_file_entry = None;
        return;
    }

    for (index, name) in entries.iter().take(MAX_BROWSER_ENTRIES).enumerate() {
        let mut full_path = InlineString::<64>::new();
        set_child_path(&mut full_path, desktop.files_path.as_str(), name);

        desktop.files_entries[index].name.set(name);
        desktop.files_entries[index].kind = if vfs.is_directory(full_path.as_str()) {
            BrowserKind::Directory
        } else {
            BrowserKind::File
        };
        desktop.files_len += 1;
    }

    if let Some(selected) = desktop.selected_file_entry {
        if selected >= desktop.files_len {
            desktop.selected_file_entry = None;
        }
    }
}

unsafe fn navigate_to(path: &str) {
    let desktop = desktop_mut();
    desktop.files_path.set(path);
    desktop.selected_file_entry = None;
    desktop.last_file_click = None;
    refresh_files_view();
}

unsafe fn navigate_parent() {
    let current = desktop_ref().files_path.as_str();
    if current == "/" {
        return;
    }

    let mut parent = InlineString::<64>::new();
    if let Some(index) = current.rfind('/') {
        if index == 0 {
            parent.set("/");
        } else {
            parent.set(&current[..index]);
        }
    } else {
        parent.set("/");
    }
    navigate_to(parent.as_str());
}

unsafe fn open_editor_internal(path: &str) -> bool {
    let vfs = VfsService::new();
    if vfs.is_directory(path) {
        return false;
    }
    if !vfs.path_exists(path) && vfs.create_file(path).is_err() {
        return false;
    }

    let mut scratch = InlineString::<EDITOR_TEXT_CAPACITY>::new();
    let Some(text) = vfs.read_file(path, &mut scratch) else {
        return false;
    };

    let desktop = desktop_mut();
    desktop.editor_path.set(path);
    desktop.editor_text.clear();
    let _ = desktop.editor_text.push_str(text);
    desktop.editor_status.set("Editing - F2 saves");
    desktop.editor.open = true;
    desktop.editor.minimized = false;
    desktop.active = WindowId::Editor;
    true
}

unsafe fn save_editor() {
    let vfs = VfsService::new();
    let desktop = desktop_mut();
    match vfs.write_text(desktop.editor_path.as_str(), desktop.editor_text.as_str()) {
        Ok(()) => {
            desktop.editor_status.set("Saved");
            refresh_files_view();
        }
        Err(err) => desktop.editor_status.set(err.message()),
    }
}

unsafe fn adjust_setting(key: &str, delta: i32) {
    let vfs = VfsService::new();
    let mut config = load_settings_config();
    match key {
        "brightness" => config.brightness = clamp_percent(config.brightness as i32 + delta),
        "volume" => config.volume = clamp_percent(config.volume as i32 + delta),
        _ => return,
    }

    if write_settings_config(&vfs, config).is_ok() {
        let desktop = desktop_mut();
        desktop.settings_status.set("Saved");
        desktop.audio_status.set("Settings updated");
    } else {
        desktop_mut().settings_status.set("Save failed");
    }
}

unsafe fn cycle_theme() {
    let vfs = VfsService::new();
    let mut config = load_settings_config();
    config.theme = match config.theme {
        Theme::Graphite => Theme::Sunrise,
        Theme::Sunrise => Theme::Glacier,
        Theme::Glacier => Theme::Graphite,
    };
    if write_settings_config(&vfs, config).is_ok() {
        desktop_mut().settings_status.set("Theme saved");
    } else {
        desktop_mut().settings_status.set("Save failed");
    }
}

unsafe fn step_video_frame(delta: isize) {
    let count = count_video_frames(desktop_ref().video_path.as_str());
    if count == 0 {
        desktop_mut().video_status.set("No frames");
        return;
    }

    let desktop = desktop_mut();
    let current = desktop.video_frame.min(count - 1);
    let next = if delta.is_negative() {
        current.saturating_sub(delta.unsigned_abs())
    } else {
        current.saturating_add(delta as usize).min(count - 1)
    };
    desktop.video_frame = next;
    desktop.video_status.set("Frame selected");
}

unsafe fn play_video_window() {
    let count = count_video_frames(desktop_ref().video_path.as_str());
    if count == 0 {
        desktop_mut().video_status.set("No frames");
        return;
    }

    for frame in 0..count {
        {
            let desktop = desktop_mut();
            desktop.video_frame = frame;
            desktop.video_status.set("Playing");
        }
        render_desktop_locked();
        crate::arch::delay(25_000_000);
    }
    desktop_mut().video_status.set("Playback complete");
}

unsafe fn play_audio_window() {
    let mut scratch = InlineString::<2048>::new();
    let vfs = VfsService::new();
    let Some(text) = vfs.read_file(desktop_ref().audio_path.as_str(), &mut scratch) else {
        desktop_mut().audio_status.set("Audio file missing");
        return;
    };
    let (_, _, body) = parse_media_header(text);

    let mut played = 0usize;
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let Some(freq_text) = parts.next() else {
            continue;
        };
        let Some(duration_text) = parts.next() else {
            continue;
        };
        let label = parts.next().unwrap_or("NOTE");
        let Ok(freq) = parse_u32(freq_text) else {
            continue;
        };
        let Ok(duration) = parse_u32(duration_text) else {
            continue;
        };

        {
            let mut status = InlineString::<64>::new();
            let _ = core::fmt::write(&mut status, format_args!("Playing {}", label));
            desktop_mut().audio_status.set(status.as_str());
        }
        render_desktop_locked();

        if freq == 0 {
            crate::arch::speaker_stop();
        } else {
            crate::arch::speaker_play(freq);
        }
        crate::arch::delay(duration.saturating_mul(25_000));
        crate::arch::speaker_stop();
        crate::arch::delay(750_000);
        played += 1;
    }

    if played == 0 {
        desktop_mut().audio_status.set("No notes");
    } else {
        desktop_mut().audio_status.set("Playback complete");
    }
}

fn set_child_path(out: &mut InlineString<64>, parent: &str, child: &str) {
    out.clear();
    if parent == "/" {
        let _ = out.push_str("/");
        let _ = out.push_str(child);
        return;
    }

    let _ = out.push_str(parent);
    let _ = out.push_str("/");
    let _ = out.push_str(child);
}

fn configure_palette() {
    for (index, &(r, g, b)) in WALLPAPER_COLORS.iter().enumerate() {
        crate::arch::set_palette_entry(index as u8, r, g, b);
    }

    crate::arch::set_palette_entry(WINDOW_WHITE, 63, 63, 63);
    crate::arch::set_palette_entry(TITLE_BLUE, 10, 42, 58);
    crate::arch::set_palette_entry(INK, 0, 0, 0);
    crate::arch::set_palette_entry(MINIMIZE_YELLOW, 63, 58, 0);
    crate::arch::set_palette_entry(ORANGE, 63, 32, 0);
    crate::arch::set_palette_entry(CLOSE_RED, 63, 10, 10);
    crate::arch::set_palette_entry(TASKBAR_CYAN, 42, 58, 63);
    crate::arch::set_palette_entry(SOFT_GRAY, 49, 51, 55);
    crate::arch::set_palette_entry(SELECTION, 48, 54, 62);
    crate::arch::set_palette_entry(TERMINAL_BLACK, 2, 2, 2);
    crate::arch::set_palette_entry(MUTED_TEXT, 24, 28, 35);
    crate::arch::set_palette_entry(EDITOR_PAPER, 60, 61, 62);
    crate::arch::set_palette_entry(START_GREEN, 14, 42, 18);
    crate::arch::set_palette_entry(VIDEO_PANEL, 10, 18, 35);
    crate::arch::set_palette_entry(AUDIO_PANEL, 32, 14, 24);
    crate::arch::set_palette_entry(SETTINGS_PANEL, 42, 34, 18);
    crate::arch::set_palette_entry(GLASS_TINT, 41, 50, 57);
    crate::arch::set_palette_entry(GLASS_HIGHLIGHT, 60, 62, 63);
    crate::arch::set_palette_entry(GLASS_EDGE, 30, 41, 48);
    crate::arch::set_palette_entry(GLASS_SHADOW, 6, 10, 18);
    crate::arch::set_palette_entry(PANEL_PAPER, 57, 58, 60);
    crate::arch::set_palette_entry(PANEL_INSET, 51, 54, 57);
    crate::arch::set_palette_entry(TITLE_MIST, 20, 34, 40);
    crate::arch::set_palette_entry(TASKBAR_DEEP, 18, 28, 36);
    crate::arch::set_palette_entry(ACTIVE_GLOW, 36, 57, 63);
    crate::arch::set_palette_entry(COPPER_ACCENT, 53, 31, 18);
    crate::arch::set_palette_entry(LOCK_VEIL, 12, 18, 24);
    crate::arch::set_palette_entry(AURORA_BLUE, 26, 48, 63);
    crate::arch::set_palette_entry(AURORA_GOLD, 63, 43, 18);
    crate::arch::set_palette_entry(AURORA_MINT, 26, 63, 52);
    crate::arch::set_palette_entry(NIGHT_INK, 8, 12, 18);
}

fn draw_desktop_backdrop(desktop: &DesktopState) {
    let screen_w = crate::arch::screen_width();
    let screen_h = crate::arch::screen_height();
    let phase = desktop.ambient_phase as i32;

    crate::arch::gfx_fill_rect_alpha(0, 0, screen_w, screen_h, NIGHT_INK, 34);
    crate::arch::gfx_fill_circle_alpha(
        screen_w - 100 - (phase % 18),
        84 + ((phase / 3) % 6),
        112,
        AURORA_BLUE,
        26,
    );
    crate::arch::gfx_fill_circle_alpha(
        116 + ((phase / 2) % 16),
        screen_h - 92,
        126,
        AURORA_GOLD,
        22,
    );
    crate::arch::gfx_fill_circle_alpha(
        screen_w / 2 + ((phase % 20) - 10),
        screen_h / 2 + 70,
        96,
        AURORA_MINT,
        20,
    );
    crate::arch::gfx_fill_rect_alpha(0, 0, screen_w, 92, GLASS_HIGHLIGHT, 36);

    let hero_w = 292;
    let hero_x = (screen_w - hero_w) / 2;
    draw_shadow_rounded_panel(hero_x, 18, hero_w, 52, 14, GLASS_TINT, GLASS_EDGE);
    crate::arch::gfx_fill_round_rect_alpha(hero_x + 1, 19, hero_w - 2, 20, 13, GLASS_HIGHLIGHT, 34);
    crate::arch::draw_text(hero_x + 18, 32, "RUSTIX", WINDOW_WHITE, 2);
    crate::arch::draw_text(hero_x + 92, 34, "SECURE MICROKERNEL", INK, 1);
    crate::arch::draw_text(
        hero_x + 92,
        45,
        "GLASS SHELL  /  F12 LOCK",
        GLASS_HIGHLIGHT,
        1,
    );
}

fn draw_lock_screen(desktop: &DesktopState) {
    let screen_w = crate::arch::screen_width();
    let screen_h = crate::arch::screen_height();

    crate::arch::gfx_fill_rect_alpha(0, 0, screen_w, screen_h, LOCK_VEIL, 112);
    crate::arch::gfx_fill_rect_alpha(0, 0, screen_w, screen_h, NIGHT_INK, 72);
    crate::arch::gfx_fill_circle_alpha(screen_w - 124, 96, 104, AURORA_BLUE, 22);
    crate::arch::gfx_fill_circle_alpha(110, screen_h - 86, 120, AURORA_GOLD, 18);

    let panel_w = 276;
    let panel_h = 148;
    let panel_x = (screen_w - panel_w) / 2;
    let panel_y = (screen_h - panel_h) / 2 - 12;
    draw_shadow_rounded_panel(
        panel_x, panel_y, panel_w, panel_h, 18, GLASS_TINT, GLASS_EDGE,
    );
    crate::arch::gfx_fill_round_rect_alpha(
        panel_x + 1,
        panel_y + 1,
        panel_w - 2,
        48,
        17,
        GLASS_HIGHLIGHT,
        38,
    );

    let mut hm = InlineString::<16>::new();
    let mut hms = InlineString::<16>::new();
    write_clock_hm(&mut hm, desktop.clock_time);
    write_clock_hms(&mut hms, desktop.clock_time);

    let hm_x = panel_x + (panel_w - crate::arch::text_width(hm.as_str(), 4)) / 2;
    crate::arch::draw_text(hm_x, panel_y + 38, hm.as_str(), WINDOW_WHITE, 4);
    let hms_x = panel_x + (panel_w - crate::arch::text_width(hms.as_str(), 1)) / 2;
    crate::arch::draw_text(hms_x, panel_y + 90, hms.as_str(), GLASS_HIGHLIGHT, 1);

    let hint = "PRESS ANY KEY OR CLICK";
    let hint_x = panel_x + (panel_w - crate::arch::text_width(hint, 1)) / 2;
    crate::arch::draw_text(hint_x, panel_y + 112, hint, WINDOW_WHITE, 1);
    let footer = "RUSTIX SECURE SESSION";
    let footer_x = panel_x + (panel_w - crate::arch::text_width(footer, 1)) / 2;
    crate::arch::draw_text(footer_x, panel_y + 126, footer, MUTED_TEXT, 1);
}

fn draw_desktop_icon(desktop: &DesktopState, icon: IconId, x: i32, y: i32) {
    let hovered = desktop.hover == HitTarget::DesktopIcon(icon);
    let pressed = desktop.pressed == HitTarget::DesktopIcon(icon);
    let selected = desktop.selected_icon == Some(icon);

    let shell = if pressed {
        SOFT_GRAY
    } else if hovered || selected {
        SELECTION
    } else {
        PANEL_INSET
    };

    draw_shadow_rounded_panel(x, y, 54, 48, 8, shell, GLASS_EDGE);

    match icon {
        IconId::Files => draw_folder_symbol(x + 12, y + 12),
        IconId::Cmd => draw_terminal_symbol(x + 12, y + 11),
        IconId::Editor => draw_editor_symbol(x + 12, y + 11),
        IconId::Settings => draw_settings_symbol(x + 12, y + 11),
        IconId::Video => draw_video_symbol(x + 12, y + 11),
        IconId::Audio => draw_audio_symbol(x + 12, y + 11),
    }

    let label = match icon {
        IconId::Files => "文件",
        IconId::Cmd => "命令",
        IconId::Editor => "编辑",
        IconId::Settings => "设置",
        IconId::Video => "视频",
        IconId::Audio => "音频",
    };
    let label_x = x + (54 - crate::arch::text_width(label, 1)) / 2;
    crate::arch::draw_text(label_x, y + 58, label, INK, 1);
    crate::arch::draw_text(label_x, y + 57, label, WINDOW_WHITE, 1);
}

fn draw_taskbar(desktop: &DesktopState) {
    draw_glass_taskbar(taskbar_x(), taskbar_y(), TASKBAR_W, TASKBAR_H, 14);
    draw_translucent_separator(taskbar_x() + 56, taskbar_y() + 7, TASKBAR_H - 14);
    draw_translucent_separator(taskbar_x() + 220, taskbar_y() + 7, TASKBAR_H - 14);
    draw_translucent_separator(taskbar_x() + 386, taskbar_y() + 7, TASKBAR_H - 14);

    draw_start_button(desktop);
    draw_taskbar_slot(desktop, file_dock_x(), WindowId::Files, IconId::Files);
    draw_taskbar_slot(desktop, cmd_dock_x(), WindowId::Cmd, IconId::Cmd);
    draw_taskbar_slot(desktop, editor_dock_x(), WindowId::Editor, IconId::Editor);
    draw_glass_chip(
        taskbar_x() + 244,
        taskbar_y() + 7,
        122,
        TASKBAR_SLOT_H,
        9,
        GLASS_TINT,
        false,
    );
    crate::arch::draw_text(taskbar_x() + 258, taskbar_y() + 13, "RUSTIX", INK, 1);
    crate::arch::draw_text(
        taskbar_x() + 258,
        taskbar_y() + 12,
        "RUSTIX",
        GLASS_HIGHLIGHT,
        1,
    );
    crate::arch::draw_text(
        taskbar_x() + 258,
        taskbar_y() + 23,
        "secure desk",
        WINDOW_WHITE,
        1,
    );
    draw_taskbar_clock(desktop);
}

fn draw_taskbar_clock(desktop: &DesktopState) {
    let hovered = desktop.hover == HitTarget::ClockChip;
    let pressed = desktop.pressed == HitTarget::ClockChip;
    let fill = if pressed {
        SOFT_GRAY
    } else if hovered || desktop.locked {
        ACTIVE_GLOW
    } else {
        PANEL_PAPER
    };

    draw_glass_chip(
        clock_chip_x(),
        clock_chip_y(),
        clock_chip_w(),
        clock_chip_h(),
        9,
        fill,
        desktop.locked,
    );

    let mut hm = InlineString::<16>::new();
    let mut seconds = InlineString::<8>::new();
    write_clock_hm(&mut hm, desktop.clock_time);
    write_clock_seconds(&mut seconds, desktop.clock_time);

    crate::arch::draw_text(
        clock_chip_x() + 10,
        clock_chip_y() + 7,
        "LOCK",
        GLASS_HIGHLIGHT,
        1,
    );
    crate::arch::draw_text(clock_chip_x() + 42, clock_chip_y() + 9, hm.as_str(), INK, 2);
    crate::arch::draw_text(
        clock_chip_x() + 92,
        clock_chip_y() + 18,
        seconds.as_str(),
        WINDOW_WHITE,
        1,
    );
}

fn draw_taskbar_slot(desktop: &DesktopState, x: i32, id: WindowId, icon: IconId) {
    let hovered = desktop.hover == HitTarget::TaskbarIcon(id);
    let pressed = desktop.pressed == HitTarget::TaskbarIcon(id);
    let window = snapshot_window(desktop, id);
    let active = desktop.active == id && window.open && !window.minimized;
    let open = window.open;

    let fill = if pressed {
        SOFT_GRAY
    } else if active {
        ACTIVE_GLOW
    } else if hovered {
        GLASS_HIGHLIGHT
    } else {
        GLASS_TINT
    };

    draw_glass_chip(
        x,
        taskbar_y() + 7,
        TASKBAR_SLOT_W,
        TASKBAR_SLOT_H,
        8,
        fill,
        active,
    );
    match icon {
        IconId::Files => draw_folder_symbol(x + 9, taskbar_y() + 13),
        IconId::Cmd => draw_terminal_symbol(x + 9, taskbar_y() + 12),
        IconId::Editor => draw_editor_symbol(x + 9, taskbar_y() + 12),
        IconId::Settings => draw_settings_symbol(x + 9, taskbar_y() + 12),
        IconId::Video => draw_video_symbol(x + 9, taskbar_y() + 12),
        IconId::Audio => draw_audio_symbol(x + 9, taskbar_y() + 12),
    }
    if open {
        crate::arch::gfx_fill_rect(x + 7, taskbar_y() + 32, 22, 1, GLASS_HIGHLIGHT);
        crate::arch::gfx_fill_rect(x + 10, taskbar_y() + 34, 16, 2, COPPER_ACCENT);
    }
}

fn draw_start_button(desktop: &DesktopState) {
    let hovered = desktop.hover == HitTarget::StartButton;
    let pressed = desktop.pressed == HitTarget::StartButton;
    let fill = if pressed {
        TITLE_MIST
    } else if hovered || desktop.start_menu_open {
        GLASS_HIGHLIGHT
    } else {
        START_GREEN
    };

    draw_glass_chip(
        start_button_x(),
        taskbar_y() + 7,
        48,
        TASKBAR_SLOT_H,
        8,
        fill,
        desktop.start_menu_open,
    );
    let label = "开始";
    let label_x = start_button_x() + (48 - crate::arch::text_width(label, 1)) / 2;
    crate::arch::draw_text(label_x, taskbar_y() + 14, label, INK, 1);
}

fn draw_start_menu(desktop: &DesktopState) {
    draw_glass_sheet_panel(
        start_menu_x(),
        start_menu_y(),
        start_menu_w(),
        start_menu_h(),
        12,
        GLASS_TINT,
        164,
        96,
    );
    crate::arch::gfx_fill_round_rect_alpha(
        start_menu_x() + 1,
        start_menu_y() + 1,
        34,
        start_menu_h() - 2,
        11,
        ACTIVE_GLOW,
        68,
    );
    crate::arch::gfx_fill_round_rect_alpha(
        start_menu_x() + 1,
        start_menu_y() + 1,
        start_menu_w() - 2,
        30,
        10,
        TITLE_MIST,
        92,
    );
    crate::arch::gfx_fill_rect_alpha(
        start_menu_x() + 6,
        start_menu_y() + 5,
        start_menu_w() - 12,
        1,
        GLASS_HIGHLIGHT,
        180,
    );
    let title = "RUSTIX 开始";
    let title_x = start_menu_x() + (start_menu_w() - crate::arch::text_width(title, 1)) / 2;
    crate::arch::draw_text(title_x, start_menu_y() + 10, title, INK, 1);
    crate::arch::draw_text(title_x, start_menu_y() + 9, title, GLASS_HIGHLIGHT, 1);
    crate::arch::draw_text(
        start_menu_x() + 12,
        start_menu_y() + 36,
        "APPS",
        GLASS_HIGHLIGHT,
        1,
    );

    for (index, icon) in start_menu_items().iter().enumerate() {
        let row_y = start_menu_y() + 16 + index as i32 * 28;
        let hovered = desktop.hover == HitTarget::StartMenuItem(*icon);
        let pressed = desktop.pressed == HitTarget::StartMenuItem(*icon);
        let fill = if pressed {
            SOFT_GRAY
        } else if hovered {
            SELECTION
        } else {
            GLASS_TINT
        };

        draw_glass_sheet_panel(
            start_menu_x() + 10,
            row_y,
            start_menu_w() - 20,
            22,
            6,
            fill,
            if hovered { 188 } else { 132 },
            72,
        );
        draw_small_icon(*icon, start_menu_x() + 18, row_y + 3);
        crate::arch::draw_text(start_menu_x() + 44, row_y + 7, icon_label(*icon), INK, 1);
    }
}

fn draw_context_menu(desktop: &DesktopState) {
    let menu = unsafe { context_menu_ref() };
    let Some(kind) = menu.kind else { return };
    let x = menu.x;
    let y = menu.y;
    let height = context_menu_height(kind);
    draw_glass_sheet_panel(x, y, CONTEXT_MENU_WIDTH, height, 8, GLASS_TINT, 210, 92);

    for index in 0..context_menu_item_count(kind) {
        let Some(action) = context_menu_action(kind, index) else {
            continue;
        };
        let row_y = y + CONTEXT_MENU_PADDING + index as i32 * CONTEXT_MENU_ROW_HEIGHT;
        let hovered = desktop.hover == HitTarget::ContextMenuItem(action);
        let pressed = desktop.pressed == HitTarget::ContextMenuItem(action);
        if hovered || pressed {
            crate::arch::gfx_fill_round_rect_alpha(
                x + 4,
                row_y,
                CONTEXT_MENU_WIDTH - 8,
                CONTEXT_MENU_ROW_HEIGHT - 2,
                5,
                if pressed { SOFT_GRAY } else { SELECTION },
                196,
            );
        }

        match action {
            ContextAction::DesktopFiles | ContextAction::FilesNewDirectory => {
                draw_folder_symbol(x + 10, row_y + 2)
            }
            ContextAction::DesktopCommand => draw_terminal_symbol(x + 10, row_y + 2),
            ContextAction::DesktopSettings => draw_settings_symbol(x + 10, row_y + 2),
            ContextAction::FilesNewFile => draw_file_symbol(x + 12, row_y + 1),
        }
        crate::arch::draw_text(x + 38, row_y + 3, context_action_label(action), INK, 1);
    }
}

fn draw_settings_window(desktop: &DesktopState, window: WindowState, active: bool) {
    draw_window_shell(desktop, WindowId::Settings, window, "设置", active);
    let config = load_settings_config();

    draw_glass_sheet_panel(
        window.x + 14,
        window.y + 40,
        window.w - 28,
        110,
        10,
        SETTINGS_PANEL,
        138,
        86,
    );
    draw_setting_row(
        window,
        "亮度",
        config.brightness,
        settings_row1_y(window),
        desktop.hover == HitTarget::SettingsBrightnessDown,
        desktop.hover == HitTarget::SettingsBrightnessUp,
    );
    draw_setting_row(
        window,
        "音量",
        config.volume,
        settings_row2_y(window),
        desktop.hover == HitTarget::SettingsVolumeDown,
        desktop.hover == HitTarget::SettingsVolumeUp,
    );

    crate::arch::draw_text(window.x + 18, window.y + 124, "主题", MUTED_TEXT, 1);
    draw_glass_sheet_panel(
        settings_theme_x(window),
        settings_theme_y(window),
        88,
        18,
        5,
        SETTINGS_PANEL,
        152,
        70,
    );
    crate::arch::draw_text(
        settings_theme_x(window) + 8,
        settings_theme_y(window) + 6,
        config.theme.as_str(),
        INK,
        1,
    );

    draw_glass_sheet_panel(
        window.x + 16,
        window.y + window.h - 32,
        window.w - 32,
        18,
        6,
        GLASS_TINT,
        128,
        64,
    );
    crate::arch::draw_text(
        window.x + 20,
        window.y + window.h - 24,
        desktop.settings_status.as_str(),
        MUTED_TEXT,
        1,
    );
}

fn draw_setting_row(
    window: WindowState,
    label: &str,
    value: u8,
    y: i32,
    hover_minus: bool,
    hover_plus: bool,
) {
    draw_glass_sheet_panel(
        window.x + 16,
        y - 2,
        window.w - 32,
        24,
        7,
        PANEL_PAPER,
        104,
        58,
    );
    crate::arch::draw_text(window.x + 18, y + 5, label, MUTED_TEXT, 1);
    draw_round_button(
        settings_minus_x(window),
        y,
        18,
        18,
        if hover_minus {
            SELECTION
        } else {
            SETTINGS_PANEL
        },
        "-",
    );
    draw_glass_sheet_panel(settings_value_x(window), y, 54, 18, 5, PANEL_PAPER, 160, 70);
    let mut text = InlineString::<16>::new();
    let _ = core::fmt::write(&mut text, format_args!("{}", value));
    crate::arch::draw_text(settings_value_x(window) + 14, y + 6, text.as_str(), INK, 1);
    draw_glass_progress(
        window.x + 52,
        y + 6,
        72,
        7,
        value,
        if label == "亮度" {
            ACTIVE_GLOW
        } else {
            COPPER_ACCENT
        },
    );
    draw_round_button(
        settings_plus_x(window),
        y,
        18,
        18,
        if hover_plus {
            SELECTION
        } else {
            SETTINGS_PANEL
        },
        "+",
    );
}

fn draw_video_window(desktop: &DesktopState, window: WindowState, active: bool) {
    draw_window_shell(desktop, WindowId::Video, window, "视频", active);

    let mut title = InlineString::<64>::new();
    let mut frame = InlineString::<768>::new();
    let frame_count = load_video_frame(
        desktop.video_path.as_str(),
        desktop.video_frame,
        &mut title,
        &mut frame,
    );

    crate::arch::draw_text(window.x + 18, window.y + 32, title.as_str(), INK, 1);
    let mut path_line = InlineString::<64>::new();
    path_line.set(desktop.video_path.as_str());
    crate::arch::draw_text(
        window.x + 18,
        window.y + 42,
        path_line.as_str(),
        MUTED_TEXT,
        1,
    );

    draw_round_button(
        video_prev_x(window),
        video_controls_y(window),
        30,
        18,
        if desktop.hover == HitTarget::VideoPrev {
            SELECTION
        } else {
            VIDEO_PANEL
        },
        "上",
    );
    draw_round_button(
        video_play_x(window),
        video_controls_y(window),
        48,
        18,
        if desktop.hover == HitTarget::VideoPlay {
            SELECTION
        } else {
            VIDEO_PANEL
        },
        "播放",
    );
    draw_round_button(
        video_next_x(window),
        video_controls_y(window),
        30,
        18,
        if desktop.hover == HitTarget::VideoNext {
            SELECTION
        } else {
            VIDEO_PANEL
        },
        "下",
    );

    draw_shadow_rounded_panel(
        video_canvas_x(window),
        video_canvas_y(window),
        video_canvas_w(window),
        video_canvas_h(window),
        8,
        VIDEO_PANEL,
        GLASS_EDGE,
    );
    draw_multiline_text(
        video_canvas_x(window) + 10,
        video_canvas_y(window) + 10,
        frame.as_str(),
        WINDOW_WHITE,
        1,
        ((video_canvas_h(window) - 20) / 8) as usize,
        ((video_canvas_w(window) - 20) / 6) as usize,
    );

    let mut footer = InlineString::<64>::new();
    let shown = desktop.video_frame.saturating_add(1);
    let _ = core::fmt::write(
        &mut footer,
        format_args!(
            "frame {}/{}  {}",
            shown,
            frame_count.max(1),
            desktop.video_status.as_str()
        ),
    );
    crate::arch::draw_text(
        window.x + 18,
        window.y + window.h - 24,
        footer.as_str(),
        MUTED_TEXT,
        1,
    );
}

fn draw_audio_window(desktop: &DesktopState, window: WindowState, active: bool) {
    draw_window_shell(desktop, WindowId::Audio, window, "音频", active);

    crate::arch::draw_text(window.x + 18, window.y + 32, "音频播放器", INK, 1);
    crate::arch::draw_text(
        window.x + 18,
        window.y + 42,
        desktop.audio_path.as_str(),
        MUTED_TEXT,
        1,
    );

    draw_round_button(
        audio_play_x(window),
        audio_controls_y(window),
        54,
        18,
        if desktop.hover == HitTarget::AudioPlay {
            SELECTION
        } else {
            AUDIO_PANEL
        },
        "播放",
    );
    draw_shadow_rounded_panel(
        audio_list_x(window),
        audio_list_y(window),
        audio_list_w(window),
        audio_list_h(window),
        8,
        AUDIO_PANEL,
        GLASS_EDGE,
    );

    let mut notes = InlineString::<768>::new();
    load_audio_notes_preview(desktop.audio_path.as_str(), &mut notes);
    draw_multiline_text(
        audio_list_x(window) + 10,
        audio_list_y(window) + 10,
        notes.as_str(),
        WINDOW_WHITE,
        1,
        ((audio_list_h(window) - 20) / 8) as usize,
        ((audio_list_w(window) - 20) / 6) as usize,
    );

    crate::arch::draw_text(
        window.x + 18,
        window.y + window.h - 24,
        desktop.audio_status.as_str(),
        MUTED_TEXT,
        1,
    );
}

fn draw_files_window(desktop: &DesktopState, window: WindowState, active: bool) {
    draw_window_shell(desktop, WindowId::Files, window, "文件", active);

    draw_glass_sheet_panel(
        window.x + 12,
        window.y + 30,
        window.w - 24,
        28,
        8,
        GLASS_TINT,
        132,
        82,
    );
    draw_glass_sheet_panel(
        window.x + 14,
        window.y + 34,
        30,
        18,
        5,
        PANEL_PAPER,
        152,
        64,
    );
    crate::arch::draw_text(window.x + 22, window.y + 39, "<", INK, 2);

    draw_glass_sheet_panel(
        window.x + 54,
        window.y + 34,
        window.w - 70,
        18,
        5,
        PANEL_PAPER,
        148,
        58,
    );
    crate::arch::draw_text(
        window.x + 62,
        window.y + 34,
        desktop.files_path.as_str(),
        INK,
        2,
    );

    draw_glass_sheet_panel(
        window.x + 16,
        window.y + 68,
        window.w - 32,
        window.h - 108,
        10,
        GLASS_TINT,
        114,
        66,
    );
    crate::arch::gfx_fill_rect_alpha(
        window.x + 20,
        window.y + 92,
        window.w - 40,
        1,
        GLASS_HIGHLIGHT,
        104,
    );
    crate::arch::draw_text(window.x + 22, window.y + 74, "文件", MUTED_TEXT, 1);
    crate::arch::draw_text(
        window.x + window.w - 22 - crate::arch::text_width("类型", 1),
        window.y + 74,
        "类型",
        MUTED_TEXT,
        1,
    );

    for index in 0..desktop.files_len {
        draw_files_row(desktop, window, index);
    }

    draw_glass_sheet_panel(
        window.x + 16,
        window.y + window.h - 30,
        window.w - 32,
        18,
        6,
        GLASS_TINT,
        126,
        60,
    );
    crate::arch::draw_text(
        window.x + 22,
        window.y + window.h - 24,
        desktop.file_status.as_str(),
        MUTED_TEXT,
        1,
    );
}

fn draw_files_row(desktop: &DesktopState, window: WindowState, index: usize) {
    let row_x = files_list_x(window);
    let row_y = files_list_y(window) + index as i32 * FILE_ROW_HEIGHT;
    let row_w = files_list_w(window);
    let hovered = desktop.hover == HitTarget::FilesEntry(index);
    let pressed = desktop.pressed == HitTarget::FilesEntry(index);
    let selected = desktop.selected_file_entry == Some(index);
    let entry = desktop.files_entries[index];

    let fill = if pressed {
        SOFT_GRAY
    } else if hovered || selected {
        SELECTION
    } else {
        PANEL_PAPER
    };

    draw_glass_sheet_panel(
        row_x,
        row_y,
        row_w,
        FILE_ROW_HEIGHT - 2,
        6,
        fill,
        if hovered || selected { 176 } else { 118 },
        62,
    );
    match entry.kind {
        BrowserKind::Directory => {
            draw_folder_symbol(row_x + 8, row_y + 5);
            crate::arch::draw_text(
                row_x + row_w - 14 - crate::arch::text_width("目录", 1),
                row_y + 7,
                "目录",
                MUTED_TEXT,
                1,
            );
        }
        BrowserKind::File => {
            draw_file_symbol(row_x + 10, row_y + 4);
            crate::arch::draw_text(
                row_x + row_w - 14 - crate::arch::text_width("文件", 1),
                row_y + 7,
                "文件",
                MUTED_TEXT,
                1,
            );
        }
    }
    crate::arch::draw_text(row_x + 34, row_y + 7, entry.name.as_str(), INK, 2);
}

fn draw_cmd_window(desktop: &DesktopState, window: WindowState, active: bool) {
    draw_window_shell(desktop, WindowId::Cmd, window, "命令提示符", active);
    draw_shadow_rounded_panel(
        window.x + 8,
        window.y + 30,
        window.w - 16,
        window.h - 40,
        8,
        TERMINAL_BLACK,
        GLASS_EDGE,
    );

    let text_x = window.x + 14;
    let text_y = window.y + 38;
    let rows = crate::arch::gfx_console_rows();
    for row in 0..rows {
        for col in 0..crate::arch::gfx_console_cols() {
            let cell = crate::arch::gfx_console_cell(row, col);
            if cell == 0 || cell == 0xFFFF {
                continue;
            }
            if let Some(ch) = char::from_u32(cell as u32) {
                let mut utf8 = [0u8; 4];
                let text = ch.encode_utf8(&mut utf8);
                crate::arch::draw_text_compact(
                    text_x + col as i32 * 6,
                    text_y + row as i32 * 8,
                    text,
                    WINDOW_WHITE,
                    1,
                );
            }
        }
    }

    let (cursor_row, cursor_col) = crate::arch::gfx_console_cursor();
    let cursor_x = text_x + cursor_col as i32 * 6;
    let cursor_y = text_y + cursor_row as i32 * 8;
    crate::arch::gfx_fill_rect(cursor_x, cursor_y, 1, 7, WINDOW_WHITE);
}

fn draw_editor_window(desktop: &DesktopState, window: WindowState, active: bool) {
    draw_window_shell(desktop, WindowId::Editor, window, "编辑", active);

    draw_shadow_rounded_panel(
        editor_save_x(window),
        editor_save_y(window),
        44,
        16,
        5,
        MINIMIZE_YELLOW,
        GLASS_EDGE,
    );
    crate::arch::draw_text(
        editor_save_x(window) + 6,
        editor_save_y(window) + 4,
        "保存",
        INK,
        1,
    );

    crate::arch::gfx_fill_rect(window.x + 16, window.y + 40, window.w - 32, 1, GLASS_EDGE);
    crate::arch::draw_text(
        window.x + 20,
        window.y + 30,
        desktop.editor_path.as_str(),
        INK,
        2,
    );

    draw_shadow_rounded_panel(
        editor_text_x(window),
        editor_text_y(window),
        editor_text_w(window),
        editor_text_h(window),
        8,
        EDITOR_PAPER,
        GLASS_EDGE,
    );

    draw_editor_text(desktop, window);

    crate::arch::gfx_fill_rect(
        window.x + 16,
        window.y + window.h - 34,
        window.w - 32,
        1,
        GLASS_EDGE,
    );
    crate::arch::draw_text(
        window.x + 20,
        window.y + window.h - 24,
        desktop.editor_status.as_str(),
        MUTED_TEXT,
        1,
    );
}

fn draw_editor_text(desktop: &DesktopState, window: WindowState) {
    let text = desktop.editor_text.as_str();
    let max_cols = ((editor_text_w(window) - 14) / 6) as usize;
    let max_rows = ((editor_text_h(window) - 12) / 8) as usize;
    let mut line = InlineString::<96>::new();
    let mut row = 0usize;
    let mut last_col = 0usize;

    for byte in text.bytes() {
        if row >= max_rows {
            break;
        }
        if byte == b'\n' || line.as_str().len() >= max_cols {
            crate::arch::draw_text(
                editor_text_x(window) + 8,
                editor_text_y(window) + 8 + row as i32 * 8,
                line.as_str(),
                INK,
                1,
            );
            row += 1;
            line.clear();
            if byte == b'\n' {
                last_col = 0;
                continue;
            }
        }

        let _ = line.push_byte(byte);
        last_col = line.as_str().len();
    }

    if row < max_rows {
        crate::arch::draw_text(
            editor_text_x(window) + 8,
            editor_text_y(window) + 8 + row as i32 * 8,
            line.as_str(),
            INK,
            1,
        );
        let cursor_x = editor_text_x(window) + 8 + last_col as i32 * 6;
        let cursor_y = editor_text_y(window) + 8 + row as i32 * 8;
        crate::arch::gfx_fill_rect(cursor_x, cursor_y, 1, 7, INK);
    }
}

fn draw_window_shell(
    desktop: &DesktopState,
    id: WindowId,
    window: WindowState,
    title: &str,
    active: bool,
) {
    draw_window_frame(window.x, window.y, window.w, window.h, 12);
    crate::arch::gfx_fill_round_rect(
        window.x + 4,
        window.y + 4,
        window.w - 8,
        TITLE_BAR_HEIGHT,
        9,
        TITLE_MIST,
    );
    crate::arch::gfx_fill_rect(
        window.x + 10,
        window.y + 7,
        window.w - 20,
        1,
        GLASS_HIGHLIGHT,
    );

    let title_chip = if active { ACTIVE_GLOW } else { GLASS_EDGE };
    draw_shadow_rounded_panel(
        window.x + 18,
        window.y + 5,
        42,
        12,
        6,
        title_chip,
        GLASS_EDGE,
    );
    let title_w = crate::arch::text_width(title, 1);
    let title_x = (window.x + window.w - title_w - 46)
        .clamp(window.x + 68, close_button_x(window) - title_w - 10);
    crate::arch::draw_text(title_x, window.y + 8, title, INK, 1);
    crate::arch::draw_text(title_x, window.y + 7, title, GLASS_HIGHLIGHT, 1);

    draw_title_button(
        min_button_x(window),
        window.y + 5,
        MINIMIZE_YELLOW,
        matches!(desktop.hover, HitTarget::Minimize(target) if target == id),
        matches!(desktop.pressed, HitTarget::Minimize(target) if target == id),
    );
    draw_title_button(max_button_x(window), window.y + 5, ORANGE, false, false);
    draw_title_button(
        close_button_x(window),
        window.y + 5,
        CLOSE_RED,
        matches!(desktop.hover, HitTarget::Close(target) if target == id),
        matches!(desktop.pressed, HitTarget::Close(target) if target == id),
    );

    crate::arch::gfx_fill_rect(min_button_x(window) + 3, window.y + 15, 8, 1, INK);
    crate::arch::gfx_fill_rect(max_button_x(window) + 4, window.y + 9, 6, 5, INK);
    crate::arch::gfx_fill_rect(max_button_x(window) + 5, window.y + 10, 4, 3, ORANGE);
    crate::arch::draw_text(close_button_x(window) + 3, window.y + 7, "X", INK, 2);
}

fn snapshot_window(desktop: &DesktopState, id: WindowId) -> WindowState {
    match id {
        WindowId::Files => desktop.files,
        WindowId::Cmd => desktop.cmd,
        WindowId::Editor => desktop.editor,
        WindowId::Settings => desktop.settings,
        WindowId::Video => desktop.video,
        WindowId::Audio => desktop.audio,
    }
}

fn draw_title_button(x: i32, y: i32, color: u8, hovered: bool, pressed: bool) {
    let fill = if pressed {
        TITLE_MIST
    } else if hovered {
        GLASS_HIGHLIGHT
    } else {
        color
    };
    draw_shadow_rounded_panel(x, y, 14, 14, 5, fill, GLASS_EDGE);
}

fn draw_folder_symbol(x: i32, y: i32) {
    crate::arch::gfx_fill_round_rect(x, y + 4, 20, 14, 3, MINIMIZE_YELLOW);
    crate::arch::gfx_fill_rect(x + 3, y + 1, 7, 5, MINIMIZE_YELLOW);
    crate::arch::gfx_fill_rect(x, y + 5, 20, 1, INK);
}

fn draw_terminal_symbol(x: i32, y: i32) {
    crate::arch::gfx_fill_round_rect(x, y + 1, 20, 16, 4, TERMINAL_BLACK);
    crate::arch::draw_text(x + 4, y + 5, ">", WINDOW_WHITE, 2);
    crate::arch::gfx_fill_rect(x + 11, y + 11, 6, 2, WINDOW_WHITE);
}

fn draw_editor_symbol(x: i32, y: i32) {
    crate::arch::gfx_fill_round_rect(x + 2, y + 1, 16, 16, 4, WINDOW_WHITE);
    crate::arch::gfx_fill_rect(x + 13, y + 2, 3, 4, TASKBAR_CYAN);
    crate::arch::gfx_fill_rect(x + 2, y + 1, 16, 1, INK);
    crate::arch::gfx_fill_rect(x + 2, y + 16, 16, 1, INK);
    crate::arch::gfx_fill_rect(x + 2, y + 1, 1, 16, INK);
    crate::arch::gfx_fill_rect(x + 17, y + 1, 1, 16, INK);
    crate::arch::gfx_fill_rect(x + 6, y + 7, 8, 2, TITLE_BLUE);
    crate::arch::gfx_fill_rect(x + 6, y + 11, 6, 2, TITLE_BLUE);
}

fn draw_settings_symbol(x: i32, y: i32) {
    crate::arch::gfx_fill_round_rect(x + 3, y + 3, 14, 14, 7, SETTINGS_PANEL);
    crate::arch::gfx_fill_rect(x + 8, y + 1, 4, 18, INK);
    crate::arch::gfx_fill_rect(x + 1, y + 8, 18, 4, INK);
    crate::arch::gfx_fill_round_rect(x + 5, y + 5, 10, 10, 5, WINDOW_WHITE);
}

fn draw_video_symbol(x: i32, y: i32) {
    crate::arch::gfx_fill_round_rect(x, y + 2, 20, 14, 4, VIDEO_PANEL);
    crate::arch::gfx_fill_rect(x + 2, y + 4, 16, 10, INK);
    crate::arch::gfx_fill_rect(x + 8, y + 6, 1, 6, WINDOW_WHITE);
    crate::arch::gfx_fill_rect(x + 9, y + 7, 1, 4, WINDOW_WHITE);
    crate::arch::gfx_fill_rect(x + 10, y + 8, 1, 2, WINDOW_WHITE);
}

fn draw_audio_symbol(x: i32, y: i32) {
    crate::arch::gfx_fill_rect(x + 9, y + 2, 2, 11, INK);
    crate::arch::gfx_fill_rect(x + 11, y + 2, 6, 2, INK);
    crate::arch::gfx_fill_circle(x + 7, y + 15, 3, AUDIO_PANEL);
    crate::arch::gfx_fill_circle(x + 15, y + 13, 3, AUDIO_PANEL);
}

fn draw_small_icon(icon: IconId, x: i32, y: i32) {
    match icon {
        IconId::Files => draw_folder_symbol(x, y),
        IconId::Cmd => draw_terminal_symbol(x, y),
        IconId::Editor => draw_editor_symbol(x, y),
        IconId::Settings => draw_settings_symbol(x, y),
        IconId::Video => draw_video_symbol(x, y),
        IconId::Audio => draw_audio_symbol(x, y),
    }
}

fn draw_round_button(x: i32, y: i32, w: i32, h: i32, fill: u8, label: &str) {
    draw_glass_sheet_panel(x, y, w, h, 5, fill, 164, 70);
    let label_x = x + (w - crate::arch::text_width(label, 1)) / 2;
    crate::arch::draw_text(label_x, y + 6, label, INK, 1);
}

fn draw_file_symbol(x: i32, y: i32) {
    crate::arch::gfx_fill_round_rect(x, y, 15, 18, 3, WINDOW_WHITE);
    crate::arch::gfx_fill_rect(x + 10, y + 1, 3, 4, TASKBAR_CYAN);
    crate::arch::gfx_fill_rect(x, y, 15, 1, INK);
    crate::arch::gfx_fill_rect(x, y + 17, 15, 1, INK);
    crate::arch::gfx_fill_rect(x, y, 1, 18, INK);
    crate::arch::gfx_fill_rect(x + 14, y, 1, 18, INK);
}

fn draw_rounded_panel(x: i32, y: i32, w: i32, h: i32, radius: i32, fill: u8, border: u8) {
    crate::arch::gfx_fill_round_rect(x, y, w, h, radius, border);
    crate::arch::gfx_fill_round_rect_alpha(
        x + 1,
        y + 1,
        w - 2,
        h - 2,
        (radius - 1).max(0),
        fill,
        230,
    );
}

fn draw_shadow_rounded_panel(x: i32, y: i32, w: i32, h: i32, radius: i32, fill: u8, border: u8) {
    draw_shadow(x + 4, y + 5, w, h, radius, GLASS_SHADOW);
    draw_rounded_panel(x, y, w, h, radius, border, GLASS_EDGE);
    crate::arch::gfx_fill_round_rect_alpha(
        x + 1,
        y + 1,
        w - 2,
        h - 2,
        (radius - 1).max(0),
        fill,
        212,
    );
    crate::arch::gfx_fill_rect_alpha(x + 5, y + 4, (w - 10).max(0), 1, GLASS_HIGHLIGHT, 180);
}

fn draw_window_frame(x: i32, y: i32, w: i32, h: i32, radius: i32) {
    draw_shadow(x + 5, y + 7, w, h, radius, GLASS_SHADOW);
    crate::arch::gfx_fill_round_rect(x, y, w, h, radius, GLASS_EDGE);
    crate::arch::gfx_fill_round_rect_alpha(
        x + 4,
        y + 4,
        w - 8,
        h - 8,
        (radius - 4).max(0),
        PANEL_PAPER,
        222,
    );
    crate::arch::gfx_fill_rect_alpha(x + 8, y + TITLE_BAR_HEIGHT + 8, w - 16, 1, PANEL_INSET, 176);
}

fn draw_glass_taskbar(x: i32, y: i32, w: i32, h: i32, radius: i32) {
    draw_shadow(x + 4, y + 7, w, h, radius, GLASS_SHADOW);
    crate::arch::gfx_fill_round_rect(x, y, w, h, radius, TASKBAR_DEEP);
    crate::arch::gfx_fill_round_rect_alpha(
        x + 1,
        y + 1,
        w - 2,
        h - 2,
        (radius - 1).max(0),
        GLASS_TINT,
        144,
    );
    crate::arch::gfx_fill_round_rect_alpha(
        x + 1,
        y + 1,
        w - 2,
        h / 2,
        (radius - 1).max(0),
        GLASS_HIGHLIGHT,
        88,
    );
    crate::arch::gfx_fill_rect_alpha(x + 14, y + 5, w - 28, 1, GLASS_HIGHLIGHT, 164);
}

fn draw_glass_chip(x: i32, y: i32, w: i32, h: i32, radius: i32, fill: u8, active: bool) {
    crate::arch::gfx_fill_round_rect(x, y, w, h, radius, GLASS_EDGE);
    crate::arch::gfx_fill_round_rect_alpha(
        x + 1,
        y + 1,
        w - 2,
        h - 2,
        (radius - 1).max(0),
        fill,
        if active { 184 } else { 132 },
    );
    crate::arch::gfx_fill_round_rect_alpha(
        x + 1,
        y + 1,
        w - 2,
        (h / 2).max(2),
        (radius - 1).max(0),
        GLASS_HIGHLIGHT,
        82,
    );
    if active {
        crate::arch::gfx_fill_rect_alpha(x + 5, y + h - 2, w - 10, 1, ACTIVE_GLOW, 180);
    }
}

fn draw_glass_sheet_panel(
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    radius: i32,
    tint: u8,
    body_alpha: u8,
    top_alpha: u8,
) {
    draw_shadow(x + 4, y + 5, w, h, radius, GLASS_SHADOW);
    crate::arch::gfx_fill_round_rect(x, y, w, h, radius, GLASS_EDGE);
    crate::arch::gfx_fill_round_rect_alpha(
        x + 1,
        y + 1,
        w - 2,
        h - 2,
        (radius - 1).max(0),
        tint,
        body_alpha,
    );
    crate::arch::gfx_fill_round_rect_alpha(
        x + 1,
        y + 1,
        w - 2,
        (h / 2).max(2),
        (radius - 1).max(0),
        GLASS_HIGHLIGHT,
        top_alpha,
    );
    crate::arch::gfx_fill_rect_alpha(x + 5, y + 4, (w - 10).max(0), 1, GLASS_HIGHLIGHT, 176);
}

fn draw_glass_progress(x: i32, y: i32, w: i32, h: i32, value: u8, accent: u8) {
    let width = w.max(0);
    let filled = (width * value as i32) / 100;
    draw_glass_sheet_panel(x, y, width, h, 3, GLASS_TINT, 110, 48);
    if filled > 0 {
        crate::arch::gfx_fill_round_rect_alpha(
            x + 1,
            y + 1,
            (filled - 2).max(1),
            (h - 2).max(1),
            2,
            accent,
            156,
        );
    }
}

fn draw_translucent_separator(x: i32, y: i32, h: i32) {
    crate::arch::gfx_fill_rect_alpha(x, y, 1, h, GLASS_HIGHLIGHT, 92);
    crate::arch::gfx_fill_rect_alpha(x + 1, y, 1, h, GLASS_EDGE, 72);
}

fn draw_shadow(x: i32, y: i32, w: i32, h: i32, radius: i32, color: u8) {
    crate::arch::gfx_fill_round_rect_alpha(x, y, w, h, radius, color, 74);
    crate::arch::gfx_fill_round_rect_alpha(
        x + 1,
        y + 1,
        w - 2,
        h - 2,
        (radius - 1).max(0),
        color,
        38,
    );
}

fn taskbar_x() -> i32 {
    (crate::arch::screen_width() - TASKBAR_W) / 2
}

fn taskbar_y() -> i32 {
    crate::arch::screen_height() - TASKBAR_H - 18
}

fn file_dock_x() -> i32 {
    taskbar_x() + 70
}

fn cmd_dock_x() -> i32 {
    taskbar_x() + 118
}

fn editor_dock_x() -> i32 {
    taskbar_x() + 166
}

fn clock_chip_x() -> i32 {
    taskbar_x() + TASKBAR_W - clock_chip_w() - 12
}

fn clock_chip_y() -> i32 {
    taskbar_y() + 6
}

fn clock_chip_w() -> i32 {
    124
}

fn clock_chip_h() -> i32 {
    30
}

fn files_back_x(window: WindowState) -> i32 {
    window.x + 14
}

fn files_back_y(window: WindowState) -> i32 {
    window.y + 34
}

fn files_list_x(window: WindowState) -> i32 {
    window.x + 18
}

fn files_list_y(window: WindowState) -> i32 {
    window.y + 102
}

fn files_list_w(window: WindowState) -> i32 {
    window.w - 36
}

fn editor_save_x(window: WindowState) -> i32 {
    window.x + window.w - 104
}

fn editor_save_y(window: WindowState) -> i32 {
    window.y + 29
}

fn editor_text_x(window: WindowState) -> i32 {
    window.x + 16
}

fn editor_text_y(window: WindowState) -> i32 {
    window.y + 52
}

fn editor_text_w(window: WindowState) -> i32 {
    window.w - 32
}

fn editor_text_h(window: WindowState) -> i32 {
    window.h - 92
}

fn start_button_x() -> i32 {
    taskbar_x() + 8
}

fn start_menu_x() -> i32 {
    taskbar_x() + 4
}

fn start_menu_y() -> i32 {
    taskbar_y() - start_menu_h() - 8
}

fn start_menu_w() -> i32 {
    176
}

fn start_menu_h() -> i32 {
    188
}

fn context_menu_height(kind: ContextMenuKind) -> i32 {
    CONTEXT_MENU_PADDING * 2 + context_menu_item_count(kind) as i32 * CONTEXT_MENU_ROW_HEIGHT
}

fn settings_minus_x(window: WindowState) -> i32 {
    window.x + 94
}

fn settings_value_x(window: WindowState) -> i32 {
    window.x + 118
}

fn settings_plus_x(window: WindowState) -> i32 {
    window.x + 178
}

fn settings_row1_y(window: WindowState) -> i32 {
    window.y + 50
}

fn settings_row2_y(window: WindowState) -> i32 {
    window.y + 82
}

fn settings_theme_x(window: WindowState) -> i32 {
    window.x + 94
}

fn settings_theme_y(window: WindowState) -> i32 {
    window.y + 118
}

fn video_controls_y(window: WindowState) -> i32 {
    window.y + 58
}

fn video_prev_x(window: WindowState) -> i32 {
    window.x + 18
}

fn video_play_x(window: WindowState) -> i32 {
    window.x + 54
}

fn video_next_x(window: WindowState) -> i32 {
    window.x + 108
}

fn video_canvas_x(window: WindowState) -> i32 {
    window.x + 18
}

fn video_canvas_y(window: WindowState) -> i32 {
    window.y + 88
}

fn video_canvas_w(window: WindowState) -> i32 {
    window.w - 36
}

fn video_canvas_h(window: WindowState) -> i32 {
    window.h - 124
}

fn audio_controls_y(window: WindowState) -> i32 {
    window.y + 58
}

fn audio_play_x(window: WindowState) -> i32 {
    window.x + 18
}

fn audio_list_x(window: WindowState) -> i32 {
    window.x + 18
}

fn audio_list_y(window: WindowState) -> i32 {
    window.y + 88
}

fn audio_list_w(window: WindowState) -> i32 {
    window.w - 36
}

fn audio_list_h(window: WindowState) -> i32 {
    window.h - 122
}

fn all_windows() -> [WindowId; 6] {
    [
        WindowId::Files,
        WindowId::Cmd,
        WindowId::Editor,
        WindowId::Settings,
        WindowId::Video,
        WindowId::Audio,
    ]
}

fn start_menu_items() -> [IconId; 6] {
    [
        IconId::Settings,
        IconId::Video,
        IconId::Audio,
        IconId::Files,
        IconId::Cmd,
        IconId::Editor,
    ]
}

fn context_menu_item_count(kind: ContextMenuKind) -> usize {
    match kind {
        ContextMenuKind::Desktop => 3,
        ContextMenuKind::Files => 2,
    }
}

fn context_menu_action(kind: ContextMenuKind, index: usize) -> Option<ContextAction> {
    match (kind, index) {
        (ContextMenuKind::Desktop, 0) => Some(ContextAction::DesktopFiles),
        (ContextMenuKind::Desktop, 1) => Some(ContextAction::DesktopCommand),
        (ContextMenuKind::Desktop, 2) => Some(ContextAction::DesktopSettings),
        (ContextMenuKind::Files, 0) => Some(ContextAction::FilesNewDirectory),
        (ContextMenuKind::Files, 1) => Some(ContextAction::FilesNewFile),
        _ => None,
    }
}

fn context_action_label(action: ContextAction) -> &'static str {
    match action {
        ContextAction::DesktopFiles => "文件",
        ContextAction::DesktopCommand => "命令提示符",
        ContextAction::DesktopSettings => "设置",
        ContextAction::FilesNewDirectory => "新建文件夹",
        ContextAction::FilesNewFile => "新建文件",
    }
}

fn icon_label(icon: IconId) -> &'static str {
    match icon {
        IconId::Files => "文件",
        IconId::Cmd => "命令提示符",
        IconId::Editor => "编辑",
        IconId::Settings => "设置",
        IconId::Video => "视频",
        IconId::Audio => "音频",
    }
}

fn write_clock_hm(out: &mut InlineString<16>, time: Option<crate::arch::ClockTime>) {
    out.clear();
    if let Some(time) = time {
        let _ = core::fmt::write(out, format_args!("{:02}:{:02}", time.hour, time.minute));
    } else {
        out.set("--:--");
    }
}

fn write_clock_hms(out: &mut InlineString<16>, time: Option<crate::arch::ClockTime>) {
    out.clear();
    if let Some(time) = time {
        let _ = core::fmt::write(
            out,
            format_args!("{:02}:{:02}:{:02}", time.hour, time.minute, time.second),
        );
    } else {
        out.set("--:--:--");
    }
}

fn write_clock_seconds(out: &mut InlineString<8>, time: Option<crate::arch::ClockTime>) {
    out.clear();
    if let Some(time) = time {
        let _ = core::fmt::write(out, format_args!("{:02}", time.second));
    } else {
        out.set("--");
    }
}

fn draw_multiline_text(
    x: i32,
    y: i32,
    text: &str,
    color: u8,
    scale: i32,
    max_rows: usize,
    max_cols: usize,
) {
    let mut line = InlineString::<96>::new();
    let mut row = 0usize;
    for byte in text.bytes() {
        if row >= max_rows {
            break;
        }
        if byte == b'\n' || line.as_str().len() >= max_cols {
            crate::arch::draw_text(x, y + row as i32 * (8 * scale), line.as_str(), color, scale);
            row += 1;
            line.clear();
            if byte == b'\n' {
                continue;
            }
        }
        let _ = line.push_byte(byte);
    }

    if row < max_rows && !line.as_str().is_empty() {
        crate::arch::draw_text(x, y + row as i32 * (8 * scale), line.as_str(), color, scale);
    }
}

fn load_settings_config() -> SettingsConfig {
    let mut scratch = InlineString::<2048>::new();
    let vfs = VfsService::new();
    let Some(text) = vfs.read_file("/etc/rustix/settings.conf", &mut scratch) else {
        return SettingsConfig::defaults();
    };

    let mut config = SettingsConfig::defaults();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "brightness" => {
                if let Ok(parsed) = parse_u32(value.trim()) {
                    config.brightness = parsed.min(100) as u8;
                }
            }
            "volume" => {
                if let Ok(parsed) = parse_u32(value.trim()) {
                    config.volume = parsed.min(100) as u8;
                }
            }
            "ui_scale" => {
                if let Ok(parsed) = parse_u32(value.trim()) {
                    config.ui_scale = parsed.clamp(1, 4) as u8;
                }
            }
            "theme" => {
                config.theme = match value.trim() {
                    "sunrise" => Theme::Sunrise,
                    "glacier" => Theme::Glacier,
                    _ => Theme::Graphite,
                };
            }
            _ => {}
        }
    }
    config
}

fn write_settings_config(vfs: &VfsService, config: SettingsConfig) -> crate::linux::Result<()> {
    let mut text = InlineString::<128>::new();
    let _ = core::fmt::write(
        &mut text,
        format_args!(
            "brightness={}\nvolume={}\nui_scale={}\ntheme={}\n",
            config.brightness,
            config.volume,
            config.ui_scale,
            config.theme.as_str()
        ),
    );
    vfs.write_text("/etc/rustix/settings.conf", text.as_str())
}

fn clamp_percent(value: i32) -> u8 {
    value.clamp(0, 100) as u8
}

fn parse_media_header(text: &str) -> (&str, u32, &str) {
    let mut title = "Untitled media";
    let mut rate = 2u32;
    if let Some((header, body)) = text.split_once("\n\n") {
        for line in header.lines() {
            if let Some((key, value)) = line.split_once('=') {
                match key.trim() {
                    "title" => title = value.trim(),
                    "fps" | "volume" => {
                        if let Ok(parsed) = parse_u32(value.trim()) {
                            rate = parsed.max(1);
                        }
                    }
                    _ => {}
                }
            }
        }
        (title, rate, body)
    } else {
        (title, rate, text)
    }
}

fn count_video_frames(path: &str) -> usize {
    let mut scratch = InlineString::<2048>::new();
    let vfs = VfsService::new();
    let Some(text) = vfs.read_file(path, &mut scratch) else {
        return 0;
    };
    let (_, _, body) = parse_media_header(text);
    body.split("\n---\n")
        .filter(|frame| !frame.trim().is_empty())
        .count()
}

fn load_video_frame(
    path: &str,
    wanted_index: usize,
    title_out: &mut InlineString<64>,
    frame_out: &mut InlineString<768>,
) -> usize {
    title_out.clear();
    frame_out.clear();
    let mut scratch = InlineString::<2048>::new();
    let vfs = VfsService::new();
    let Some(text) = vfs.read_file(path, &mut scratch) else {
        title_out.set("Video missing");
        frame_out.set("No frame data");
        return 0;
    };
    let (title, _, body) = parse_media_header(text);
    title_out.set(title);

    let mut count = 0usize;
    let mut last_frame = "";
    for frame in body
        .split("\n---\n")
        .filter(|frame| !frame.trim().is_empty())
    {
        if count == wanted_index {
            let _ = frame_out.push_str(frame.trim_end_matches('\n'));
        }
        last_frame = frame;
        count += 1;
    }

    if count == 0 {
        frame_out.set("No frame data");
    } else if frame_out.is_empty() {
        let _ = frame_out.push_str(last_frame.trim_end_matches('\n'));
    }

    count
}

fn load_audio_notes_preview(path: &str, out: &mut InlineString<768>) {
    out.clear();
    let mut scratch = InlineString::<2048>::new();
    let vfs = VfsService::new();
    let Some(text) = vfs.read_file(path, &mut scratch) else {
        out.set("Audio file missing");
        return;
    };
    let (title, _, body) = parse_media_header(text);
    let _ = out.push_str(title);
    let _ = out.push_str("\n\n");
    for (index, line) in body
        .lines()
        .filter(|line| !line.trim().is_empty())
        .enumerate()
    {
        if index >= 8 {
            let _ = out.push_str("...\n");
            break;
        }
        let _ = out.push_str(line);
        let _ = out.push_str("\n");
    }
}

fn parse_u32(text: &str) -> Result<u32, ()> {
    if text.is_empty() {
        return Err(());
    }

    let mut value = 0u32;
    for byte in text.bytes() {
        if !byte.is_ascii_digit() {
            return Err(());
        }
        value = value
            .saturating_mul(10)
            .saturating_add((byte - b'0') as u32);
    }
    Ok(value)
}

fn point_in_rect(px: i32, py: i32, x: i32, y: i32, w: i32, h: i32) -> bool {
    px >= x && py >= y && px < x + w && py < y + h
}

fn min_button_x(window: WindowState) -> i32 {
    window.x + window.w - 48
}

fn max_button_x(window: WindowState) -> i32 {
    window.x + window.w - 30
}

fn close_button_x(window: WindowState) -> i32 {
    window.x + window.w - 12
}
