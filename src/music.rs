use std::path::PathBuf;
use std::sync::mpsc::{sync_channel, Receiver};
use std::thread;

use rodio::{OutputStream, Sink, Source};
use xmrs::core::module::Module;
use xmrsplayer::xmrsplayer::XmrsPlayer;

const FALLBACK_VOLUME: f32 = 0.42;
const SAMPLE_RATE: u32 = 44100;
const BUFFER_SAMPLES: usize = 8192;

fn shuffle(files: &mut Vec<PathBuf>) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9E37_79B9_7F4A_7C15);
    let mut seed = now ^ (std::process::id() as u64).rotate_left(32);
    if seed == 0 {
        seed = 0xD1B5_4A32_D192_ED03;
    }
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let n = files.len();
    for i in (1..n).rev() {
        let j = (next() % (i as u64 + 1)) as usize;
        files.swap(i, j);
    }
}

pub struct MusicPlayer {
    _stream: OutputStream,
    sink: Sink,
    files: Vec<PathBuf>,
    index: usize,
    muted: bool,
}

impl MusicPlayer {
    pub fn new(mut files: Vec<PathBuf>) -> Result<Self, String> {
        if files.is_empty() {
            return Err("music list is empty".to_string());
        }
        shuffle(&mut files);
        let (stream, handle) =
            OutputStream::try_default().map_err(|e| format!("no audio device: {e}"))?;
        let sink = Sink::try_new(&handle).map_err(|e| format!("no audio device: {e}"))?;

        let mut player = Self {
            _stream: stream,
            sink,
            files,
            index: 0,
            muted: false,
        };
        player.sink.set_volume(FALLBACK_VOLUME);
        player.play_at(0);
        Ok(player)
    }

    fn play_at(&mut self, idx: usize) {
        if self.files.is_empty() {
            return;
        }
        let n = self.files.len();
        for k in 0..n {
            let i = (idx + k) % n;
            let Some(data) = std::fs::read(&self.files[i]).ok() else {
                continue;
            };
            let Ok(module) = Module::load(&data) else {
                continue;
            };
            self.index = i;
            self.sink.stop();
            self.sink.append(TrackerLoop::new(module));
            self.sink.play();
            return;
        }
    }

    pub fn next(&mut self) {
        self.play_at((self.index + 1) % self.files.len());
    }

    pub fn toggle_mute(&mut self) -> bool {
        self.muted = !self.muted;
        self.sink
            .set_volume(if self.muted { 0.0 } else { FALLBACK_VOLUME });
        self.muted
    }

    pub fn is_muted(&self) -> bool {
        self.muted
    }

    pub fn track_count(&self) -> usize {
        self.files.len()
    }

    pub fn now_playing(&self) -> String {
        self.files
            .get(self.index)
            .and_then(|f| f.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "no track".to_string())
    }
}

struct TrackerLoop {
    receiver: Receiver<i16>,
}

impl TrackerLoop {
    fn new(module: Module) -> Self {
        let (tx, receiver) = sync_channel::<i16>(BUFFER_SAMPLES);
        thread::spawn(move || {
            let mut player = XmrsPlayer::new(&module, SAMPLE_RATE, 0);
            loop {
                match player.next() {
                    Some(sample) => {
                        if tx.send(sample).is_err() {
                            break;
                        }
                    }
                    None => {
                        player = XmrsPlayer::new(&module, SAMPLE_RATE, 0);
                    }
                }
            }
        });
        Self { receiver }
    }
}

impl Iterator for TrackerLoop {
    type Item = i16;

    fn next(&mut self) -> Option<Self::Item> {
        self.receiver.recv().ok()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (1024, None)
    }
}

impl Source for TrackerLoop {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> u16 {
        2
    }

    fn sample_rate(&self) -> u32 {
        SAMPLE_RATE
    }

    fn total_duration(&self) -> Option<std::time::Duration> {
        None
    }
}