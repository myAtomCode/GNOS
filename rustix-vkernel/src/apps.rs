use crate::fixed::InlineString;
use crate::kernel::SysApi;

pub type ProgramEntry = fn(&[&str], &mut SysApi<'_>) -> i32;

pub struct Program {
    pub path: &'static str,
    pub summary: &'static str,
    pub entry: ProgramEntry,
}

const PROGRAMS: [Program; 7] = [
    Program {
        path: "/bin/hello",
        summary: "hello app using console/vfs IPC",
        entry: hello_main,
    },
    Program {
        path: "/bin/game",
        summary: "interactive ASCII dungeon game",
        entry: game_main,
    },
    Program {
        path: "/bin/echo",
        summary: "Linux-style echo utility",
        entry: echo_main,
    },
    Program {
        path: "/bin/calc",
        summary: "tiny integer calculator",
        entry: calc_main,
    },
    Program {
        path: "/bin/settings",
        summary: "Linux-style settings utility backed by /etc/rustix/settings.conf",
        entry: settings_main,
    },
    Program {
        path: "/bin/video-player",
        summary: "safe demo video player for .rvf assets",
        entry: video_player_main,
    },
    Program {
        path: "/bin/audio-player",
        summary: "safe demo audio player for .rta assets",
        entry: audio_player_main,
    },
];

pub fn programs() -> &'static [Program] {
    &PROGRAMS
}

pub fn lookup(path: &str) -> Option<&'static Program> {
    PROGRAMS.iter().find(|program| program.path == path)
}

pub fn resolve_command(command: &str) -> Option<&'static str> {
    match command {
        "hello" => Some("/bin/hello"),
        "game" => Some("/bin/game"),
        "echo" => Some("/bin/echo"),
        "calc" => Some("/bin/calc"),
        "settings" => Some("/bin/settings"),
        "video-player" | "video" => Some("/bin/video-player"),
        "audio-player" | "audio" => Some("/bin/audio-player"),
        _ => None,
    }
}

const SETTINGS_PATH: &str = "/etc/rustix/settings.conf";
const DEMO_VIDEO_PATH: &str = "/usr/share/media/demo-video.rvf";
const DEMO_AUDIO_PATH: &str = "/usr/share/media/demo-audio.rta";

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

fn echo_main(argv: &[&str], api: &mut SysApi<'_>) -> i32 {
    if argv.len() <= 1 {
        api.write_line("");
        return 0;
    }

    let mut line = InlineString::<256>::new();
    for (index, arg) in argv[1..].iter().enumerate() {
        if index > 0 {
            let _ = line.push_byte(b' ');
        }
        let _ = line.push_str(arg);
    }
    api.write_line(line.as_str());
    0
}

fn calc_main(argv: &[&str], api: &mut SysApi<'_>) -> i32 {
    if argv.len() != 4 {
        api.write_line("usage: calc <a> <op> <b>");
        return 1;
    }

    let Ok(left) = parse_i32(argv[1]) else {
        api.write_line("calc: invalid left operand");
        return 1;
    };
    let Ok(right) = parse_i32(argv[3]) else {
        api.write_line("calc: invalid right operand");
        return 1;
    };

    let result = match argv[2] {
        "+" => left.saturating_add(right),
        "-" => left.saturating_sub(right),
        "*" | "x" => left.saturating_mul(right),
        "/" => {
            if right == 0 {
                api.write_line("calc: division by zero");
                return 1;
            }
            left / right
        }
        _ => {
            api.write_line("calc: unsupported operator");
            return 1;
        }
    };

    let mut line = InlineString::<64>::new();
    let _ = core::fmt::write(&mut line, format_args!("{}", result));
    api.write_line(line.as_str());
    0
}

fn settings_main(argv: &[&str], api: &mut SysApi<'_>) -> i32 {
    let mut scratch = InlineString::<2048>::new();
    let text = match api.read_path(SETTINGS_PATH, &mut scratch) {
        Ok(text) => text,
        Err(err) => {
            api.write_line(err);
            return 1;
        }
    };
    let mut config = parse_settings(text);

    if argv.len() <= 1 || argv[1] == "show" {
        show_settings(api, config);
        api.write_line("usage: settings get <key> | settings set <key> <value>");
        return 0;
    }

    match argv[1] {
        "get" => {
            if argv.len() != 3 {
                api.write_line("usage: settings get <key>");
                return 1;
            }
            let mut line = InlineString::<96>::new();
            if write_setting_value(config, argv[2], &mut line).is_err() {
                api.write_line("settings: unsupported key");
                return 1;
            }
            api.write_line(line.as_str());
            0
        }
        "set" => {
            if argv.len() != 4 {
                api.write_line("usage: settings set <key> <value>");
                return 1;
            }
            if let Err(err) = apply_setting(&mut config, argv[2], argv[3]) {
                api.write_line(err);
                return 1;
            }

            let mut output = InlineString::<256>::new();
            write_settings_text(config, &mut output);
            if let Err(err) = api.write_file(SETTINGS_PATH, output.as_str()) {
                api.write_line(err.message());
                return 1;
            }

            api.write_line("settings updated");
            show_settings(api, config);
            0
        }
        _ => {
            api.write_line("usage: settings show | get <key> | set <key> <value>");
            1
        }
    }
}

