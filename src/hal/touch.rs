//! 画面へのふれ方を読む。
//!
//! タッチ制御チップの違いは BSP と LVGL が吸収するため、ここでは
//! LVGL が持っている状態を見るだけにする。押している間だけ録音する方式なので、
//! 座標より「押された／離された」の変わり目を取りこぼさないことを優先する。

use esp_idf_svc::sys::bsp;
use m5a_core::layout::Point;

/// 指の状態の変わり目。
///
/// スワイプの判定には離した瞬間の座標が要るため、押されたまま座標が
/// 動いたことも `Moved` として拾い、`Released` にも最後の座標を持たせる。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TouchChange {
    Pressed(Point),
    Moved(Point),
    Released(Point),
}

/// 直前の状態を覚えて変わり目だけを返す。
#[derive(Debug)]
pub struct TouchReader {
    device: *mut bsp::lv_indev_t,
    was_touching: bool,
    last_point: Point,
}

impl TouchReader {
    pub fn new(device: *mut bsp::lv_indev_t) -> Self {
        Self {
            device,
            was_touching: false,
            last_point: Point::new(0, 0),
        }
    }

    /// 1回読み取る。状態も座標も変わっていなければ `None` を返す。
    pub fn poll(&mut self) -> Option<TouchChange> {
        if self.device.is_null() {
            return None;
        }

        let touching = unsafe { bsp::lv_indev_get_state(self.device) }
            == bsp::lv_indev_state_t_LV_INDEV_STATE_PRESSED;

        if !touching {
            if !self.was_touching {
                return None;
            }
            self.was_touching = false;
            return Some(TouchChange::Released(self.last_point));
        }

        let mut at = bsp::lv_point_t { x: 0, y: 0 };
        unsafe { bsp::lv_indev_get_point(self.device, &mut at) };
        let point = Point::new(at.x as i16, at.y as i16);

        let change = if !self.was_touching {
            TouchChange::Pressed(point)
        } else if point != self.last_point {
            TouchChange::Moved(point)
        } else {
            return None;
        };

        self.was_touching = true;
        self.last_point = point;

        Some(change)
    }
}
