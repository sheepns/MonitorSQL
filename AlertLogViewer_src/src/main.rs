#![windows_subsystem = "windows"] // 隱藏終端機視窗

use eframe::egui;
use egui_extras::{Column, TableBuilder};
use std::fs;

// 定義告警類別
#[derive(Clone)]
enum AlertEntry {
    Session(SessionAlertData),
    Worker(WorkerAlertData),
}

#[derive(Clone, Default)]
struct SessionAlertData {
    log_time: String,
    server: String,
    spid: String,
    elapsed: String,
    status: String,
    wait_type: String,
    wait_time_ms: String, // Wait Type 括號裡的時間
    last_wait_type: String,
    wait_resource: String,
    blocking_spid: String,
    cpu_time_ms: String,
    logical_reads: String,
    physical_reads: String,
    writes: String,
    row_count: String,
    open_trans: String,
    dop: String,
    client_address: String,
    login_name: String,
    db_name: String,
    command: String,
    start_time: String,
    executing_sql: String,
    parent_batch_sql: String,
}

#[derive(Clone, Default)]
struct WorkerAlertData {
    log_time: String,
    server: String,
    usage_msg: String,
}

struct LogViewerApp {
    loaded_file_name: String,
    alerts: Vec<AlertEntry>,
    selected_session: Option<SessionAlertData>,
}

impl Default for LogViewerApp {
    fn default() -> Self {
        Self {
            loaded_file_name: String::from("請將 Alert Log 檔案拖曳至此視窗"),
            alerts: Vec::new(),
            selected_session: None,
        }
    }
}

impl LogViewerApp {
    // 核心解析邏輯：將讀進來的字串轉換為結構化資料
    fn parse_log_content(&mut self, content: &str) {
        self.alerts.clear();
        
        // 使用分隔線切出每一個告警區塊
        let blocks = content.split("==================================================");
        
        for block in blocks {
            let block = block.trim();
            if block.is_empty() {
                continue;
            }

            if block.contains("Worker Threads usage is high") {
                // 解析 Worker Alert
                let mut data = WorkerAlertData::default();
                for line in block.lines() {
                    if line.starts_with('[') && line.contains("Server:") {
                        let parts: Vec<&str> = line.split("] Server: ").collect();
                        if parts.len() == 2 {
                            data.log_time = parts[0].trim_start_matches('[').to_string();
                            let rest: Vec<&str> = parts[1].split(" | ").collect();
                            if rest.len() >= 2 {
                                data.server = rest[0].to_string();
                                data.usage_msg = rest[1].replace("ALERT! ", "").to_string();
                            }
                        }
                    }
                }
                self.alerts.push(AlertEntry::Worker(data));
            } else if block.contains("Alert! Session Elapsed:") {
                // 解析 Session Alert
                let mut data = SessionAlertData::default();
                let mut current_sql_field = 0; // 1: Executing, 2: Parent

                for line in block.lines() {
                    if line.starts_with('[') && line.contains("SPID:") {
                        let parts: Vec<&str> = line.split("] Server: ").collect();
                        if parts.len() == 2 {
                            data.log_time = parts[0].trim_start_matches('[').to_string();
                            let rest: Vec<&str> = parts[1].split(" | ").collect();
                            if rest.len() >= 3 {
                                data.server = rest[0].to_string();
                                data.spid = rest[1].replace("SPID: ", "");
                                data.elapsed = rest[2].replace("Alert! Session Elapsed: ", "").replace(" ms", "");
                            }
                        }
                    } else if line.starts_with("\tStatus: ") { data.status = line.replace("\tStatus: ", ""); }
                      else if line.starts_with("\tWaitType: ") { 
                          let raw = line.replace("\tWaitType: ", "");
                          let w_parts: Vec<&str> = raw.split(" (Time: ").collect();
                          if w_parts.len() == 2 {
                              data.wait_type = w_parts[0].to_string();
                              data.wait_time_ms = w_parts[1].replace(" ms)", "");
                          } else {
                              data.wait_type = raw;
                          }
                      }
                      else if line.starts_with("\tLastWaitType: ") { data.last_wait_type = line.replace("\tLastWaitType: ", ""); }
                      else if line.starts_with("\tWaitResource: ") { data.wait_resource = line.replace("\tWaitResource: ", ""); }
                      else if line.starts_with("\tWaitSessionID (Blocking): ") { data.blocking_spid = line.replace("\tWaitSessionID (Blocking): ", ""); }
                      else if line.starts_with("\tCPU Time: ") { data.cpu_time_ms = line.replace("\tCPU Time: ", "").replace(" ms", ""); }
                      else if line.starts_with("\tLogical Reads: ") { data.logical_reads = line.replace("\tLogical Reads: ", ""); }
                      else if line.starts_with("\tPhysical Reads: ") { data.physical_reads = line.replace("\tPhysical Reads: ", ""); }
                      else if line.starts_with("\tWrites: ") { data.writes = line.replace("\tWrites: ", ""); }
                      else if line.starts_with("\tRowCount: ") { data.row_count = line.replace("\tRowCount: ", ""); }
                      else if line.starts_with("\tOpen Trans: ") { data.open_trans = line.replace("\tOpen Trans: ", ""); }
                      else if line.starts_with("\tDOP: ") { data.dop = line.replace("\tDOP: ", ""); }
                      else if line.starts_with("\tClient Address: ") { data.client_address = line.replace("\tClient Address: ", ""); }
                      else if line.starts_with("\tLogin Name: ") { data.login_name = line.replace("\tLogin Name: ", ""); }
                      else if line.starts_with("\tDB Name: ") { data.db_name = line.replace("\tDB Name: ", ""); }
                      else if line.starts_with("\tCommand: ") { data.command = line.replace("\tCommand: ", ""); }
                      else if line.starts_with("\tStart Time: ") { data.start_time = line.replace("\tStart Time: ", ""); }
                      else if line.starts_with("\tExecuting SQL: ") {
                          current_sql_field = 1;
                          data.executing_sql = line.replace("\tExecuting SQL: ", "");
                      }
                      else if line.starts_with("\tParent Batch SQL: ") {
                          current_sql_field = 2;
                          data.parent_batch_sql = line.replace("\tParent Batch SQL: ", "");
                      }
                      else if current_sql_field == 1 {
                          data.executing_sql.push('\n');
                          data.executing_sql.push_str(line);
                      }
                      else if current_sql_field == 2 {
                          data.parent_batch_sql.push('\n');
                          data.parent_batch_sql.push_str(line);
                      }
                }
                self.alerts.push(AlertEntry::Session(data));
            }
        }
    }
}