fn video_player_main(argv: &[&str], api: &mut SysApi<'_>) -> i32 {
    let path = if argv.len() > 1 {
        argv[1]
    } else {
        DEMO_VIDEO_PATH
    };
    let mut scratch = InlineString::<2048>::new();
    let text = match api.read_path(path, &mut scratch) {
        Ok(text) => text,
        Err(err) => {
            api.write_line(err);
            return 1;
        }
    };

    let (title, fps, frames_blob) = parse_media_header(text);
    let frame_count = count_frames(frames_blob);
    if frame_count == 0 {
        api.write_line("video-player: no frames found");
        return 1;
    }

    let mut header = InlineString::<128>::new();
    let _ = core::fmt::write(
        &mut header,
        format_args!("video-player: {} ({}, {} fps)", path, frame_count, fps),
    );
    api.write_line(header.as_str());
    api.write_line(title);

    let frame_delay_ms = (1_000u32 / fps.max(1)).max(120);
    for (index, frame) in frames_blob.split("\n---\n").enumerate() {
        api.clear_screen();

        let mut line = InlineString::<96>::new();
        let _ = core::fmt::write(
            &mut line,
            format_args!("{}  frame {}/{}", title, index + 1, frame_count),
        );
        api.write_line(line.as_str());
        api.write_line(frame.trim_end_matches('\n'));
        api.sleep_ms(frame_delay_ms);
    }

    api.write_line("video-player: playback complete");
    0
}

fn audio_player_main(argv: &[&str], api: &mut SysApi<'_>) -> i32 {
    let path = if argv.len() > 1 {
        argv[1]
    } else {
        DEMO_AUDIO_PATH
    };
    let mut scratch = InlineString::<2048>::new();
    let text = match api.read_path(path, &mut scratch) {
        Ok(text) => text,
        Err(err) => {
            api.write_line(err);
            return 1;
        }
    };

    let (title, _volume, notes_blob) = parse_media_header(text);
    api.write_line("audio-player: safe PC-speaker backend");
    api.write_line(title);

    let mut notes = 0usize;
    for line in notes_blob.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let mut parts = line.split_whitespace();
        let Some(freq_text) = parts.next() else {
            continue;
        };
        let Some(duration_text) = parts.next() else {
            api.write_line("audio-player: malformed score line");
            crate::arch::speaker_stop();
            return 1;
        };
        let label = parts.next().unwrap_or("NOTE");

        let Ok(freq_hz) = parse_u32(freq_text) else {
            api.write_line("audio-player: invalid frequency");
            crate::arch::speaker_stop();
            return 1;
        };
        let Ok(duration_ms) = parse_u32(duration_text) else {
            api.write_line("audio-player: invalid duration");
            crate::arch::speaker_stop();
            return 1;
        };

        let mut status = InlineString::<96>::new();
        let _ = core::fmt::write(
            &mut status,
            format_args!("note {}: {}Hz {}ms", label, freq_hz, duration_ms),
        );
        api.write_line(status.as_str());

        if freq_hz == 0 {
            crate::arch::speaker_stop();
        } else {
            crate::arch::speaker_play(freq_hz);
        }
        api.sleep_ms(duration_ms);
        crate::arch::speaker_stop();
        api.sleep_ms(30);
        notes += 1;
    }

    crate::arch::speaker_stop();
    if notes == 0 {
        api.write_line("audio-player: no notes found");
        return 1;
    }

    api.write_line("audio-player: playback complete");
    0
}

