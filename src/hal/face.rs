//! 顔を LVGL の部品として画面に置く。
//!
//! 部品は起動時に一度だけ作り、以後は位置・大きさ・色を変えるだけにする。
//! 毎回作り直すと画面全体が描き直され、まばたきが点滅に見えてしまう。
//! どこに何を置くかは [`m5a_core::layout`] が決め、ここは反映だけを行う。

use esp_idf_svc::sys::bsp;
use m5a_core::layout::{self, Brow, Color, FaceLayout, Microphone, Mouth, Rect};

/// 既定の見た目を指す選択子。
const MAIN_PART: u32 = 0;
/// 弧の進んだ部分を指す選択子。
const INDICATOR_PART: u32 = bsp::lv_part_t_LV_PART_INDICATOR;
/// つまみを指す選択子。
const KNOB_PART: u32 = bsp::lv_part_t_LV_PART_KNOB;
/// 完全に不透明。
const OPAQUE: u8 = 255;
/// 眉の線の太さ。
const BROW_THICKNESS: i32 = 6;
/// 読み込みの印の太さ。
const SPINNER_THICKNESS: i32 = 8;
/// 読み込みの印が一周する時間と、弧の長さ。
const SPINNER_PERIOD_MS: u32 = 1_000;
const SPINNER_ARC_DEGREES: u32 = 60;
/// 読み込みの印の大きさ。
const SPINNER_SIZE: i32 = 72;

/// 画面に置かれた顔。
pub struct FaceView {
    /// 顔の部品をまとめる入れ物。
    ///
    /// 読み込み中はこれごと隠す。部品を一つずつ隠すと、
    /// 見せ直すときに取りこぼす（実際に口だけ消えたことがある）。
    face: *mut bsp::lv_obj_t,
    left_eye: *mut bsp::lv_obj_t,
    right_eye: *mut bsp::lv_obj_t,
    left_pupil: *mut bsp::lv_obj_t,
    right_pupil: *mut bsp::lv_obj_t,
    mouth: *mut bsp::lv_obj_t,
    /// 笑った口にするため、口の上半分を背景色で隠す板。
    mouth_cover: *mut bsp::lv_obj_t,
    left_brow: *mut bsp::lv_obj_t,
    right_brow: *mut bsp::lv_obj_t,
    /// 眉の線の座標。LVGL が参照し続けるので、動かない場所に置いておく。
    brow_points: Box<[[bsp::lv_point_precise_t; 2]; 2]>,
    button: *mut bsp::lv_obj_t,
    /// マイクの絵。頭・支柱・台座の3つで組む。
    mic_head: *mut bsp::lv_obj_t,
    mic_stem: *mut bsp::lv_obj_t,
    mic_base: *mut bsp::lv_obj_t,
    /// 立ち上げ中に出す読み込みの印。
    spinner: *mut bsp::lv_obj_t,
    applied: Option<FaceLayout>,
}

impl FaceView {
    /// 顔の部品を作る。呼ぶ前に画面の鍵を取っておくこと。
    pub fn create() -> Self {
        unsafe {
            let screen = bsp::lv_screen_active();
            let face = make_group(screen);

            let left_eye = make_panel(face);
            let right_eye = make_panel(face);
            let left_pupil = make_panel(face);
            let right_pupil = make_panel(face);
            let mouth = make_panel(face);
            let mouth_cover = make_panel(face);
            let left_brow = make_line(face);
            let right_brow = make_line(face);
            let button = make_panel(face);
            let mic_head = make_panel(face);
            let mic_stem = make_panel(face);
            let mic_base = make_panel(face);
            let spinner = make_spinner(screen);

            for part in [left_eye, right_eye, left_pupil, right_pupil, button, mic_head] {
                round(part);
            }
            paint(left_pupil, layout::pupil_color());
            paint(right_pupil, layout::pupil_color());
            paint(left_eye, layout::eye_color());
            paint(right_eye, layout::eye_color());
            paint(mouth, layout::mouth_color());
            for part in [mic_head, mic_stem, mic_base] {
                paint(part, layout::button_mark_color());
            }

            for brow in [left_brow, right_brow] {
                bsp::lv_obj_set_style_line_color(brow, color_of(layout::brow_color()), MAIN_PART);
                bsp::lv_obj_set_style_line_width(brow, BROW_THICKNESS, MAIN_PART);
                bsp::lv_obj_set_style_line_rounded(brow, true, MAIN_PART);
            }

            Self {
                face,
                left_eye,
                right_eye,
                left_pupil,
                right_pupil,
                mouth,
                mouth_cover,
                left_brow,
                right_brow,
                brow_points: Box::new([[bsp::lv_point_precise_t { x: 0, y: 0 }; 2]; 2]),
                button,
                mic_head,
                mic_stem,
                mic_base,
                spinner,
                applied: None,
            }
        }
    }