impl eframe::App for LogViewerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        
        // 偵測拖曳檔案 (Drag & Drop)
        ctx.input(|i| {
            if let Some(dropped_file) = i.raw.dropped_files.first() {
                if let Some(path) = &dropped_file.path {
                    if let Ok(content) = fs::read_to_string(path) {
                        self.loaded_file_name = format!("載入成功: {}", path.display());
                        self.parse_log_content(&content);
                    } else {
                        self.loaded_file_name = format!("無法讀取檔案: {}", path.display());
                    }
                }
            }
        });

        // 頂部面板：顯示拖曳提示與目前檔案名稱
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.heading("📂 Alert Log 解析工具");
                ui.add_space(20.0);
                ui.label(egui::RichText::new(&self.loaded_file_name).color(egui::Color32::from_rgb(22, 101, 192)).strong());
            });
            ui.add_space(10.0);
        });

        // 中央面板：顯示資料表格
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.alerts.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label(egui::RichText::new("請將 log 檔案拖曳至此視窗").size(24.0).color(egui::Color32::GRAY));
                });
                return;
            }

            TableBuilder::new(ui)
                .striped(true)
                .resizable(true)
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .column(Column::initial(60.0).at_least(60.0))   // Action (詳細按鈕移至第一位)
                .column(Column::initial(150.0).at_least(150.0)) // Time
                .column(Column::initial(80.0).at_least(80.0))   // Server
                .column(Column::initial(60.0).at_least(50.0))   // SPID
                .column(Column::initial(70.0).at_least(60.0))   // Elapsed
                .column(Column::initial(80.0).at_least(80.0))   // Status
                .column(Column::initial(120.0).at_least(100.0)) // Wait Type / Alert Msg
                .column(Column::initial(120.0).at_least(100.0)) // Wait Resource
                .column(Column::initial(80.0).at_least(70.0))   // Wait SPID
                .column(Column::remainder().at_least(150.0))    // SQL (變為動態延伸餘裕寬度)
                .min_scrolled_height(0.0)
                .header(28.0, |mut header| {
                    header.col(|ui| { ui.strong("Action"); });
                    header.col(|ui| { ui.strong("紀錄時間點"); });
                    header.col(|ui| { ui.strong("主機名稱"); });
                    header.col(|ui| { ui.strong("SPID"); });
                    header.col(|ui| { ui.strong("Elapsed"); });
                    header.col(|ui| { ui.strong("Status"); });
                    header.col(|ui| { ui.strong("Wait Type / Info"); });
                    header.col(|ui| { ui.strong("Wait Resource"); });
                    header.col(|ui| { ui.strong("Wait SPID"); });
                    header.col(|ui| { ui.strong("Executing SQL"); });
                })
                .body(|mut body| {
                    for alert in &self.alerts {
                        body.row(30.0, |mut row| {
                            match alert {
                                AlertEntry::Session(session) => {
                                    row.col(|ui| {
                                        ui.style_mut().visuals.widgets.inactive.weak_bg_fill = egui::Color32::from_rgb(226, 232, 240);
                                        if ui.add_sized([45.0, 22.0], egui::Button::new(egui::RichText::new("詳細").color(egui::Color32::from_rgb(30, 64, 175)))).clicked() {
                                            self.selected_session = Some(session.clone());
                                        }
                                    });
                                    row.col(|ui| { ui.label(&session.log_time); });
                                    row.col(|ui| { ui.label(&session.server); });
                                    row.col(|ui| { ui.label(&session.spid); });
                                    row.col(|ui| { ui.label(&session.elapsed); });
                                    row.col(|ui| { ui.label(&session.status); });
                                    row.col(|ui| { 
                                        if session.wait_type.trim().is_empty() {
                                            ui.label("-");
                                        } else {
                                            ui.label(format!("{} ({}ms)", session.wait_type, session.wait_time_ms));
                                        }
                                    });
                                    row.col(|ui| { ui.label(&session.wait_resource); });
                                    row.col(|ui| { ui.label(&session.blocking_spid); });
                                    row.col(|ui| { 
                                        let clean_sql = session.executing_sql.replace('\n', " ").replace('\r', "");
                                        ui.add(egui::Label::new(clean_sql).truncate(true));
                                    });
                                },
                                AlertEntry::Worker(worker) => {
                                    // 修正點：使用 _ui 讓編譯器知道這是一個不會被使用的變數
                                    row.col(|_ui| { /* Worker 無詳細內容，留白 */ });
                                    row.col(|ui| { ui.label(&worker.log_time); });
                                    row.col(|ui| { ui.label(&worker.server); });
                                    row.col(|ui| { ui.label("-"); });
                                    row.col(|ui| { ui.label("-"); });
                                    row.col(|ui| { ui.label("-"); });
                                    // 將 Worker 的告警訊息顯示在 Wait Type 欄位，並加上醒目顏色
                                    row.col(|ui| { ui.label(egui::RichText::new(&worker.usage_msg).color(egui::Color32::RED).strong()); });
                                    row.col(|ui| { ui.label("-"); });
                                    row.col(|ui| { ui.label("-"); });
                                    row.col(|ui| { ui.label("-"); });
                                }
                            }
                        });
                    }
                });
        });

        // 詳細內容視窗 (與 MonitorSQL 相同體驗)
        if let Some(session) = &self.selected_session {
            let mut is_open = true;
            egui::Window::new(format!("Session Detail - SPID: {}", session.spid))
                .open(&mut is_open)
                .default_size([800.0, 600.0])
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        if ui.button("📋 複製所有內容").clicked() {
                            let copy_text = format!(
                                "Log Time: {}\nServer: {}\nSPID: {}\nElapsed Time: {} ms\nStatus: {}\nWait Type: {} ({} ms)\nLast Wait Type: {}\nWait Resource: {}\nWait Session ID (Blocking): {}\nCPU Time: {} ms\nLogical Reads: {}\nPhysical Reads: {}\nWrites: {}\nRow Count: {}\nOpen Trans: {}\nDOP: {}\nClient Address: {}\nLogin Name: {}\nDB Name: {}\nCommand: {}\nStart Time: {}\n\n--- Executing SQL ---\n{}\n\n--- Parent Batch SQL ---\n{}",
                                session.log_time, session.server, session.spid, session.elapsed, session.status, session.wait_type, session.wait_time_ms,
                                session.last_wait_type, session.wait_resource, session.blocking_spid, session.cpu_time_ms,
                                session.logical_reads, session.physical_reads, session.writes, session.row_count,
                                session.open_trans, session.dop, session.client_address, session.login_name,
                                session.db_name, session.command, session.start_time,
                                session.executing_sql.trim(), session.parent_batch_sql.trim()
                            );
                            ui.output_mut(|o| o.copied_text = copy_text);
                        }
                    });
                    
                    ui.separator();

                    egui::ScrollArea::vertical().show(ui, |ui| {
                        egui::Grid::new("detail_grid").num_columns(2).striped(true).show(ui, |ui| {
                            ui.label("Log Time:"); ui.label(&session.log_time); ui.end_row();
                            ui.label("Server Name:"); ui.label(&session.server); ui.end_row();
                            ui.label("Elapsed Time (ms):"); ui.label(&session.elapsed); ui.end_row();
                            ui.label("Status:"); ui.label(&session.status); ui.end_row();
                            ui.label("Wait Type:"); ui.label(&session.wait_type); ui.end_row();
                            ui.label("Wait Time (ms):"); ui.label(&session.wait_time_ms); ui.end_row();
                            ui.label("Last Wait Type:"); ui.label(&session.last_wait_type); ui.end_row();
                            ui.label("Wait Resource:"); ui.label(&session.wait_resource); ui.end_row();
                            ui.label("Wait Session ID (Blocking):"); ui.label(&session.blocking_spid); ui.end_row();
                            ui.label("CPU Time (ms):"); ui.label(&session.cpu_time_ms); ui.end_row();
                            ui.label("Logical Reads:"); ui.label(&session.logical_reads); ui.end_row();
                            ui.label("Physical Reads:"); ui.label(&session.physical_reads); ui.end_row();
                            ui.label("Writes:"); ui.label(&session.writes); ui.end_row();
                            ui.label("Row Count:"); ui.label(&session.row_count); ui.end_row();
                            ui.label("Open Trans:"); ui.label(&session.open_trans); ui.end_row();
                            ui.label("DOP:"); ui.label(&session.dop); ui.end_row();
                            ui.label("Client Net Address:"); ui.label(&session.client_address); ui.end_row();
                            ui.label("Login Name:"); ui.label(&session.login_name); ui.end_row();
                            ui.label("Database Name:"); ui.label(&session.db_name); ui.end_row();
                            ui.label("Command:"); ui.label(&session.command); ui.end_row();
                            ui.label("Start Time:"); ui.label(&session.start_time); ui.end_row();
                        });

                        ui.separator();
                        ui.heading("Executing SQL Statement:");
                        ui.code(&session.executing_sql);

                        ui.separator();
                        ui.heading("Parent Batch SQL:");
                        ui.code(&session.parent_batch_sql);
                    });
                });

            if !is_open {
                self.selected_session = None;
            }
        }
    }
}

fn main() -> eframe::Result<()> {
    // 若要在 Windows Server 上執行，可解除下方註解套用 DX11 WARP
    // unsafe {
    //     std::env::set_var("WGPU_BACKEND", "dx11");
    //     std::env::set_var("WGPU_ADAPTER_NAME", "WARP");
    //     std::env::set_var("WGPU_POWER_PREF", "low");
    // }

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 700.0])
            .with_title("Alert Log Viewer"),
        // 開啟拖曳檔案支援
        ..Default::default()
    };

    eframe::run_native(
        "Alert Log Viewer",
        options,
        Box::new(|cc| {
            cc.egui_ctx.set_visuals(eframe::egui::Visuals::light());

            // 載入微軟正黑體，防止中文亂碼
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

            Box::new(LogViewerApp::default())
        }),
    )
}