fn hello_main(argv: &[&str], api: &mut SysApi<'_>) -> i32 {
    api.write_line("Rustix v-kernel user program: /bin/hello");
    api.write_line("model: proc service + vfs service + console service");
    api.write_line_num("pid=", api.pid() as i32);
    api.write_line_num("ppid=", api.ppid() as i32);
    api.write_line_num("tgid=", api.tgid() as i32);
    api.write_line_num("pgid=", api.pgid() as i32);
    api.write_line_num("sid=", api.sid() as i32);
    api.write_line_num("argc=", argv.len() as i32);

    for (index, arg) in argv.iter().enumerate() {
        let mut line = InlineString::<96>::new();
        let _ = core::fmt::write(&mut line, format_args!("argv[{}]={}", index, arg));
        api.write_line(line.as_str());
    }

    match api.read_file("/etc/motd") {
        Ok(text) => {
            api.write_line("--- /etc/motd ---");
            api.write(text);
            if !text.ends_with('\n') {
                api.write_line("");
            }
            api.write_line("--- end motd ---");
            0
        }
        Err(err) => {
            api.write_line(err);
            1
        }
    }
}

fn game_main(_argv: &[&str], api: &mut SysApi<'_>) -> i32 {
    let mut game = DungeonGame::new();
    let mut input = InlineString::<128>::new();

    api.write_line("Rustix v-kernel ASCII dungeon");
    api.write_line("Commands: north south west east map status help quit");
    game.render(api);

    loop {
        input.clear();
        let command = api.prompt("game> ", &mut input).trim();
        match command {
            "" => continue,
            "help" => {
                api.write_line("Move with: north south west east");
                api.write_line("Use: map, status, quit");
            }
            "map" => game.render(api),
            "status" => game.status(api),
            "north" | "up" | "w" => {
                if let Some(code) = game.step(0, -1, api) {
                    return code;
                }
            }
            "south" | "down" | "s" => {
                if let Some(code) = game.step(0, 1, api) {
                    return code;
                }
            }
            "west" | "left" | "a" => {
                if let Some(code) = game.step(-1, 0, api) {
                    return code;
                }
            }
            "east" | "right" | "d" => {
                if let Some(code) = game.step(1, 0, api) {
                    return code;
                }
            }
            "quit" | "exit" => {
                api.write_line("Leaving the dungeon.");
                return 0;
            }
            other => {
                let mut line = InlineString::<96>::new();
                let _ = core::fmt::write(&mut line, format_args!("unknown command: {}", other));
                api.write_line(line.as_str());
            }
        }
    }
}

struct DungeonGame {
    x: usize,
    y: usize,
    hp: i32,
    steps: usize,
}

impl DungeonGame {
    const WIDTH: usize = 5;
    const HEIGHT: usize = 5;
    const EXIT_X: usize = 4;
    const EXIT_Y: usize = 4;

    const fn new() -> Self {
        Self {
            x: 0,
            y: 0,
            hp: 3,
            steps: 0,
        }
    }

    fn step(&mut self, dx: isize, dy: isize, api: &mut SysApi<'_>) -> Option<i32> {
        let Some(next_x) = self.x.checked_add_signed(dx) else {
            api.write_line("Edge of map.");
            return None;
        };
        let Some(next_y) = self.y.checked_add_signed(dy) else {
            api.write_line("Edge of map.");
            return None;
        };
        if next_x >= Self::WIDTH || next_y >= Self::HEIGHT {
            api.write_line("Edge of map.");
            return None;
        }
        if tile_at(next_x, next_y) == '#' {
            api.write_line("A wall blocks the path.");
            self.render(api);
            return None;
        }

        self.x = next_x;
        self.y = next_y;
        self.steps += 1;

        match tile_at(self.x, self.y) {
            '!' => {
                self.hp -= 1;
                api.write_line("Trap! You lost 1 HP.");
            }
            _ => api.write_line("You moved safely."),
        }

        self.status(api);
        self.render(api);

        if self.hp <= 0 {
            api.write_line("Game over.");
            return Some(1);
        }
        if self.x == Self::EXIT_X && self.y == Self::EXIT_Y {
            api.write_line("Victory. You reached the exit.");
            return Some(0);
        }
        None
    }

    fn status(&self, api: &mut SysApi<'_>) {
        let mut line = InlineString::<96>::new();
        let _ = core::fmt::write(
            &mut line,
            format_args!(
                "pos=({}, {}) hp={} steps={}",
                self.x, self.y, self.hp, self.steps
            ),
        );
        api.write_line(line.as_str());
    }

