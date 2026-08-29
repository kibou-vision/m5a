//! 指の動きから画面を切り替えるスワイプを読み取る。

use crate::layout::Point;

/// スワイプと判定する横方向の最小移動量（px）。
pub const SWIPE_THRESHOLD: i16 = 60;

/// スワイプの向き。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwipeDirection {
    Left,
    Right,
}

/// 押し始めた点と離した点からスワイプを判定する。
///
/// 横方向の移動が閾値に届かない、または縦方向の動きが大きすぎて
/// 斜めに近い操作は、意図しない誤検出を避けるためスワイプとみなさない。
pub fn detect_swipe(start: Point, end: Point) -> Option<SwipeDirection> {
    let dx = end.x - start.x;
    let dy = end.y - start.y;

    if dx.abs() < SWIPE_THRESHOLD || dx.abs() < dy.abs() * 2 {
        return None;
    }

    Some(if dx > 0 {
        SwipeDirection::Right
    } else {
        SwipeDirection::Left
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_movement_is_not_a_swipe() {
        let start = Point::new(100, 100);
        let end = Point::new(120, 100);

        assert_eq!(detect_swipe(start, end), None);
    }

    #[test]
    fn diagonal_movement_is_not_a_swipe() {
        let start = Point::new(100, 100);
        let end = Point::new(170, 150);

        assert_eq!(detect_swipe(start, end), None);
    }

    #[test]
    fn long_rightward_movement_is_a_right_swipe() {
        let start = Point::new(50, 100);
        let end = Point::new(150, 105);

        assert_eq!(detect_swipe(start, end), Some(SwipeDirection::Right));
    }

    #[test]
    fn long_leftward_movement_is_a_left_swipe() {
        let start = Point::new(200, 100);
        let end = Point::new(90, 95);

        assert_eq!(detect_swipe(start, end), Some(SwipeDirection::Left));
    }
}
