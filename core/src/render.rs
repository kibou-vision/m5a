//! 顔を画面に描く。
//!
//! [`crate::face::FaceFrame`] が決めた形を図形に落とすだけで、
//! 表情の時間変化はここでは扱わない。画像を持たず図形で組み立てるため、
//! SDカードの読み込みを待たずに描け、まばたきも滑らかに出せる。

use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{Circle, Ellipse, PrimitiveStyle, Rectangle};
use embedded_graphics::{pixelcolor::Rgb565, primitives::Line};

use crate::face::{Expression, FaceFrame};

/// 想定する画面の大きさ。CoreS3 の LCD に合わせている。
pub const SCREEN_WIDTH: u32 = 320;
pub const SCREEN_HEIGHT: u32 = 240;

/// 目の中心。
const LEFT_EYE_CENTER: Point = Point::new(104, 96);
const RIGHT_EYE_CENTER: Point = Point::new(216, 96);
/// 目の最大の大きさ。
const EYE_WIDTH: u32 = 72;
const EYE_HEIGHT: u32 = 72;
/// 瞳の大きさ。
const PUPIL_DIAMETER: u32 = 30;
/// 瞳を描くのに必要な目の開き具合。これ以下は閉じた線として描く。
const PUPIL_VISIBLE_OPENNESS: u8 = 35;
/// 瞳が左右に動ける幅。
const GAZE_TRAVEL: i32 = 14;

/// 口の中心と大きさ。
const MOUTH_CENTER: Point = Point::new(160, 182);
const MOUTH_WIDTH: u32 = 96;
const MOUTH_MAX_HEIGHT: u32 = 56;

/// 落ち着いているときの背景。
const CALM_BACKGROUND: Rgb565 = Rgb565::new(2, 6, 11);
/// 困っているときの背景。赤みで親の目を引く。
const TROUBLE_BACKGROUND: Rgb565 = Rgb565::new(14, 6, 4);
/// 眠っているときの背景。
const SLEEPING_BACKGROUND: Rgb565 = Rgb565::new(1, 3, 6);

const EYE_COLOR: Rgb565 = Rgb565::new(30, 60, 31);
const PUPIL_COLOR: Rgb565 = Rgb565::new(2, 5, 9);
const MOUTH_COLOR: Rgb565 = Rgb565::new(31, 34, 16);
const BROW_COLOR: Rgb565 = Rgb565::new(28, 50, 26);

/// おはなしボタン。仕様どおり画面の右下に置く。
pub const TALK_BUTTON_CENTER: Point = Point::new(272, 192);
pub const TALK_BUTTON_RADIUS: i32 = 40;
/// 押しやすさのため、見た目の円より少し広い範囲を反応させる。
const TALK_BUTTON_TOUCH_MARGIN: i32 = 10;

const BUTTON_IDLE_COLOR: Rgb565 = Rgb565::new(6, 30, 12);
const BUTTON_ACTIVE_COLOR: Rgb565 = Rgb565::new(30, 16, 6);
const BUTTON_DISABLED_COLOR: Rgb565 = Rgb565::new(8, 16, 8);
const BUTTON_MARK_COLOR: Rgb565 = Rgb565::new(31, 63, 31);

/// 指の位置がおはなしボタンの上かどうか。
pub fn contains_talk_button(point: Point) -> bool {
    let reach = TALK_BUTTON_RADIUS + TALK_BUTTON_TOUCH_MARGIN;
    let offset = point - TALK_BUTTON_CENTER;

    offset.x * offset.x + offset.y * offset.y <= reach * reach
}

/// 顔をひとコマ描く。
pub fn draw_face<D>(target: &mut D, frame: &FaceFrame) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let background = background_of(frame.expression);
    target.clear(background)?;

    draw_eye(target, &EyeShape::new(LEFT_EYE_CENTER, frame))?;
    draw_eye(target, &EyeShape::new(RIGHT_EYE_CENTER, frame))?;

    if frame.expression == Expression::Trouble {
        draw_worried_brows(target)?;
    }

    draw_mouth(target, frame, background)?;
    draw_talk_button(target, frame.expression)
}

