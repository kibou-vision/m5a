// CoreS3 の BSP と LVGL を Rust から呼ぶためのヘッダ。
// esp-idf-sys がこの内容を bindgen にかけ、esp_idf_svc::sys::bsp に置く。
#include "bsp/esp-bsp.h"
#include "bsp/display.h"
#include "bsp/touch.h"
#include "esp_lvgl_port.h"
