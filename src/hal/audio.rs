//! マイクとスピーカー。
//!
//! 録音と再生はそれぞれ専用の仕事で回す。描画と同じ流れで読むと、
//! 画面を描いている間の音を取りこぼして声が途切れる。
//!
//! コーデックの初期化順序やレジスタ列は BSP が引き受けるため、
//! ここでは開いて読み書きするだけにする。

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
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
/// 録音の待ち行列の深さ。溢れたら捨てて、遅れを溜めないようにする。
const CAPTURE_QUEUE_DEPTH: usize = 24;
/// 再生の待ち行列の深さ。
///
/// サーバーは音声をリアルタイムより速く送ってくることがあり、
/// 短いと数百ミリ秒の話でも一瞬で溢れて音を取りこぼす。PSRAM に余裕が
/// あるため、長い応答でも溜めておけるだけの深さを持たせる（20ms×750=15秒分）。
const PLAYBACK_QUEUE_DEPTH: usize = 750;
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

/// AW88298 の音量レジスタ。上位バイトが減衰量で、0 が最大、0.5dB 刻み。
const VOLUME_REGISTER: i32 = 0x0C;
/// 音量レジスタの下位バイト。コーデックの既定値に合わせる。
const VOLUME_FLAGS: i32 = 0x64;
/// BSP がアンプ利得として決め打ちしている値（dB）。
const BSP_PA_GAIN_DB: i32 = 15;
/// 減衰量レジスタは 0.5dB ごとに 1 進む。
const STEPS_PER_DB: i32 = 2;
/// マイクの入力利得（dB）。小さいと子どもの声を拾えない。
const MICROPHONE_GAIN_DB: f32 = 36.0;

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
    /// いま拾っているマイクの音の大きさ。声の区切りの判定に使う。
    input_level: Arc<AtomicU8>,
    /// いま鳴っている音の大きさ。口の開きに使う。
    level: Arc<AtomicU8>,
    /// 実際に応答の音声を鳴らしている最中かどうか。
    ///
    /// サーバーの「応答が終わった」知らせは、まだ再生しきっていない分が
    /// 待ち行列に残っている段階で届くため、口を閉じる判断はこちらで行う。
    speaking: Arc<AtomicBool>,
    /// 再生が追いつかず捨てた回数。
    dropped: Arc<AtomicUsize>,
    /// 立てると、再生の仕事が待ち行列に溜まっている分を鳴らさずに捨てる。
    /// 割り込みで応答を打ち切ったとき、溜めておいた分が後から鳴るのを防ぐ。
    flush: Arc<AtomicBool>,
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
        let input_level = Arc::new(AtomicU8::new(0));
        let (captured_tx, captured) = sync_channel(CAPTURE_QUEUE_DEPTH);
        spawn_capture(
            microphone,
            format,
            capturing.clone(),
            input_level.clone(),
            captured_tx,
        )?;

        // 読み取りが始まってクロックが安定するのを待つ。
        thread::sleep(CLOCK_SETTLE);

        let speaker = open_device(
            unsafe { bsp::bsp_audio_codec_speaker_init() },
            "スピーカーを開けません",
        )?;
        set_volume(&speaker, SPEAKER_VOLUME);
        restore_amplifier_gain(&speaker);

        let level = Arc::new(AtomicU8::new(0));
        let speaking = Arc::new(AtomicBool::new(false));
        let flush = Arc::new(AtomicBool::new(false));
        let (to_play, playing) = sync_channel(PLAYBACK_QUEUE_DEPTH);
        spawn_playback(speaker, format, level.clone(), speaking.clone(), flush.clone(), playing)?;

        Ok(Self {
            captured,
            to_play,
            capturing,
            input_level,
            level,
            speaking,
            dropped: Arc::new(AtomicUsize::new(0)),
            flush,
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

    /// 前の録音の残りを捨てる。新しく録音を始める前に呼び、
    /// 前回の音が新しい発話の頭に混ざらないようにする。
    pub fn discard_captured(&self) {
        while self.captured.try_recv().is_ok() {}
    }

    /// いまマイクが拾っている音の大きさ。声の区切りの判定に使う。
    pub fn input_level(&self) -> u8 {
        self.input_level.load(Ordering::Relaxed)
    }

    /// 応答の音声を鳴らす。追いつかないときは捨てて遅れを溜めない。
    pub fn play(&self, audio: Vec<u8>) {
        if self.to_play.try_send(audio).is_err() {
            let total = self.dropped.fetch_add(1, Ordering::Relaxed) + 1;
            let free = unsafe { esp_idf_svc::sys::esp_get_free_heap_size() };
            log::warn!("再生が追いつかないため音を捨てました（累計{total}回、空きヒープ{free}バイト）");
        }
    }

    /// いま鳴っている音の大きさ。
    pub fn level(&self) -> u8 {
        self.level.load(Ordering::Relaxed)
    }

    /// 応答の音声を、まだ鳴らしきっていないか。
    pub fn is_speaking(&self) -> bool {
        self.speaking.load(Ordering::Relaxed)
    }

    /// 割り込みで応答を打ち切る。口を閉じ、溜まっている分も鳴らさず捨てる。
    ///
    /// ここでだけ待ち行列を空にする。捨てずに残すと、打ち切ったはずの
    /// 古い応答が次の会話に被さって鳴ってしまう。
    pub fn interrupt(&self) {
        self.level.store(0, Ordering::Relaxed);
        self.flush.store(true, Ordering::Relaxed);
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

/// BSP が差し引いているアンプ利得を取り戻す。
///
/// AW88298 のドライバは要求音量から `pa_gain` を引いてからレジスタに落とす。
/// BSP がこれを 15dB で決め打ちしており、指定した音量よりかなり小さくなる。
/// BSP のコーデック生成には手を入れられないため、設定後にレジスタを
/// 直接書き戻して目減りを打ち消す。
fn restore_amplifier_gain(speaker: &Codec) {
    let mut current = 0;
    let read = unsafe { bsp::esp_codec_dev_read_reg(speaker.0, VOLUME_REGISTER, &mut current) };
    if read != 0 {
        log::warn!("音量レジスタを読めません: {read}");
        return;
    }

    let attenuation = (current >> 8) & 0xFF;
    let restored = (attenuation - BSP_PA_GAIN_DB * STEPS_PER_DB).max(0);

    let written = unsafe {
        bsp::esp_codec_dev_write_reg(speaker.0, VOLUME_REGISTER, restored << 8 | VOLUME_FLAGS)
    };
    if written != 0 {
        log::warn!("音量レジスタを書けません: {written}");
        return;
    }

    log::info!(
        "アンプの利得を戻しました: 減衰 {attenuation} → {restored}（{} dB ぶん）",
        BSP_PA_GAIN_DB
    );
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
    input_level: Arc<AtomicU8>,
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
                let is_capturing = capturing.load(Ordering::Relaxed);
                input_level.store(
                    if is_capturing { waveform::measure_level(&samples) } else { 0 },
                    Ordering::Relaxed,
                );
                if !is_capturing {
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
    speaking: Arc<AtomicBool>,
    flush: Arc<AtomicBool>,
    playing: Receiver<Vec<u8>>,
) -> Result<()> {
    thread::Builder::new()
        .stack_size(THREAD_STACK)
        .spawn(move || {
            let speaker = speaker;

            let mut played = 0_usize;
            let silence_samples = DEVICE_RATE as usize * IDLE_SILENCE.as_millis() as usize / 1_000;

            loop {
                if flush.swap(false, Ordering::Relaxed) {
                    while playing.try_recv().is_ok() {}
                    level.store(0, Ordering::Relaxed);
                    speaking.store(false, Ordering::Relaxed);
                }

                let mut samples = match playing.recv_timeout(IDLE_SILENCE) {
                    Ok(chunk) => {
                        played += 1;
                        let samples = decode(&chunk, format);
                        level.store(waveform::measure_level(&samples), Ordering::Relaxed);
                        speaking.store(true, Ordering::Relaxed);

                        if played % 50 == 1 {
                            let peak =
                                samples.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0);
                            log::debug!(
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
                        speaking.store(false, Ordering::Relaxed);
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