    /// 配置を画面に反映する。前回と同じなら何もしない。
    pub fn apply(&mut self, layout: &FaceLayout) {
        if self.applied.as_ref() == Some(layout) {
            return;
        }

        unsafe { self.write(layout) };
        self.applied = Some(*layout);
    }

    unsafe fn write(&mut self, layout: &FaceLayout) {
        bsp::lv_obj_set_style_bg_color(
            bsp::lv_screen_active(),
            color_of(layout.background),
            MAIN_PART,
        );

        // 立ち上げ中は顔を見せず、読み込みの印だけにする。
        // 起動のたびに困り顔が出ると、子どもが不安になる。
        // 印の位置は作るときに中央へ寄せてあるので、ここでは見せ隠しだけ行う。
        if layout.is_loading() {
            show(self.spinner);
            hide(self.face);
            return;
        }
        hide(self.spinner);
        show(self.face);

        place(self.left_eye, layout.left_eye);
        place(self.right_eye, layout.right_eye);
        show_at(self.left_pupil, layout.left_pupil);
        show_at(self.right_pupil, layout.right_pupil);

        self.write_mouth(layout);
        self.write_brows(layout);

        place(self.button, layout.button);
        paint(self.button, layout.button_color);
        self.write_microphone(layout.microphone);
    }

    /// マイクの絵を置く。押せる場所は画面全体だが、ここを目印にしてもらう。
    unsafe fn write_microphone(&mut self, microphone: Microphone) {
        place(self.mic_head, microphone.head);
        place(self.mic_stem, microphone.stem);
        place(self.mic_base, microphone.base);
    }

    unsafe fn write_mouth(&mut self, layout: &FaceLayout) {
        let bounds = layout.mouth.bounds();
        place(self.mouth, bounds);

        match layout.mouth {
            // 閉じた口は角を丸めた細い棒にする。
            Mouth::Closed(_) => {
                bsp::lv_obj_set_style_radius(self.mouth, bounds.height as i32 / 2, MAIN_PART);
                hide(self.mouth_cover);
            }
            Mouth::Open(_) => {
                round(self.mouth);
                hide(self.mouth_cover);
            }
            // 楕円の上半分を背景色で隠して弧に見せる。
            Mouth::Smile(_) => {
                round(self.mouth);
                place(
                    self.mouth_cover,
                    Rect {
                        x: bounds.x,
                        y: bounds.y,
                        width: bounds.width,
                        height: bounds.height / 2,
                    },
                );
                paint(self.mouth_cover, layout.background);
                show(self.mouth_cover);
            }
        }
    }

    unsafe fn write_brows(&mut self, layout: &FaceLayout) {
        let Some((left, right)) = layout.brows else {
            hide(self.left_brow);
            hide(self.right_brow);
            return;
        };

        self.brow_points[0] = points_of(left);
        self.brow_points[1] = points_of(right);

        bsp::lv_line_set_points(self.left_brow, self.brow_points[0].as_ptr(), 2);
        bsp::lv_line_set_points(self.right_brow, self.brow_points[1].as_ptr(), 2);
        show(self.left_brow);
        show(self.right_brow);
    }
}

/// 顔の部品をまとめる、透明で画面いっぱいの入れ物を作る。
unsafe fn make_group(parent: *mut bsp::lv_obj_t) -> *mut bsp::lv_obj_t {
    let group = bsp::lv_obj_create(parent);

    bsp::lv_obj_remove_style_all(group);
    bsp::lv_obj_set_size(
        group,
        i32::from(layout::SCREEN_WIDTH),
        i32::from(layout::SCREEN_HEIGHT),
    );
    bsp::lv_obj_set_pos(group, 0, 0);
    bsp::lv_obj_remove_flag(
        group,
        bsp::lv_obj_flag_t_LV_OBJ_FLAG_SCROLLABLE | bsp::lv_obj_flag_t_LV_OBJ_FLAG_CLICKABLE,
    );

    group
}

