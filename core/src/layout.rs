//! 顔を画面のどこにどう置くかを決める。
//!
//! 画素を塗るのは LVGL の役目で、ここは配置と色だけを決める。
//! 判断をこの層に閉じ込めることで、実機なしで見た目の規則を検証できる。

use crate::face::{Expression, FaceFrame};

/// 画面の大きさ。CoreS3 の LCD に合わせている。
pub const SCREEN_WIDTH: i16 = 320;
pub const SCREEN_HEIGHT: i16 = 240;

/// 目の中心。
const LEFT_EYE_CENTER: Point = Point::new(80, 108);
const RIGHT_EYE_CENTER: Point = Point::new(240, 108);
const EYE_WIDTH: u16 = 72;
const EYE_HEIGHT: u16 = 72;
/// 閉じきっても線として見えるようにする下限。
const EYE_MIN_HEIGHT: u16 = 4;

const PUPIL_DIAMETER: u16 = 38;
/// 瞳を描くのに必要な目の開き具合。
/// 瞳が目からはみ出さない高さまで開いているときだけ描く。
const PUPIL_VISIBLE_OPENNESS: u8 = 55;
/// 瞳が左右に動ける幅。
const GAZE_TRAVEL: i16 = 14;

const MOUTH_CENTER: Point = Point::new(160, 182);
const MOUTH_WIDTH: u16 = 96;
const MOUTH_MAX_HEIGHT: u16 = 56;
const MOUTH_MIN_HEIGHT: u16 = 10;
const CLOSED_MOUTH_HEIGHT: u16 = 6;
/// 笑った口を描く弧の直径。
///
/// 弧は正方形の枠に内接する円として描かれる。横長の枠を渡すと
/// 円が枠の中で寄ってしまい、口が左にずれて見える。
const SMILE_DIAMETER: u16 = 84;
/// 弧のどのあたりを見せるか。大きいほど口が深くなる。
const SMILE_DEPTH: i16 = 22;

/// 眉が目の上に浮く高さ。
const BROW_LIFT: i16 = 52;
/// 眉の傾き。困り顔は外側が下がり内側が上がる「ハの字」になる。
const BROW_SLANT: i16 = 9;
const BROW_HALF_WIDTH: i16 = 32;

/// おはなしボタン。仕様どおり画面の右下に置く。
pub const TALK_BUTTON_CENTER: Point = Point::new(272, 192);
pub const TALK_BUTTON_RADIUS: i16 = 32;

/// マイクの絵の各部。円の中に収まる大きさにする。
const MIC_HEAD_WIDTH: u16 = 16;
const MIC_HEAD_HEIGHT: u16 = 24;
/// 頭の中心をボタンの中心より上に置き、受け皿と支柱の場所を空ける。
const MIC_HEAD_LIFT: i16 = 12;
/// 受け皿。頭の下half を包む下向きの弧として描く。
const MIC_CRADLE_SIZE: u16 = 30;
const MIC_CRADLE_LIFT: i16 = 6;
const MIC_STEM_WIDTH: u16 = 3;
const MIC_STEM_HEIGHT: u16 = 7;
const MIC_BASE_WIDTH: u16 = 22;
const MIC_BASE_HEIGHT: u16 = 3;

/// 読み込み中に出す印の大きさ。
const SPINNER_DIAMETER: u16 = 72;

/// 落ち着いているときの背景。黒にして顔だけが浮かんで見えるようにする。
const CALM_BACKGROUND: Color = Color::new(0, 0, 0);
const TROUBLE_BACKGROUND: Color = Color::new(96, 28, 24);

const EYE_COLOR: Color = Color::new(245, 250, 255);
const PUPIL_COLOR: Color = Color::new(18, 24, 40);
const MOUTH_COLOR: Color = Color::new(255, 140, 130);
const BROW_COLOR: Color = Color::new(230, 220, 210);

const BUTTON_IDLE_COLOR: Color = Color::new(40, 170, 90);
const BUTTON_ACTIVE_COLOR: Color = Color::new(235, 130, 40);
const BUTTON_DISABLED_COLOR: Color = Color::new(70, 80, 80);
const BUTTON_MARK_COLOR: Color = Color::new(250, 250, 250);
const SPINNER_COLOR: Color = Color::new(120, 190, 255);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Point {
    pub x: i16,
    pub y: i16,
}

