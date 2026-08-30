//! 指の動きから画面を切り替えるスワイプを読み取る。

use crate::layout::Point;

/// スワイプと判定する縦方向の最小移動量（px）。
pub const SWIPE_THRESHOLD: i16 = 60;

/// スワイプの向き。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwipeDirection {
    Up,
    Down,
}

/// 押し始めた点と離した点からスワイプを判定する。
///
/// 縦方向の移動が閾値に届かない、または横方向の動きが大きすぎて
/// 斜めに近い操作は、意図しない誤検出を避けるためスワイプとみなさない。
pub fn detect_swipe(start: Point, end: Point) -> Option<SwipeDirection> {
    let dx = end.x - start.x;
    let dy = end.y - start.y;

    if dy.abs() < SWIPE_THRESHOLD || dy.abs() < dx.abs() * 2 {
        return None;
    }

    Some(if dy > 0 {
        SwipeDirection::Down
    } else {
        SwipeDirection::Up
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_movement_is_not_a_swipe() {
        let start = Point::new(100, 100);
        let end = Point::new(100, 120);

        assert_eq!(detect_swipe(start, end), None);
    }

    #[test]
    fn diagonal_movement_is_not_a_swipe() {
        let start = Point::new(100, 100);
        let end = Point::new(150, 170);

        assert_eq!(detect_swipe(start, end), None);
    }

    #[test]
    fn long_downward_movement_is_a_down_swipe() {
        let start = Point::new(100, 50);
        let end = Point::new(105, 150);

        assert_eq!(detect_swipe(start, end), Some(SwipeDirection::Down));
    }

    #[test]
    fn long_upward_movement_is_an_up_swipe() {
        let start = Point::new(100, 200);
        let end = Point::new(95, 90);

        assert_eq!(detect_swipe(start, end), Some(SwipeDirection::Up));
    }
}