/// おはなしボタンを描く。押している間だけ色を変えて、録音中だと分かるようにする。
fn draw_talk_button<D>(target: &mut D, expression: Expression) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let color = match expression {
        Expression::Listening => BUTTON_ACTIVE_COLOR,
        // 設定待ちや接続前は押しても始まらないので沈んだ色にする。
        Expression::Sleeping | Expression::Waiting | Expression::Trouble => BUTTON_DISABLED_COLOR,
        _ => BUTTON_IDLE_COLOR,
    };

    Circle::with_center(TALK_BUTTON_CENTER, TALK_BUTTON_RADIUS as u32 * 2)
        .into_styled(PrimitiveStyle::with_fill(color))
        .draw(target)?;

    // 中央のまるいしるし。字が読めなくてもマイクだと分かるようにする。
    Circle::with_center(TALK_BUTTON_CENTER, TALK_BUTTON_RADIUS as u32)
        .into_styled(PrimitiveStyle::with_fill(BUTTON_MARK_COLOR))
        .draw(target)
}

fn background_of(expression: Expression) -> Rgb565 {
    match expression {
        Expression::Sleeping => SLEEPING_BACKGROUND,
        Expression::Trouble => TROUBLE_BACKGROUND,
        _ => CALM_BACKGROUND,
    }
}

/// ひとつの目の描画に必要な寸法。
struct EyeShape {
    center: Point,
    height: u32,
    /// 瞳を描くか。閉じかけの目に瞳を描くとつぶれて見える。
    shows_pupil: bool,
    gaze_offset: i32,
}

impl EyeShape {
    fn new(center: Point, frame: &FaceFrame) -> Self {
        // 完全に閉じても線として見えるよう下限を設ける。
        let height = (EYE_HEIGHT * u32::from(frame.eye_openness) / 100).max(4);

        Self {
            center,
            height,
            shows_pupil: frame.eye_openness >= PUPIL_VISIBLE_OPENNESS,
            gaze_offset: i32::from(frame.gaze_x) * GAZE_TRAVEL / 100,
        }
    }
}

fn draw_eye<D>(target: &mut D, eye: &EyeShape) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let top_left = Point::new(
        eye.center.x - EYE_WIDTH as i32 / 2,
        eye.center.y - eye.height as i32 / 2,
    );

    Ellipse::new(top_left, Size::new(EYE_WIDTH, eye.height))
        .into_styled(PrimitiveStyle::with_fill(EYE_COLOR))
        .draw(target)?;

    if !eye.shows_pupil {
        return Ok(());
    }

    let pupil_center = Point::new(eye.center.x + eye.gaze_offset, eye.center.y);
    Circle::with_center(pupil_center, PUPIL_DIAMETER)
        .into_styled(PrimitiveStyle::with_fill(PUPIL_COLOR))
        .draw(target)
}

/// 困り顔の眉。内側に向かって下がる線で表す。
fn draw_worried_brows<D>(target: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let style = PrimitiveStyle::with_stroke(BROW_COLOR, 6);
    let brow_y = LEFT_EYE_CENTER.y - EYE_HEIGHT as i32 / 2 - 16;

    Line::new(
        Point::new(LEFT_EYE_CENTER.x - 34, brow_y - 8),
        Point::new(LEFT_EYE_CENTER.x + 30, brow_y + 8),
    )
    .into_styled(style)
    .draw(target)?;

    Line::new(
        Point::new(RIGHT_EYE_CENTER.x + 34, brow_y - 8),
        Point::new(RIGHT_EYE_CENTER.x - 30, brow_y + 8),
    )
    .into_styled(style)
    .draw(target)
}

fn draw_mouth<D>(target: &mut D, frame: &FaceFrame, background: Rgb565) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    match frame.expression {
        Expression::Talking => draw_open_mouth(target, frame.mouth_openness),
        Expression::Idle | Expression::Listening => draw_smile(target, background),
        _ => draw_closed_mouth(target),
    }
}

fn draw_open_mouth<D>(target: &mut D, openness: u8) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let height = (MOUTH_MAX_HEIGHT * u32::from(openness) / 100).max(10);
    let top_left = Point::new(
        MOUTH_CENTER.x - MOUTH_WIDTH as i32 / 2,
        MOUTH_CENTER.y - height as i32 / 2,
    );

    Ellipse::new(top_left, Size::new(MOUTH_WIDTH, height))
        .into_styled(PrimitiveStyle::with_fill(MOUTH_COLOR))
        .draw(target)
}