/// 枠も余白も持たない、色を塗るだけの板を作る。
unsafe fn make_panel(parent: *mut bsp::lv_obj_t) -> *mut bsp::lv_obj_t {
    let panel = bsp::lv_obj_create(parent);

    bsp::lv_obj_remove_style_all(panel);
    bsp::lv_obj_set_style_bg_opa(panel, OPAQUE, MAIN_PART);
    // 触れると動いてしまうため、指の操作を受け取らないようにする。
    bsp::lv_obj_remove_flag(
        panel,
        bsp::lv_obj_flag_t_LV_OBJ_FLAG_SCROLLABLE | bsp::lv_obj_flag_t_LV_OBJ_FLAG_CLICKABLE,
    );

    panel
}

/// くるくる回る読み込みの印を作る。
///
/// 大きさを決めてから中央に寄せる。位置を数値で指定すると、
/// 部品の既定の大きさが確定する前に置かれて左上に寄ってしまう。
unsafe fn make_spinner(parent: *mut bsp::lv_obj_t) -> *mut bsp::lv_obj_t {
    let spinner = bsp::lv_spinner_create(parent);

    bsp::lv_spinner_set_anim_params(spinner, SPINNER_PERIOD_MS, SPINNER_ARC_DEGREES);
    bsp::lv_obj_set_size(spinner, SPINNER_SIZE, SPINNER_SIZE);
    bsp::lv_obj_center(spinner);
    // 置き場所が決まるまでは見せない。
    hide(spinner);
    bsp::lv_obj_set_style_arc_width(spinner, SPINNER_THICKNESS, MAIN_PART);
    bsp::lv_obj_set_style_arc_color(spinner, color_of(layout::spinner_color()), INDICATOR_PART);
    bsp::lv_obj_set_style_arc_width(spinner, SPINNER_THICKNESS, INDICATOR_PART);
    // つまみは使わないので消す。
    bsp::lv_obj_set_style_bg_opa(spinner, 0, KNOB_PART);

    spinner
}

unsafe fn make_line(parent: *mut bsp::lv_obj_t) -> *mut bsp::lv_obj_t {
    let line = bsp::lv_line_create(parent);
    bsp::lv_obj_remove_style_all(line);
    line
}

unsafe fn place(object: *mut bsp::lv_obj_t, area: Rect) {
    bsp::lv_obj_set_pos(object, i32::from(area.x), i32::from(area.y));
    bsp::lv_obj_set_size(object, i32::from(area.width), i32::from(area.height));
}

unsafe fn paint(object: *mut bsp::lv_obj_t, color: Color) {
    bsp::lv_obj_set_style_bg_color(object, color_of(color), MAIN_PART);
}

unsafe fn round(object: *mut bsp::lv_obj_t) {
    bsp::lv_obj_set_style_radius(object, bsp::LV_RADIUS_CIRCLE as i32, MAIN_PART);
}

unsafe fn show(object: *mut bsp::lv_obj_t) {
    bsp::lv_obj_remove_flag(object, bsp::lv_obj_flag_t_LV_OBJ_FLAG_HIDDEN);
}

unsafe fn hide(object: *mut bsp::lv_obj_t) {
    bsp::lv_obj_add_flag(object, bsp::lv_obj_flag_t_LV_OBJ_FLAG_HIDDEN);
}

/// 置き場所があれば置いて見せ、なければ隠す。
unsafe fn show_at(object: *mut bsp::lv_obj_t, area: Option<Rect>) {
    match area {
        Some(area) => {
            place(object, area);
            show(object);
        }
        None => hide(object),
    }
}

fn color_of(color: Color) -> bsp::lv_color_t {
    unsafe { bsp::lv_color_make(color.r, color.g, color.b) }
}

fn points_of(brow: Brow) -> [bsp::lv_point_precise_t; 2] {
    [
        bsp::lv_point_precise_t {
            x: i32::from(brow.outer.x),
            y: i32::from(brow.outer.y),
        },
        bsp::lv_point_precise_t {
            x: i32::from(brow.inner.x),
            y: i32::from(brow.inner.y),
        },
    ]
}