impl Point {
    pub const fn new(x: i16, y: i16) -> Self {
        Self { x, y }
    }
}

/// 左上の位置と大きさ。LVGL のオブジェクト配置にそのまま渡せる形にしている。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i16,
    pub y: i16,
    pub width: u16,
    pub height: u16,
}

impl Rect {
    /// 中心と大きさから作る。
    fn around(center: Point, width: u16, height: u16) -> Self {
        Self {
            x: center.x - width as i16 / 2,
            y: center.y - height as i16 / 2,
            width,
            height,
        }
    }

    pub fn right(&self) -> i16 {
        self.x + self.width as i16
    }

    pub fn bottom(&self) -> i16 {
        self.y + self.height as i16
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
}

/// 口の形。形ごとに描き方が変わるため区別する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mouth {
    /// 閉じた口。細い横棒。
    Closed(Rect),
    /// 開いた口。声の大きさで縦に伸びる楕円。
    Open(Rect),
    /// 笑った口。口は閉じたまま、口角の上がった線で描く。
    Smile(Rect),
}

impl Mouth {
    pub fn bounds(self) -> Rect {
        match self {
            Self::Closed(rect) | Self::Open(rect) | Self::Smile(rect) => rect,
        }
    }
}

/// マイクの絵。頭・受け皿・支柱・台座で組む。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Microphone {
    /// 角を丸めた縦長の頭。
    pub head: Rect,
    /// 頭を下から包む受け皿。下向きの弧として描く。
    pub cradle: Rect,
    pub stem: Rect,
    pub base: Rect,
}

/// 片方の眉。外側から内側へ引く線。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Brow {
    pub outer: Point,
    pub inner: Point,
}

/// ある瞬間の顔の配置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FaceLayout {
    pub background: Color,
    /// 立ち上げ中はここに読み込みの印を出し、顔とボタンは見せない。
    pub spinner: Option<Rect>,
    pub left_eye: Rect,
    pub right_eye: Rect,
    /// 目が細いときは描かない。
    pub left_pupil: Option<Rect>,
    pub right_pupil: Option<Rect>,
    pub mouth: Mouth,
    /// 困り顔のときだけ眉を出す。
    pub brows: Option<(Brow, Brow)>,
    pub button: Rect,
    /// ボタンの中に描くマイクの絵。
    pub microphone: Microphone,
    pub button_color: Color,
}

impl FaceLayout {
    /// 顔ではなく読み込みの印を見せる場面か。
    pub fn is_loading(&self) -> bool {
        self.spinner.is_some()
    }
}

/// 顔ひとコマ分の配置を決める。
pub fn lay_out_face(frame: &FaceFrame) -> FaceLayout {
    let eye_height = (EYE_HEIGHT * u16::from(frame.eye_openness) / 100).max(EYE_MIN_HEIGHT);
    let shows_pupil = frame.eye_openness >= PUPIL_VISIBLE_OPENNESS;
    let gaze = i16::from(frame.gaze_x) * GAZE_TRAVEL / 100;

    FaceLayout {
        background: background_of(frame.expression),
        spinner: frame.expression.is_loading().then(|| {
            Rect::around(
                Point::new(SCREEN_WIDTH / 2, SCREEN_HEIGHT / 2),
                SPINNER_DIAMETER,
                SPINNER_DIAMETER,
            )
        }),
        left_eye: Rect::around(LEFT_EYE_CENTER, EYE_WIDTH, eye_height),
        right_eye: Rect::around(RIGHT_EYE_CENTER, EYE_WIDTH, eye_height),
        left_pupil: lay_out_pupil(LEFT_EYE_CENTER, gaze, shows_pupil),
        right_pupil: lay_out_pupil(RIGHT_EYE_CENTER, gaze, shows_pupil),
        mouth: lay_out_mouth(frame),
        brows: lay_out_brows(frame.expression),
        button: Rect::around(
            TALK_BUTTON_CENTER,
            TALK_BUTTON_RADIUS as u16 * 2,
            TALK_BUTTON_RADIUS as u16 * 2,
        ),
        microphone: lay_out_microphone(),
        button_color: button_color_of(frame.expression),
    }
}