    fn render(&self, api: &mut SysApi<'_>) {
        for y in 0..Self::HEIGHT {
            let mut line = InlineString::<32>::new();
            for x in 0..Self::WIDTH {
                let ch = if x == self.x && y == self.y {
                    '@'
                } else if x == 0 && y == 0 {
                    'S'
                } else if x == Self::EXIT_X && y == Self::EXIT_Y {
                    'E'
                } else {
                    tile_at(x, y)
                };
                let _ = line.push_byte(ch as u8);
                let _ = line.push_byte(b' ');
            }
            api.write_line(line.as_str());
        }
        api.write_line("Legend: @ you, S start, E exit, # wall, ! trap, . floor");
    }
}

fn tile_at(x: usize, y: usize) -> char {
    match (x, y) {
        (1, 0) | (3, 0) | (3, 1) | (1, 2) | (1, 3) | (3, 3) => '#',
        (2, 1) | (4, 1) | (2, 3) | (0, 4) => '!',
        _ => '.',
    }
}

fn parse_i32(text: &str) -> Result<i32, ()> {
    let mut value = 0i32;
    let mut negative = false;
    for (index, byte) in text.bytes().enumerate() {
        if index == 0 && byte == b'-' {
            negative = true;
            continue;
        }
        if !byte.is_ascii_digit() {
            return Err(());
        }
        value = value
            .saturating_mul(10)
            .saturating_add((byte - b'0') as i32);
    }
    Ok(if negative { -value } else { value })
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

fn parse_settings(text: &str) -> SettingsConfig {
    let mut config = SettingsConfig::defaults();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let _ = apply_setting(&mut config, key.trim(), value.trim());
    }
    config
}

fn apply_setting(config: &mut SettingsConfig, key: &str, value: &str) -> Result<(), &'static str> {
    match key {
        "brightness" => {
            let value = parse_percent(value)?;
            config.brightness = value;
            Ok(())
        }
        "volume" => {
            let value = parse_percent(value)?;
            config.volume = value;
            Ok(())
        }
        "ui_scale" => {
            let value = parse_u32(value).map_err(|_| "settings: ui_scale must be 1..4")?;
            if !(1..=4).contains(&value) {
                return Err("settings: ui_scale must be 1..4");
            }
            config.ui_scale = value as u8;
            Ok(())
        }
        "theme" => {
            config.theme = match value {
                "graphite" => Theme::Graphite,
                "sunrise" => Theme::Sunrise,
                "glacier" => Theme::Glacier,
                _ => return Err("settings: theme must be graphite|sunrise|glacier"),
            };
            Ok(())
        }
        _ => Err("settings: unsupported key"),
    }
}

fn parse_percent(value: &str) -> Result<u8, &'static str> {
    let value = parse_u32(value).map_err(|_| "settings: value must be 0..100")?;
    if value > 100 {
        return Err("settings: value must be 0..100");
    }
    Ok(value as u8)
}

fn write_setting_value(
    config: SettingsConfig,
    key: &str,
    out: &mut InlineString<96>,
) -> Result<(), ()> {
    out.clear();
    match key {
        "brightness" => core::fmt::write(out, format_args!("brightness={}", config.brightness)),
        "volume" => core::fmt::write(out, format_args!("volume={}", config.volume)),
        "ui_scale" => core::fmt::write(out, format_args!("ui_scale={}", config.ui_scale)),
        "theme" => core::fmt::write(out, format_args!("theme={}", config.theme.as_str())),
        _ => return Err(()),
    }
    .map_err(|_| ())
}

fn show_settings(api: &mut SysApi<'_>, config: SettingsConfig) {
    let mut line = InlineString::<96>::new();
    line.set("settings file: /etc/rustix/settings.conf");
    api.write_line(line.as_str());

    line.clear();
    let _ = core::fmt::write(&mut line, format_args!("brightness={}", config.brightness));
    api.write_line(line.as_str());

    line.clear();
    let _ = core::fmt::write(&mut line, format_args!("volume={}", config.volume));
    api.write_line(line.as_str());

    line.clear();
    let _ = core::fmt::write(&mut line, format_args!("ui_scale={}", config.ui_scale));
    api.write_line(line.as_str());

    line.clear();
    let _ = core::fmt::write(&mut line, format_args!("theme={}", config.theme.as_str()));
    api.write_line(line.as_str());
}

fn write_settings_text(config: SettingsConfig, out: &mut InlineString<256>) {
    out.clear();
    let _ = core::fmt::write(
        out,
        format_args!(
            "brightness={}\nvolume={}\nui_scale={}\ntheme={}\n",
            config.brightness,
            config.volume,
            config.ui_scale,
            config.theme.as_str()
        ),
    );
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

fn count_frames(text: &str) -> usize {
    text.split("\n---\n")
        .filter(|frame| !frame.trim().is_empty())
        .count()
}
