use directories::ProjectDirs;
use figlet_rs::FIGfont;
use humantime::parse_duration;
use iocraft::prelude::*;
use rodio::{OutputStream, Sink};
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

const FIGLET_MIN_WIDTH: usize = 60;

fn find_sound_file() -> Option<PathBuf> {
    if PathBuf::from("sound.mp3").exists() {
        return Some(PathBuf::from("sound.mp3"));
    }


    if let Some(proj_dirs) = ProjectDirs::from("com", "brownbishop", "timer") {
        let data_path = proj_dirs.data_dir().join("sound.mp3");
        if data_path.exists() {
            return Some(data_path);
        }
    }

    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let exe_sound = exe_dir.join("sound.mp3");
            if exe_sound.exists() {
                return Some(exe_sound);
            }
        }
    }

    None
}

fn format_duration_hms(duration: Duration) -> String {
    let total_secs = duration.as_secs();
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;
    format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
}

fn format_duration_figlet(hms: &str) -> Option<String> {
    Some(FIGfont::standard().ok()?.convert(hms)?.to_string())
}

#[derive(Default, Props)]
struct CounterProps {
    duration: Duration,
    sound_file: PathBuf,
}

#[component]
fn Timer(props: &mut CounterProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let mut remaining = hooks.use_state(|| props.duration);
    let mut playing = hooks.use_state(|| false);
    let mut should_exit = hooks.use_state(|| false);
    let mut system = hooks.use_context_mut::<SystemContext>();
    let (width, height) = hooks.use_terminal_size();
    let stop_signal = hooks.use_ref(|| Arc::new(AtomicBool::new(false)));
    let finished_signal = hooks.use_ref(|| Arc::new(AtomicBool::new(false)));

    let sound_file = props.sound_file.clone();

    hooks.use_future(async move {
        loop {
            smol::Timer::after(Duration::from_millis(1000)).await;
            if !playing.get() && !remaining.get().is_zero() {
                remaining.set(remaining.get().saturating_sub(Duration::from_secs(1)));
            }
        }
    });

    hooks.use_future(async move {
        loop {
            smol::Timer::after(Duration::from_millis(100)).await;
            if remaining.get().is_zero() && !playing.get() {
                let stop = stop_signal.read().clone();
                let finished = finished_signal.read().clone();
                let sound_file = sound_file.clone();
                thread::spawn(move || {
                    if let Ok((stream, stream_handle)) = OutputStream::try_default() {
                        if let Ok(sink) = Sink::try_new(&stream_handle) {
                            if let Ok(file) = File::open(&sound_file) {
                                if let Ok(source) = rodio::Decoder::new(BufReader::new(file)) {
                                    sink.append(source);
                                    while !sink.empty() && !stop.load(Ordering::Relaxed) {
                                        thread::sleep(Duration::from_millis(50));
                                    }
                                    sink.stop();
                                }
                            }
                            drop(sink);
                        }
                        drop(stream);
                    }
                    finished.store(true, Ordering::Relaxed);
                });
                playing.set(true);
            }
            if playing.get() && finished_signal.read().load(Ordering::Relaxed) {
                should_exit.set(true);
            }
        }
    });

    hooks.use_terminal_events({
        let stop_signal = stop_signal.clone();
        move |event| match event {
            TerminalEvent::Key(KeyEvent { code, kind, .. }) if kind != KeyEventKind::Release => {
                match code {
                    KeyCode::Char('q') => {
                        stop_signal.read().store(true, Ordering::Relaxed);
                        should_exit.set(true);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    });

    if should_exit.get() {
        system.exit();
    }

    let text_color = if playing.get() { Color::Red } else { Color::Blue };

    let hms = format_duration_hms(remaining.get());
    let use_figlet = usize::from(width) >= FIGLET_MIN_WIDTH;
    let display_text = if use_figlet {
        format_duration_figlet(&hms).unwrap_or(hms)
    } else {
        hms
    };

    element! {
        View(
            width,
            height,
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
        ) {
            View(
                border_style: BorderStyle::Round,
                border_color: Color::Green,
            ) {
                Text(color: text_color, content: display_text)
            }
        }
    }
}

fn main() {
    let duration = std::env::args()
        .nth(1)
        .map(|s| parse_duration(&s).expect("Invalid duration format. Examples: '30s', '1m', '1h30m'"))
        .unwrap_or(Duration::from_secs(60));

    let sound_file = find_sound_file().expect("Could not find sound.mp3 in any of these locations:\n  - ./sound.mp3 (current directory)\n  - <data_dir>/timer/sound.mp3\n  - <executable_dir>/sound.mp3");

    smol::block_on(element!(Timer(duration, sound_file)).render_loop()).unwrap()
}