/// その位置へのふれ方が、おはなしの合図になるか。
///
/// 5歳児は小さな的を正確に狙えないため、画面のどこでも受け付ける。
/// 右下のマイクの絵は「ここを押すと話せる」と伝えるための目印で、
/// 押せる場所を限る枠ではない。
pub fn is_talk_target(at: Point) -> bool {
    (0..SCREEN_WIDTH).contains(&at.x) && (0..SCREEN_HEIGHT).contains(&at.y)
}

/// マイクの絵の置き場所。ボタンの中心を基準に、上から順に積む。
fn lay_out_microphone() -> Microphone {
    let head_center = Point::new(TALK_BUTTON_CENTER.x, TALK_BUTTON_CENTER.y - MIC_HEAD_LIFT);
    let cradle = Rect::around(
        Point::new(TALK_BUTTON_CENTER.x, TALK_BUTTON_CENTER.y - MIC_CRADLE_LIFT),
        MIC_CRADLE_SIZE,
        MIC_CRADLE_SIZE,
    );

    // 前の部品の下端から積む。中心からの計算を重ねると丸めでずれる。
    let stem = Rect {
        x: TALK_BUTTON_CENTER.x - MIC_STEM_WIDTH as i16 / 2,
        y: cradle.bottom(),
        width: MIC_STEM_WIDTH,
        height: MIC_STEM_HEIGHT,
    };
    let base = Rect {
        x: TALK_BUTTON_CENTER.x - MIC_BASE_WIDTH as i16 / 2,
        y: stem.bottom(),
        width: MIC_BASE_WIDTH,
        height: MIC_BASE_HEIGHT,
    };

    Microphone {
        head: Rect::around(head_center, MIC_HEAD_WIDTH, MIC_HEAD_HEIGHT),
        cradle,
        stem,
        base,
    }
}

/// 読み込みの印の色。
pub fn spinner_color() -> Color {
    SPINNER_COLOR
}

/// 目の色。
pub fn eye_color() -> Color {
    EYE_COLOR
}

/// 瞳の色。
pub fn pupil_color() -> Color {
    PUPIL_COLOR
}

/// 口の色。
pub fn mouth_color() -> Color {
    MOUTH_COLOR
}

/// 眉の色。
pub fn brow_color() -> Color {
    BROW_COLOR
}

/// ボタン中央のしるしの色。
pub fn button_mark_color() -> Color {
    BUTTON_MARK_COLOR
}

fn background_of(expression: Expression) -> Color {
    match expression {
        Expression::Trouble => TROUBLE_BACKGROUND,
        _ => CALM_BACKGROUND,
    }
}

fn button_color_of(expression: Expression) -> Color {
    match expression {
        Expression::Listening => BUTTON_ACTIVE_COLOR,
        // 設定待ちや接続前は押しても始まらないので沈んだ色にする。
        Expression::Waiting | Expression::Trouble => BUTTON_DISABLED_COLOR,
        _ => BUTTON_IDLE_COLOR,
    }
}

fn lay_out_pupil(eye_center: Point, gaze: i16, shows_pupil: bool) -> Option<Rect> {
    if !shows_pupil {
        return None;
    }

    let center = Point::new(eye_center.x + gaze, eye_center.y);
    Some(Rect::around(center, PUPIL_DIAMETER, PUPIL_DIAMETER))
}

fn lay_out_mouth(frame: &FaceFrame) -> Mouth {
    match frame.expression {
        Expression::Talking => {
            let height = (MOUTH_MAX_HEIGHT * u16::from(frame.mouth_openness) / 100)
                .max(MOUTH_MIN_HEIGHT);
            Mouth::Open(Rect::around(MOUTH_CENTER, MOUTH_WIDTH, height))
        }
        // 口は開けず、口角を上げた線にする。
        Expression::Idle | Expression::Listening => Mouth::Smile(Rect::around(
            Point::new(
                MOUTH_CENTER.x,
                MOUTH_CENTER.y - SMILE_DIAMETER as i16 / 2 + SMILE_DEPTH,
            ),
            SMILE_DIAMETER,
            SMILE_DIAMETER,
        )),
        _ => Mouth::Closed(Rect::around(
            MOUTH_CENTER,
            MOUTH_WIDTH / 2,
            CLOSED_MOUTH_HEIGHT,
        )),
    }
}