/// 笑った口。楕円の上半分を背景で塗り戻して弧にする。
fn draw_smile<D>(target: &mut D, background: Rgb565) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let height = MOUTH_MAX_HEIGHT;
    let top_left = Point::new(
        MOUTH_CENTER.x - MOUTH_WIDTH as i32 / 2,
        MOUTH_CENTER.y - height as i32 / 2,
    );

    Ellipse::new(top_left, Size::new(MOUTH_WIDTH, height))
        .into_styled(PrimitiveStyle::with_fill(MOUTH_COLOR))
        .draw(target)?;

    Rectangle::new(top_left, Size::new(MOUTH_WIDTH, height / 2))
        .into_styled(PrimitiveStyle::with_fill(background))
        .draw(target)
}

fn draw_closed_mouth<D>(target: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let top_left = Point::new(MOUTH_CENTER.x - MOUTH_WIDTH as i32 / 4, MOUTH_CENTER.y - 3);

    Rectangle::new(top_left, Size::new(MOUTH_WIDTH / 2, 6))
        .into_styled(PrimitiveStyle::with_fill(MOUTH_COLOR))
        .draw(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::face::{Expression, FaceFrame};

    /// 描かれた点を数えるだけの描画先。重ね描きを許すため MockDisplay は使わない。
    struct PixelCounter {
        pixels: Vec<(Point, Rgb565)>,
    }

    impl PixelCounter {
        fn new() -> Self {
            Self { pixels: Vec::new() }
        }

        /// 最後に描かれた色を座標ごとにまとめる。
        fn color_at(&self, point: Point) -> Option<Rgb565> {
            self.pixels
                .iter()
                .rev()
                .find(|(at, _)| *at == point)
                .map(|(_, color)| *color)
        }

        fn count_of(&self, color: Rgb565) -> usize {
            let mut seen = std::collections::BTreeMap::new();
            for (point, drawn) in &self.pixels {
                seen.insert((point.x, point.y), *drawn);
            }
            seen.values().filter(|drawn| **drawn == color).count()
        }
    }

    impl Dimensions for PixelCounter {
        fn bounding_box(&self) -> Rectangle {
            Rectangle::new(Point::zero(), Size::new(SCREEN_WIDTH, SCREEN_HEIGHT))
        }
    }

    impl DrawTarget for PixelCounter {
        type Color = Rgb565;
        type Error = core::convert::Infallible;

        fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
        where
            I: IntoIterator<Item = Pixel<Self::Color>>,
        {
            let bounds = self.bounding_box();
            for Pixel(point, color) in pixels {
                if bounds.contains(point) {
                    self.pixels.push((point, color));
                }
            }
            Ok(())
        }
    }

    fn frame(expression: Expression, eye_openness: u8) -> FaceFrame {
        FaceFrame {
            expression,
            eye_openness,
            mouth_openness: 0,
            gaze_x: 0,
        }
    }

    fn drawn(frame: &FaceFrame) -> PixelCounter {
        let mut target = PixelCounter::new();
        draw_face(&mut target, frame).expect("描画は失敗しない");
        target
    }

    #[test]
    fn open_eyes_show_pupils() {
        let target = drawn(&frame(Expression::Idle, 100));

        assert!(target.count_of(EYE_COLOR) > 0, "白目が描かれるはず");
        assert!(target.count_of(PUPIL_COLOR) > 0, "瞳が描かれるはず");
        assert_eq!(target.color_at(LEFT_EYE_CENTER), Some(PUPIL_COLOR));
    }

    #[test]
    fn closed_eyes_hide_pupils() {
        let target = drawn(&frame(Expression::Idle, 0));

        assert!(target.count_of(EYE_COLOR) > 0, "閉じた目も線として残るはず");
        assert_eq!(target.count_of(PUPIL_COLOR), 0, "閉じた目に瞳は描かない");
    }

    #[test]
    fn eyes_shrink_as_they_close() {
        let wide = drawn(&frame(Expression::Listening, 100)).count_of(EYE_COLOR);
        let narrow = drawn(&frame(Expression::Listening, 40)).count_of(EYE_COLOR);

        assert!(narrow < wide, "細めた目のほうが面積が小さいはず");
    }

    #[test]
    fn gaze_moves_the_pupil_sideways() {
        let looking_left = FaceFrame {
            gaze_x: -100,
            ..frame(Expression::Thinking, 100)
        };
        let looking_right = FaceFrame {
            gaze_x: 100,
            ..frame(Expression::Thinking, 100)
        };

        let left = drawn(&looking_left);
        let right = drawn(&looking_right);

        assert_eq!(
            left.color_at(Point::new(LEFT_EYE_CENTER.x - GAZE_TRAVEL, LEFT_EYE_CENTER.y)),
            Some(PUPIL_COLOR)
        );
        assert_eq!(
            right.color_at(Point::new(RIGHT_EYE_CENTER.x + GAZE_TRAVEL, RIGHT_EYE_CENTER.y)),
            Some(PUPIL_COLOR)
        );
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

        assert!(drawn(&quiet).count_of(MOUTH_COLOR) < drawn(&loud).count_of(MOUTH_COLOR));
    }

    #[test]
    fn smile_only_occupies_the_lower_half() {
        let target = drawn(&frame(Expression::Idle, 100));

        let above = Point::new(MOUTH_CENTER.x, MOUTH_CENTER.y - MOUTH_MAX_HEIGHT as i32 / 4);
        let below = Point::new(MOUTH_CENTER.x, MOUTH_CENTER.y + MOUTH_MAX_HEIGHT as i32 / 4);

        assert_ne!(target.color_at(above), Some(MOUTH_COLOR), "口の上半分は消すはず");
        assert_eq!(target.color_at(below), Some(MOUTH_COLOR));
    }

    #[test]
    fn trouble_face_draws_brows_on_a_warm_background() {
        let target = drawn(&frame(Expression::Trouble, 60));

        assert!(target.count_of(BROW_COLOR) > 0, "困り眉が描かれるはず");
        assert!(target.count_of(TROUBLE_BACKGROUND) > 0, "背景が赤みを帯びるはず");
    }

    #[test]
    fn sleeping_face_stays_dark_and_shut() {
        let target = drawn(&frame(Expression::Sleeping, 0));

        assert!(target.count_of(SLEEPING_BACKGROUND) > 0);
        assert_eq!(target.count_of(PUPIL_COLOR), 0);
    }

    #[test]
    fn talk_button_sits_in_the_lower_right_corner() {
        assert!(TALK_BUTTON_CENTER.x > SCREEN_WIDTH as i32 / 2);
        assert!(TALK_BUTTON_CENTER.y > SCREEN_HEIGHT as i32 / 2);
        assert!(TALK_BUTTON_CENTER.x + TALK_BUTTON_RADIUS < SCREEN_WIDTH as i32);
        assert!(TALK_BUTTON_CENTER.y + TALK_BUTTON_RADIUS < SCREEN_HEIGHT as i32);
    }

    #[test]
    fn talk_button_accepts_touches_on_and_near_it() {
        assert!(contains_talk_button(TALK_BUTTON_CENTER));
        assert!(contains_talk_button(Point::new(
            TALK_BUTTON_CENTER.x + TALK_BUTTON_RADIUS,
            TALK_BUTTON_CENTER.y
        )));
        assert!(!contains_talk_button(Point::new(20, 20)));
        assert!(!contains_talk_button(Point::new(
            TALK_BUTTON_CENTER.x - TALK_BUTTON_RADIUS * 3,
            TALK_BUTTON_CENTER.y
        )));
    }

    #[test]
    fn talk_button_changes_colour_while_listening() {
        let idle = drawn(&frame(Expression::Idle, 100));
        let listening = drawn(&frame(Expression::Listening, 100));

        assert!(idle.count_of(BUTTON_IDLE_COLOR) > 0);
        assert!(listening.count_of(BUTTON_ACTIVE_COLOR) > 0);
        assert_eq!(listening.count_of(BUTTON_IDLE_COLOR), 0);
    }

    #[test]
    fn talk_button_looks_disabled_before_the_session_is_ready() {
        for expression in [Expression::Sleeping, Expression::Waiting, Expression::Trouble] {
            let target = drawn(&frame(expression, 60));
            assert!(
                target.count_of(BUTTON_DISABLED_COLOR) > 0,
                "{expression:?} では沈んだ色になるはず"
            );
        }
    }

    #[test]
    fn stays_inside_the_screen() {
        for expression in [
            Expression::Sleeping,
            Expression::Waiting,
            Expression::Idle,
            Expression::Listening,
            Expression::Thinking,
            Expression::Talking,
            Expression::Trouble,
        ] {
            let target = drawn(&FaceFrame {
                mouth_openness: 100,
                gaze_x: 100,
                ..frame(expression, 100)
            });

            let bounds = target.bounding_box();
            assert!(
                target.pixels.iter().all(|(point, _)| bounds.contains(*point)),
                "{expression:?} が画面外にはみ出した"
            );
        }
    }
}
