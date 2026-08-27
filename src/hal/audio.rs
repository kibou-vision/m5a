//! マイクとスピーカー。
//!
//! 録音と再生はそれぞれ専用の仕事で回す。描画と同じ流れで読むと、
//! 画面を描いている間の音を取りこぼして声が途切れる。
//!
//! コーデックの初期化順序やレジスタ列は BSP が引き受けるため、
//! ここでは開いて読み書きするだけにする。

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, RecvTimeoutError, SyncSender};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use esp_idf_svc::sys::{bsp, esp};
use m5a_core::audio as waveform;
use m5a_core::config::AudioFormat;

/// I2S を流す標本化周波数。マイクとスピーカーは端子を共有するため揃える。
const DEVICE_RATE: u32 = 16_000;
/// 一度に扱う長さ。短いほど声の出だしが早く相手に届く。
const CHUNK_MS: usize = 20;
const CHUNK_SAMPLES: usize = DEVICE_RATE as usize * CHUNK_MS / 1_000;
/// 待ち行列の深さ。溢れたら捨てて、遅れを溜めないようにする。
const QUEUE_DEPTH: usize = 24;
/// 音声を扱う仕事の作業領域。
/// 内部メモリは SD カードの DMA バッファと取り合いになるため切り詰める。
const THREAD_STACK: usize = 5 * 1_024;
/// 読み取りに失敗したときに待つ時間。
const RETRY_DELAY: Duration = Duration::from_millis(20);

/// 鳴らす音が無いときに流す無音の長さ。
///
/// I2S は送るものが無いとクロックを止めてしまい、アンプの PLL が外れて
/// 次に音を送っても鳴らなくなる。黙っている間も無音を流し続けて防ぐ。
const IDLE_SILENCE: Duration = Duration::from_millis(40);

/// マイクを読み始めてからクロックが安定するまでの待ち。
const CLOCK_SETTLE: Duration = Duration::from_millis(200);

/// スピーカーの音量（百分率）。既定のままだと鳴らないため必ず設定する。
const SPEAKER_VOLUME: i32 = 80;
/// マイクの入力利得（dB）。小さいと子どもの声を拾えない。
const MICROPHONE_GAIN_DB: f32 = 30.0;

/// コーデックの取っ手。
///
/// それぞれの取っ手をひとつの仕事の中だけで使うため、別の仕事へ渡してよい。
struct Codec(bsp::esp_codec_dev_handle_t);

// 安全性: マイクと再生でそれぞれ別の取っ手を持ち、生成後は各仕事の中でしか触らない。
unsafe impl Send for Codec {}

/// 開いているマイクとスピーカー。
pub struct Audio {
    /// 録音した音声。送信できる形になっている。
    captured: Receiver<Vec<u8>>,
    /// 再生を待っている音声。
    to_play: SyncSender<Vec<u8>>,
    capturing: Arc<AtomicBool>,
    /// いま鳴っている音の大きさ。口の開きに使う。
    level: Arc<AtomicU8>,
}

impl Audio {
    /// マイクとスピーカーを開き、読み書きの仕事を始める。
    ///
    /// 順序に理由がある。AW88298 は設定するときに BCK が流れていないと
    /// PLL がロックせず、以後どれだけ書き込んでも無音のままになる。
    /// そこで先にマイクを開いて読み始め、クロックが出ている状態を作ってから
    /// スピーカーを設定する。給電は BSP のコーデック初期化が行う。
    pub fn start(format: AudioFormat) -> Result<Self> {
        // 既定の設定で全二重に開く。標本化周波数はコーデック側で決める。
        esp!(unsafe { bsp::bsp_audio_init(core::ptr::null()) }).context("音声を初期化できません")?;

        let microphone = open_device(
            unsafe { bsp::bsp_audio_codec_microphone_init() },
            "マイクを開けません",
        )?;
        set_gain(&microphone, MICROPHONE_GAIN_DB);

        let capturing = Arc::new(AtomicBool::new(false));
        let (captured_tx, captured) = sync_channel(QUEUE_DEPTH);
        spawn_capture(microphone, format, capturing.clone(), captured_tx)?;

        // 読み取りが始まってクロックが安定するのを待つ。
        thread::sleep(CLOCK_SETTLE);

        let speaker = open_device(
            unsafe { bsp::bsp_audio_codec_speaker_init() },
            "スピーカーを開けません",
        )?;
        set_volume(&speaker, SPEAKER_VOLUME);

        let level = Arc::new(AtomicU8::new(0));
        let (to_play, playing) = sync_channel(QUEUE_DEPTH);
        spawn_playback(speaker, format, level.clone(), playing)?;

        Ok(Self {
            captured,
            to_play,
            capturing,
            level,
        })
    }

    /// 録音した音を送り出し始める。
    pub fn begin_capture(&self) {
        self.capturing.store(true, Ordering::Relaxed);
    }

    /// 録音を止める。マイク自体は読み続け、I2S のクロックを保つ。
    pub fn end_capture(&self) {
        self.capturing.store(false, Ordering::Relaxed);
    }

    /// 送るべき録音を1つ取り出す。
    pub fn take_captured(&self) -> Option<Vec<u8>> {
        self.captured.try_recv().ok()
    }

