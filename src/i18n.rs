use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    #[default]
    Zh,
    En,
}

impl Language {
    /// Best guess from the Windows user UI language: English if the primary
    /// language id is `LANG_ENGLISH`, Chinese otherwise.
    pub fn system_default() -> Self {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetUserDefaultUILanguage() -> u16;
        }

        // SAFETY: no arguments; returns the current user's UI language id.
        let langid = unsafe { GetUserDefaultUILanguage() };
        if langid & 0x03FF == 0x0009 {
            Language::En
        } else {
            Language::Zh
        }
    }

    pub fn text(self) -> Text {
        match self {
            Language::Zh => Text {
                net_up: "上传",
                net_down: "下载",
                cpu: "CPU",
                cpu_temp: "温度",
                mem: "内存",
                gpu: "GPU",
                vram: "显存",
                gpu_temp: "GPU温",
                metrics: "显示数据",
                layout: "布局",
                vertical: "竖排",
                horizontal: "横排",
                grid: "竖两排",
                spacing: "间距",
                spacing_loose: "宽松",
                spacing_tight: "紧凑",
                align: "对齐",
                align_left: "向左对齐",
                align_center: "居中",
                align_right: "向右对齐",
                opacity: "透明度",
                language: "语言",
                autostart: "开机自启",
                quit: "退出",
                tray_toggle: "显示 / 隐藏",
            },
            Language::En => Text {
                net_up: "Up",
                net_down: "Down",
                cpu: "CPU",
                cpu_temp: "Temp",
                mem: "Mem",
                gpu: "GPU",
                vram: "VRAM",
                gpu_temp: "GPU T",
                metrics: "Show",
                layout: "Layout",
                vertical: "Vertical",
                horizontal: "Horizontal",
                grid: "2 cols",
                spacing: "Spacing",
                spacing_loose: "Loose",
                spacing_tight: "Tight",
                align: "Align",
                align_left: "Left",
                align_center: "Center",
                align_right: "Right",
                opacity: "Opacity",
                language: "Language",
                autostart: "Start with Windows",
                quit: "Quit",
                tray_toggle: "Show / Hide",
            },
        }
    }
}

/// All user-facing strings for one language.
pub struct Text {
    pub net_up: &'static str,
    pub net_down: &'static str,
    pub cpu: &'static str,
    pub cpu_temp: &'static str,
    pub mem: &'static str,
    pub gpu: &'static str,
    pub vram: &'static str,
    pub gpu_temp: &'static str,
    pub metrics: &'static str,
    pub layout: &'static str,
    pub vertical: &'static str,
    pub horizontal: &'static str,
    pub grid: &'static str,
    pub spacing: &'static str,
    pub spacing_loose: &'static str,
    pub spacing_tight: &'static str,
    pub align: &'static str,
    pub align_left: &'static str,
    pub align_center: &'static str,
    pub align_right: &'static str,
    pub opacity: &'static str,
    pub language: &'static str,
    pub autostart: &'static str,
    pub quit: &'static str,
    pub tray_toggle: &'static str,
}
