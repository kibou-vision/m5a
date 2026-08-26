//! 顔を画面のどこにどう置くかを決める。
//!
//! 画素を塗るのは LVGL の役目で、ここは配置と色だけを決める。
//! 判断をこの層に閉じ込めることで、実機なしで見た目の規則を検証できる。

use crate::face::{Expression, FaceFrame};

/// 画面の大きさ。CoreS3 の LCD に合わせている。
pub const SCREEN_WIDTH: i16 = 320;
pub const SCREEN_HEIGHT: i16 = 240;

/// 目の中心。
const LEFT_EYE_CENTER: Point = Point::new(104, 96);
const RIGHT_EYE_CENTER: Point = Point::new(216, 96);
const EYE_WIDTH: u16 = 72;
const EYE_HEIGHT: u16 = 72;
/// 閉じきっても線として見えるようにする下限。
const EYE_MIN_HEIGHT: u16 = 4;

const PUPIL_DIAMETER: u16 = 30;
/// 瞳を描くのに必要な目の開き具合。これ以下はつぶれて見えるので描かない。
const PUPIL_VISIBLE_OPENNESS: u8 = 35;
/// 瞳が左右に動ける幅。
const GAZE_TRAVEL: i16 = 14;

const MOUTH_CENTER: Point = Point::new(160, 182);
const MOUTH_WIDTH: u16 = 96;
const MOUTH_MAX_HEIGHT: u16 = 56;
const MOUTH_MIN_HEIGHT: u16 = 10;
const CLOSED_MOUTH_HEIGHT: u16 = 6;

/// 眉が目の上に浮く高さ。
const BROW_LIFT: i16 = 52;
/// 眉の傾き。困り顔は外側が下がり内側が上がる「ハの字」になる。
const BROW_SLANT: i16 = 9;
const BROW_HALF_WIDTH: i16 = 32;

/// おはなしボタン。仕様どおり画面の右下に置く。
pub const TALK_BUTTON_CENTER: Point = Point::new(272, 192);
pub const TALK_BUTTON_RADIUS: i16 = 40;
/// 5歳児は正確に狙えないので、見た目の円より少し広く反応させる。
const TALK_BUTTON_TOUCH_MARGIN: i16 = 10;
/// 中央のしるしの大きさ。字が読めなくてもマイクだと分かるようにする。
const TALK_BUTTON_MARK_DIAMETER: u16 = 40;

const CALM_BACKGROUND: Color = Color::new(16, 26, 46);
const TROUBLE_BACKGROUND: Color = Color::new(96, 28, 24);
const SLEEPING_BACKGROUND: Color = Color::new(8, 12, 24);

const EYE_COLOR: Color = Color::new(245, 250, 255);
const PUPIL_COLOR: Color = Color::new(18, 24, 40);
const MOUTH_COLOR: Color = Color::new(255, 140, 130);
const BROW_COLOR: Color = Color::new(230, 220, 210);

const BUTTON_IDLE_COLOR: Color = Color::new(40, 170, 90);
const BUTTON_ACTIVE_COLOR: Color = Color::new(235, 130, 40);
const BUTTON_DISABLED_COLOR: Color = Color::new(70, 80, 80);
const BUTTON_MARK_COLOR: Color = Color::new(250, 250, 250);

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
    /// 笑った口。下向きの弧。
    Smile(Rect),
}

impl Mouth {
    pub fn bounds(self) -> Rect {
        match self {
            Self::Closed(rect) | Self::Open(rect) | Self::Smile(rect) => rect,
        }
    }
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
    pub left_eye: Rect,
    pub right_eye: Rect,
    /// 目が細いときは描かない。
    pub left_pupil: Option<Rect>,
    pub right_pupil: Option<Rect>,
    pub mouth: Mouth,
    /// 困り顔のときだけ眉を出す。
    pub brows: Option<(Brow, Brow)>,
    pub button: Rect,
    pub button_mark: Rect,
    pub button_color: Color,
}

/// 顔ひとコマ分の配置を決める。
pub fn lay_out_face(frame: &FaceFrame) -> FaceLayout {
    let eye_height = (EYE_HEIGHT * u16::from(frame.eye_openness) / 100).max(EYE_MIN_HEIGHT);
    let shows_pupil = frame.eye_openness >= PUPIL_VISIBLE_OPENNESS;
    let gaze = i16::from(frame.gaze_x) * GAZE_TRAVEL / 100;

    FaceLayout {
        background: background_of(frame.expression),
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
        button_mark: Rect::around(
            TALK_BUTTON_CENTER,
            TALK_BUTTON_MARK_DIAMETER,
            TALK_BUTTON_MARK_DIAMETER,
        ),
        button_color: button_color_of(frame.expression),
    }
}

/// 指の位置がおはなしボタンの上かどうか。
pub fn contains_talk_button(at: Point) -> bool {
    let reach = TALK_BUTTON_RADIUS + TALK_BUTTON_TOUCH_MARGIN;
    let dx = i32::from(at.x - TALK_BUTTON_CENTER.x);
    let dy = i32::from(at.y - TALK_BUTTON_CENTER.y);

    dx * dx + dy * dy <= i32::from(reach) * i32::from(reach)
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
        Expression::Sleeping => SLEEPING_BACKGROUND,
        Expression::Trouble => TROUBLE_BACKGROUND,
        _ => CALM_BACKGROUND,
    }
}

fn button_color_of(expression: Expression) -> Color {
    match expression {
        Expression::Listening => BUTTON_ACTIVE_COLOR,
        // 設定待ちや接続前は押しても始まらないので沈んだ色にする。
        Expression::Sleeping | Expression::Waiting | Expression::Trouble => BUTTON_DISABLED_COLOR,
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
        Expression::Idle | Expression::Listening => {
            Mouth::Smile(Rect::around(MOUTH_CENTER, MOUTH_WIDTH, MOUTH_MAX_HEIGHT))
        }
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
            Expression::Sleeping,
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
    fn talk_button_accepts_touches_on_and_near_it() {
        assert!(contains_talk_button(TALK_BUTTON_CENTER));
        assert!(contains_talk_button(Point::new(
            TALK_BUTTON_CENTER.x + TALK_BUTTON_RADIUS,
            TALK_BUTTON_CENTER.y
        )));
        assert!(!contains_talk_button(Point::new(20, 20)));
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
        for expression in [Expression::Sleeping, Expression::Waiting, Expression::Trouble] {
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
            Expression::Sleeping,
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
                layout.button_mark,
            ];
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
