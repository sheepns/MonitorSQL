// hide the console, if want to debug, mark it.
#![windows_subsystem = "windows"] 

mod app;
mod db;

use app::MonitorApp;
use std::fs;

fn main() -> eframe::Result<()> {
    let config_str = fs::read_to_string("config.toml")
        .expect("無法找到 config.toml，請確認檔案與執行檔放在同一目錄！");
    let app_config: app::AppConfig = toml::from_str(&config_str)
        .expect("config.toml 格式錯誤！");

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1400.0, 950.0])
            .with_title("SQL Server 執行狀況監控面板"),
        ..Default::default()
    };

    eframe::run_native(
        "SQL Monitor",
        options,
        Box::new(|cc| {
            // 強制設定為明亮主題 (Light Theme) 以符合截圖風格
            cc.egui_ctx.set_visuals(eframe::egui::Visuals::light());

            // 動態載入 Windows 系統的微軟正黑體，解決中文字亂碼
            let mut fonts = eframe::egui::FontDefinitions::default();
            if let Ok(font_data) = std::fs::read("C:\\Windows\\Fonts\\msjh.ttc") {
                fonts.font_data.insert("msjh".to_owned(), eframe::egui::FontData::from_owned(font_data));
                if let Some(prop) = fonts.families.get_mut(&eframe::egui::FontFamily::Proportional) {
                    prop.insert(0, "msjh".to_owned());
                }
                if let Some(mono) = fonts.families.get_mut(&eframe::egui::FontFamily::Monospace) {
                    mono.insert(0, "msjh".to_owned());
                }
            }
            cc.egui_ctx.set_fonts(fonts);

            Box::new(MonitorApp::new(app_config))
        }),
    )
}