/// 困り眉。外側が下がり内側が上がる「ハの字」にする。
/// 逆向きにすると怒った顔に見えてしまう。
fn lay_out_brows(expression: Expression) -> Option<(Brow, Brow)> {
    if expression != Expression::Trouble {
        return None;
    }

    let brow_y = LEFT_EYE_CENTER.y - BROW_LIFT;

    let left = Brow {
        outer: Point::new(LEFT_EYE_CENTER.x - BROW_HALF_WIDTH, brow_y + BROW_SLANT),
        inner: Point::new(LEFT_EYE_CENTER.x + BROW_HALF_WIDTH, brow_y - BROW_SLANT),
    };
    let right = Brow {
        outer: Point::new(RIGHT_EYE_CENTER.x + BROW_HALF_WIDTH, brow_y + BROW_SLANT),
        inner: Point::new(RIGHT_EYE_CENTER.x - BROW_HALF_WIDTH, brow_y - BROW_SLANT),
    };

    Some((left, right))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::face::{Expression, FaceFrame};

    fn frame(expression: Expression, eye_openness: u8) -> FaceFrame {
        FaceFrame {
            expression,
            eye_openness,
            mouth_openness: 0,
            gaze_x: 0,
        }
    }

    #[test]
    fn open_eyes_show_pupils_at_their_centre() {
        let layout = lay_out_face(&frame(Expression::Idle, 100));

        let pupil = layout.left_pupil.expect("開いた目には瞳が要る");
        assert_eq!(pupil.x + pupil.width as i16 / 2, LEFT_EYE_CENTER.x);
        assert_eq!(pupil.y + pupil.height as i16 / 2, LEFT_EYE_CENTER.y);
    }

    #[test]
    fn closed_eyes_hide_pupils_but_keep_a_line() {
        let layout = lay_out_face(&frame(Expression::Idle, 0));

        assert!(layout.left_pupil.is_none());
        assert!(layout.right_pupil.is_none());
        assert_eq!(layout.left_eye.height, EYE_MIN_HEIGHT);
    }

    #[test]
    fn eyes_shrink_as_they_close() {
        let wide = lay_out_face(&frame(Expression::Listening, 100)).left_eye.height;
        let narrow = lay_out_face(&frame(Expression::Listening, 40)).left_eye.height;

        assert!(narrow < wide, "細めた目のほうが低いはず: {narrow} < {wide}");
    }

    #[test]
    fn eyes_stay_centred_while_blinking() {
        for openness in [100, 60, 20, 0] {
            let eye = lay_out_face(&frame(Expression::Idle, openness)).left_eye;
            assert_eq!(eye.y + eye.height as i16 / 2, LEFT_EYE_CENTER.y);
        }
    }

    #[test]
    fn gaze_moves_both_pupils_the_same_way() {
        let looking_right = FaceFrame {
            gaze_x: 100,
            ..frame(Expression::Thinking, 100)
        };

        let layout = lay_out_face(&looking_right);

        let left = layout.left_pupil.unwrap();
        let right = layout.right_pupil.unwrap();
        assert_eq!(left.x + left.width as i16 / 2, LEFT_EYE_CENTER.x + GAZE_TRAVEL);
        assert_eq!(
            right.x + right.width as i16 / 2,
            RIGHT_EYE_CENTER.x + GAZE_TRAVEL
        );
    }

    #[test]
    fn mouth_shape_follows_the_expression() {
        assert!(matches!(
            lay_out_face(&frame(Expression::Idle, 100)).mouth,
            Mouth::Smile(_)
        ));
        assert!(matches!(
            lay_out_face(&frame(Expression::Talking, 100)).mouth,
            Mouth::Open(_)
        ));
        assert!(matches!(
            lay_out_face(&frame(Expression::Thinking, 100)).mouth,
            Mouth::Closed(_)
        ));
    }

    #[test]
    fn talking_mouth_grows_with_voice() {
        let quiet = FaceFrame {
            mouth_openness: 10,
            ..frame(Expression::Talking, 100)
        };
        let loud = FaceFrame {
            mouth_openness: 100,
            ..frame(Expression::Talking, 100)
        };

        let quiet_height = lay_out_face(&quiet).mouth.bounds().height;
        let loud_height = lay_out_face(&loud).mouth.bounds().height;

        assert!(quiet_height < loud_height);
        assert_eq!(loud_height, MOUTH_MAX_HEIGHT);
    }

    #[test]
    fn worried_brows_rise_towards_the_middle() {
        let (left, right) = lay_out_face(&frame(Expression::Trouble, 60))
            .brows
            .expect("困り顔には眉が要る");

        // 内側の端が外側より高い（y が小さい）のが困り顔。逆だと怒り顔になる。
        assert!(
            left.inner.y < left.outer.y,
            "左眉は内側が上がるはず: 内 {} 外 {}",
            left.inner.y,
            left.outer.y
        );
        assert!(
            right.inner.y < right.outer.y,
            "右眉は内側が上がるはず: 内 {} 外 {}",
            right.inner.y,
            right.outer.y
        );
        // 内側どうしが顔の中央に寄っている。
        assert!(left.inner.x > left.outer.x);
        assert!(right.inner.x < right.outer.x);
    }

    #[test]
    fn brows_appear_only_when_troubled() {
        for expression in [
            Expression::Waiting,
            Expression::Idle,
            Expression::Listening,
            Expression::Thinking,
            Expression::Talking,
        ] {
            assert!(lay_out_face(&frame(expression, 80)).brows.is_none());
        }
        assert!(lay_out_face(&frame(Expression::Trouble, 80)).brows.is_some());
    }

    #[test]
    fn talk_button_sits_in_the_lower_right_corner() {
        let layout = lay_out_face(&frame(Expression::Idle, 100));

        assert!(layout.button.x > SCREEN_WIDTH / 2);
        assert!(layout.button.y > SCREEN_HEIGHT / 2);
        assert!(layout.button.right() <= SCREEN_WIDTH);
        assert!(layout.button.bottom() <= SCREEN_HEIGHT);
    }

    #[test]
    fn any_touch_on_the_screen_starts_talking() {
        // 5歳児が的を狙えなくてよいよう、画面のどこでも受け付ける。
        for at in [
            TALK_BUTTON_CENTER,
            Point::new(0, 0),
            Point::new(20, 20),
            Point::new(SCREEN_WIDTH - 1, SCREEN_HEIGHT - 1),
        ] {
            assert!(is_talk_target(at), "{at:?}");
        }
    }

    #[test]
    fn touches_outside_the_screen_are_ignored() {
        assert!(!is_talk_target(Point::new(-1, 100)));
        assert!(!is_talk_target(Point::new(SCREEN_WIDTH, 100)));
        assert!(!is_talk_target(Point::new(100, SCREEN_HEIGHT)));
    }

    #[test]
    fn microphone_sits_inside_the_button() {
        let layout = lay_out_face(&frame(Expression::Idle, 100));
        let button = layout.button;
        let mic = layout.microphone;

        for part in [mic.head, mic.cradle, mic.stem, mic.base] {
            assert!(
                part.x >= button.x
                    && part.y >= button.y
                    && part.right() <= button.right()
                    && part.bottom() <= button.bottom(),
                "{part:?} がボタン {button:?} からはみ出した"
            );
        }
    }

    #[test]
    fn microphone_stacks_its_parts_downwards() {
        let mic = lay_out_face(&frame(Expression::Idle, 100)).microphone;

        assert!(mic.cradle.bottom() <= mic.stem.y, "受け皿の下に支柱が来るはず");
        assert!(mic.stem.bottom() <= mic.base.y, "支柱の下に台座が来るはず");
        assert!(mic.base.width > mic.stem.width, "台座は支柱より広いはず");
    }

    #[test]
    fn cradle_wraps_the_microphone_head() {
        let mic = lay_out_face(&frame(Expression::Idle, 100)).microphone;

        assert!(mic.cradle.width > mic.head.width, "受け皿は頭より広いはず");
        assert!(mic.cradle.y > mic.head.y, "受け皿は頭より下から始まるはず");
        assert!(
            mic.cradle.bottom() > mic.head.bottom(),
            "受け皿は頭の下端を越えて包むはず"
        );
    }

    #[test]
    fn smile_is_drawn_in_a_square_so_it_stays_centred() {
        let Mouth::Smile(bounds) = lay_out_face(&frame(Expression::Idle, 100)).mouth else {
            panic!("待機中は笑顔のはず");
        };

        // 弧は正方形に内接する円として描かれる。横長だと口が横にずれる。
        assert_eq!(bounds.width, bounds.height);
        assert_eq!(bounds.x + bounds.width as i16 / 2, MOUTH_CENTER.x);
    }

    #[test]
    fn pupils_stay_inside_the_eyes() {
        // 瞳が目より高いと、はみ出して見える。
        let smallest_eye = EYE_HEIGHT * u16::from(PUPIL_VISIBLE_OPENNESS) / 100;
        assert!(
            PUPIL_DIAMETER <= smallest_eye,
            "瞳 {PUPIL_DIAMETER} が目の高さ {smallest_eye} を超えている"
        );
    }

    #[test]
    fn smiling_keeps_the_mouth_closed() {
        let smiling = lay_out_face(&frame(Expression::Idle, 100)).mouth;
        let talking = lay_out_face(&FaceFrame {
            mouth_openness: 100,
            ..frame(Expression::Talking, 100)
        })
        .mouth;

        // 笑顔は線で描くので、口を開けている形とは区別する。
        assert!(matches!(smiling, Mouth::Smile(_)));
        assert!(matches!(talking, Mouth::Open(_)));
    }

    #[test]
    fn loading_shows_a_spinner_instead_of_the_face() {
        let loading = lay_out_face(&frame(Expression::Waiting, 70));

        assert!(loading.is_loading());
        let spinner = loading.spinner.expect("読み込み中は印を出す");
        // 画面の中央に置く。
        assert_eq!(spinner.x + spinner.width as i16 / 2, SCREEN_WIDTH / 2);
        assert_eq!(spinner.y + spinner.height as i16 / 2, SCREEN_HEIGHT / 2);
    }

    #[test]
    fn settled_states_show_the_face_not_a_spinner() {
        for expression in [
            Expression::Idle,
            Expression::Listening,
            Expression::Thinking,
            Expression::Talking,
            Expression::Trouble,
        ] {
            let layout = lay_out_face(&frame(expression, 100));
            assert!(!layout.is_loading(), "{expression:?}");
            assert!(layout.spinner.is_none());
        }
    }

    #[test]
    fn talk_button_changes_colour_with_the_state() {
        assert_eq!(
            lay_out_face(&frame(Expression::Listening, 100)).button_color,
            BUTTON_ACTIVE_COLOR
        );
        assert_eq!(
            lay_out_face(&frame(Expression::Idle, 100)).button_color,
            BUTTON_IDLE_COLOR
        );
        for expression in [Expression::Waiting, Expression::Trouble] {
            assert_eq!(
                lay_out_face(&frame(expression, 60)).button_color,
                BUTTON_DISABLED_COLOR,
                "{expression:?}"
            );
        }
    }

    #[test]
    fn trouble_face_uses_a_warm_background() {
        assert_eq!(
            lay_out_face(&frame(Expression::Trouble, 60)).background,
            TROUBLE_BACKGROUND
        );
        assert_eq!(
            lay_out_face(&frame(Expression::Idle, 100)).background,
            CALM_BACKGROUND
        );
    }

    #[test]
    fn everything_stays_inside_the_screen() {
        for expression in [
            Expression::Waiting,
            Expression::Idle,
            Expression::Listening,
            Expression::Thinking,
            Expression::Talking,
            Expression::Trouble,
        ] {
            let layout = lay_out_face(&FaceFrame {
                mouth_openness: 100,
                gaze_x: 100,
                ..frame(expression, 100)
            });

            let mut boxes = vec![
                layout.left_eye,
                layout.right_eye,
                layout.mouth.bounds(),
                layout.button,
                layout.microphone.head,
                layout.microphone.base,
            ];
            boxes.extend(layout.spinner);
            boxes.extend(layout.left_pupil);
            boxes.extend(layout.right_pupil);

            for area in boxes {
                assert!(
                    area.x >= 0
                        && area.y >= 0
                        && area.right() <= SCREEN_WIDTH
                        && area.bottom() <= SCREEN_HEIGHT,
                    "{expression:?} で {area:?} が画面外にはみ出した"
                );
            }
        }
    }
}