    /// 応答の音声を鳴らす。追いつかないときは捨てて遅れを溜めない。
    pub fn play(&self, audio: Vec<u8>) {
        if self.to_play.try_send(audio).is_err() {
            log::debug!("再生が追いつかないため音を捨てました");
        }
    }

    /// いま鳴っている音の大きさ。
    pub fn level(&self) -> u8 {
        self.level.load(Ordering::Relaxed)
    }

    /// 鳴らし終えたことにする。口を閉じるために音量を戻す。
    pub fn silence(&self) {
        self.level.store(0, Ordering::Relaxed);
    }
}

fn open_device(handle: bsp::esp_codec_dev_handle_t, whats_wrong: &str) -> Result<Codec> {
    if handle.is_null() {
        bail!("{whats_wrong}");
    }

    let mut wanted = bsp::esp_codec_dev_sample_info_t {
        bits_per_sample: 16,
        channel: 1,
        sample_rate: DEVICE_RATE,
        ..Default::default()
    };

    let result = unsafe { bsp::esp_codec_dev_open(handle, &mut wanted) };
    if result != 0 {
        bail!("{whats_wrong}: {result}");
    }

    Ok(Codec(handle))
}

/// スピーカーの音量を決める。
fn set_volume(speaker: &Codec, percent: i32) {
    let result = unsafe { bsp::esp_codec_dev_set_out_vol(speaker.0, percent) };
    if result != 0 {
        log::warn!("スピーカーの音量を設定できません: {result}");
    } else {
        log::info!("スピーカーの音量: {percent}%");
    }
}

/// マイクの感度を決める。
fn set_gain(microphone: &Codec, decibels: f32) {
    let result = unsafe { bsp::esp_codec_dev_set_in_gain(microphone.0, decibels) };
    if result != 0 {
        log::warn!("マイクの感度を設定できません: {result}");
    } else {
        log::info!("マイクの感度: {decibels} dB");
    }
}

fn spawn_capture(
    microphone: Codec,
    format: AudioFormat,
    capturing: Arc<AtomicBool>,
    captured: SyncSender<Vec<u8>>,
) -> Result<()> {
    thread::Builder::new()
        .stack_size(THREAD_STACK)
        .spawn(move || {
            let microphone = microphone;
            let mut samples = vec![0_i16; CHUNK_SAMPLES];

            loop {
                let bytes = (samples.len() * 2) as i32;
                let result = unsafe {
                    bsp::esp_codec_dev_read(microphone.0, samples.as_mut_ptr().cast(), bytes)
                };

                if result != 0 {
                    thread::sleep(RETRY_DELAY);
                    continue;
                }

                // 録音していない間も読み続ける。止めると I2S のクロックが途切れる。
                if !capturing.load(Ordering::Relaxed) {
                    continue;
                }

                if captured.try_send(encode(&samples, format)).is_err() {
                    log::debug!("送信が追いつかないため録音を捨てました");
                }
            }
        })
        .context("録音の仕事を作れません")?;

    Ok(())
}

fn spawn_playback(
    speaker: Codec,
    format: AudioFormat,
    level: Arc<AtomicU8>,
    playing: Receiver<Vec<u8>>,
) -> Result<()> {
    thread::Builder::new()
        .stack_size(THREAD_STACK)
        .spawn(move || {
            let speaker = speaker;

            let mut played = 0_usize;
            let silence_samples = DEVICE_RATE as usize * IDLE_SILENCE.as_millis() as usize / 1_000;

            loop {
                let mut samples = match playing.recv_timeout(IDLE_SILENCE) {
                    Ok(chunk) => {
                        played += 1;
                        let samples = decode(&chunk, format);
                        level.store(waveform::measure_level(&samples), Ordering::Relaxed);

                        if played % 10 == 1 {
                            let peak =
                                samples.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0);
                            log::info!(
                                "再生 {played}個目: 届いた {} バイト → {} 標本 / 最大振幅 {peak}",
                                chunk.len(),
                                samples.len()
                            );
                        }
                        samples
                    }
                    // 黙っている間もクロックを保つため無音を送る。
                    Err(RecvTimeoutError::Timeout) => {
                        level.store(0, Ordering::Relaxed);
                        vec![0_i16; silence_samples]
                    }
                    Err(RecvTimeoutError::Disconnected) => break,
                };

                let bytes = (samples.len() * 2) as i32;
                let result = unsafe {
                    bsp::esp_codec_dev_write(speaker.0, samples.as_mut_ptr().cast(), bytes)
                };

                if result != 0 {
                    log::warn!("音を出せません: {result}");
                }
            }
        })
        .context("再生の仕事を作れません")?;

    Ok(())
}

/// 録音を送れる形にする。
fn encode(samples: &[i16], format: AudioFormat) -> Vec<u8> {
    let resampled = waveform::resample_linear(samples, DEVICE_RATE, format.sample_rate());

    match format {
        AudioFormat::Ulaw => waveform::encode_ulaw_block(&resampled),
        AudioFormat::Pcm16 => resampled
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect(),
    }
}

/// 届いた音声を鳴らせる形にする。
fn decode(audio: &[u8], format: AudioFormat) -> Vec<i16> {
    let samples = match format {
        AudioFormat::Ulaw => waveform::decode_ulaw_block(audio),
        AudioFormat::Pcm16 => audio
            .chunks_exact(2)
            .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
            .collect(),
    };

    waveform::resample_linear(&samples, format.sample_rate(), DEVICE_RATE)
}
