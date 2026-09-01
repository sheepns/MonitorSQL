#![windows_subsystem = "windows"] // 隱藏終端機視窗

use eframe::egui;
use egui_extras::{Column, TableBuilder};
use std::fs;
use std::io::Write; // 新增：用於寫入匯出檔案

// --- 排序狀態定義 ---
#[derive(Clone, PartialEq)]
enum SortColumn {
    Original, // 原始檔案讀取順序
    Spid,
    Elapsed,
    Status,
    WaitType,
    WaitResource,
    Blocking,
    Sql,
}

// --- 資料結構定義 ---
#[derive(Clone, Default)]
struct SnapshotData {
    server: String,
    capture_time: String,
    workers: String,
    logical_connections: String,
    ple: String,
    active_temp_tables: String,
    transactions: String,
    request_sessions_count: String,
    sessions: Vec<SessionSnapshotData>,
}

#[derive(Clone, Default)]
struct SessionSnapshotData {
    original_idx: usize, // 紀錄它在 Log 檔中的原始順序，方便還原
    spid: String,
    elapsed: String,
    status: String,
    wait_type: String,
    wait_time_ms: String,
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

struct SnapshotViewerApp {
    loaded_file_name: String,
    loaded_file_path: String, // 新增：保留原始檔案路徑，以利匯出時儲存於同目錄
    snapshots: Vec<SnapshotData>,
    selected_snapshot_idx: usize,
    selected_session: Option<SessionSnapshotData>,
    
    // --- 排序相關狀態 ---
    sort_col: SortColumn,
    sort_asc: bool,
}

impl Default for SnapshotViewerApp {
    fn default() -> Self {
        Self {
            loaded_file_name: String::from("請將 Snapshot Log 檔案拖曳至此視窗"),
            loaded_file_path: String::new(),
            snapshots: Vec::new(),
            selected_snapshot_idx: 0,
            selected_session: None,
            sort_col: SortColumn::Original, // 預設使用原始順序
            sort_asc: true,
        }
    }
}

// 輔助繪圖函式 (Badge)
fn draw_badge(ui: &mut egui::Ui, text: &str, bg_color: egui::Color32, text_color: egui::Color32) {
    egui::Frame::none()
        .fill(bg_color)
        .rounding(4.0)
        .inner_margin(egui::Margin::symmetric(6.0, 1.0))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text).color(text_color).strong());
        });
}

// 輔助繪圖函式 (可排序標題)，回傳是否被點擊以及要排序的欄位
fn ui_sortable_header(
    ui: &mut egui::Ui,
    text: &str,
    col: SortColumn,
    current_sort_col: &SortColumn,
    current_sort_asc: bool,
) -> Option<SortColumn> {
    let is_selected = *current_sort_col == col;
    let arrow = if is_selected {
        if current_sort_asc { " ⬆" } else { " ⬇" }
    } else {
        ""
    };
    
    let mut rich_text = egui::RichText::new(format!("{}{}", text, arrow)).strong();
    if is_selected {
        rich_text = rich_text.color(egui::Color32::from_rgb(22, 101, 192)); // 排序中的欄位顯示藍字
    }

    // 使用 frame(false) 讓按鈕看起來就像一般的文字
    if ui.add_sized(ui.available_size(), egui::Button::new(rich_text).frame(false)).clicked() {
        Some(col)
    } else {
        None
    }
}

impl SnapshotViewerApp {
    // 核心解析邏輯
    fn parse_snapshot_content(&mut self, content: &str) {
        self.snapshots.clear();
        self.selected_snapshot_idx = 0;
        self.selected_session = None;
        self.sort_col = SortColumn::Original; // 載入新檔時重置排序
        self.sort_asc = true;

        let blocks = content.split("==================================================");
        
        for block in blocks {
            let block = block.trim();
            if block.is_empty() {
                continue;
            }

            let mut snapshot = SnapshotData::default();
            let mut current_session = SessionSnapshotData::default();
            let mut in_session_block = false;
            let mut sql_state = 0; 
            let mut session_counter = 0; // 用來記錄原始排序順序

            for line in block.lines() {
                let line_str = line.trim_end(); 

                if line_str.starts_with("Server: ") { snapshot.server = line_str.replace("Server: ", ""); }
                else if line_str.starts_with("Capture Time: ") { snapshot.capture_time = line_str.replace("Capture Time: ", ""); }
                else if line_str.starts_with("Workers: ") { snapshot.workers = line_str.replace("Workers: ", ""); }
                else if line_str.starts_with("Logical Connections: ") { snapshot.logical_connections = line_str.replace("Logical Connections: ", ""); }
                else if line_str.starts_with("Page Life Expectancy: ") { snapshot.ple = line_str.replace("Page Life Expectancy: ", ""); }
                else if line_str.starts_with("Active Temp Tables: ") { snapshot.active_temp_tables = line_str.replace("Active Temp Tables: ", ""); }
                else if line_str.starts_with("Transactions: ") { snapshot.transactions = line_str.replace("Transactions: ", ""); }
                else if line_str.starts_with("Request Sessions: ") { snapshot.request_sessions_count = line_str.replace("Request Sessions: ", ""); }
                else if line_str.starts_with("--- [Session ") {
                    if in_session_block {
                        snapshot.sessions.push(current_session.clone());
                    }
                    current_session = SessionSnapshotData::default();
                    current_session.original_idx = session_counter;
                    session_counter += 1;
                    in_session_block = true;
                    sql_state = 0;
                }
                else if in_session_block {
                    if line_str.starts_with("SPID: ") {
                        let parts: Vec<&str> = line_str.split(" | ").collect();
                        for p in parts {
                            if p.starts_with("SPID: ") { current_session.spid = p.replace("SPID: ", ""); }
                            else if p.starts_with("Elapsed Time: ") { current_session.elapsed = p.replace("Elapsed Time: ", "").replace(" ms", ""); }
                            else if p.starts_with("Status: ") { current_session.status = p.replace("Status: ", ""); }
                        }
                    }
                    else if line_str.starts_with("Wait Type: ") {
                        let parts: Vec<&str> = line_str.split(" | ").collect();
                        for p in parts {
                            if p.starts_with("Wait Type: ") {
                                let w = p.replace("Wait Type: ", "");
                                let w_parts: Vec<&str> = w.split(" (").collect();
                                if w_parts.len() == 2 {
                                    current_session.wait_type = w_parts[0].to_string();
                                    current_session.wait_time_ms = w_parts[1].replace(" ms)", "");
                                } else {
                                    current_session.wait_type = w;
                                }
                            }
                            else if p.starts_with("Last Wait: ") { current_session.last_wait_type = p.replace("Last Wait: ", ""); }
                        }
                    }
                    else if line_str.starts_with("Wait Resource: ") {
                        let parts: Vec<&str> = line_str.split(" | ").collect();
                        for p in parts {
                            if p.starts_with("Wait Resource: ") { current_session.wait_resource = p.replace("Wait Resource: ", ""); }
                            else if p.starts_with("Blocking SPID: ") { current_session.blocking_spid = p.replace("Blocking SPID: ", ""); }
                        }
                    }
                    else if line_str.starts_with("CPU Time: ") {
                        let parts: Vec<&str> = line_str.split(" | ").collect();
                        for p in parts {
                            if p.starts_with("CPU Time: ") { current_session.cpu_time_ms = p.replace("CPU Time: ", "").replace(" ms", ""); }
                            else if p.starts_with("Logical Reads: ") { current_session.logical_reads = p.replace("Logical Reads: ", ""); }
                            else if p.starts_with("Physical Reads: ") { current_session.physical_reads = p.replace("Physical Reads: ", ""); }
                            else if p.starts_with("Writes: ") { current_session.writes = p.replace("Writes: ", ""); }
                        }
                    }
                    else if line_str.starts_with("Row Count: ") {
                        let parts: Vec<&str> = line_str.split(" | ").collect();
                        for p in parts {
                            if p.starts_with("Row Count: ") { current_session.row_count = p.replace("Row Count: ", ""); }
                            else if p.starts_with("Open Trans: ") { current_session.open_trans = p.replace("Open Trans: ", ""); }
                            else if p.starts_with("DOP: ") { current_session.dop = p.replace("DOP: ", ""); }
                        }
                    }
                    else if line_str.starts_with("Client Address: ") {
                        let parts: Vec<&str> = line_str.split(" | ").collect();
                        for p in parts {
                            if p.starts_with("Client Address: ") { current_session.client_address = p.replace("Client Address: ", ""); }
                            else if p.starts_with("Login Name: ") { current_session.login_name = p.replace("Login Name: ", ""); }
                            else if p.starts_with("DB Name: ") { current_session.db_name = p.replace("DB Name: ", ""); }
                        }
                    }
                    else if line_str.starts_with("Command: ") {
                        let parts: Vec<&str> = line_str.split(" | ").collect();
                        for p in parts {
                            if p.starts_with("Command: ") { current_session.command = p.replace("Command: ", ""); }
                            else if p.starts_with("Start Time: ") { current_session.start_time = p.replace("Start Time: ", ""); }
                        }
                    }
                    else if line_str.starts_with("Executing SQL:") { sql_state = 1; }
                    else if line_str.starts_with("Parent Batch SQL:") { sql_state = 2; }
                    else if line_str.starts_with("--------------------------------------------------") || line_str.starts_with("Captured Sessions Details:") || line_str.starts_with("No sessions captured") {
                        // 忽略分隔線
                    }
                    else {
                        if sql_state == 1 {
                            if !current_session.executing_sql.is_empty() { current_session.executing_sql.push('\n'); }
                            current_session.executing_sql.push_str(line_str);
                        } else if sql_state == 2 {
                            if !current_session.parent_batch_sql.is_empty() { current_session.parent_batch_sql.push('\n'); }
                            current_session.parent_batch_sql.push_str(line_str);
                        }
                    }
                }
            }
            if in_session_block {
                snapshot.sessions.push(current_session);
            }

            if !snapshot.server.is_empty() {
                self.snapshots.push(snapshot);
            }
        }
    }

    // 核心排序引擎
    fn apply_sort(&mut self) {
        if self.snapshots.is_empty() { return; }
        
        let col = self.sort_col.clone();
        let asc = self.sort_asc;
        let snapshot = &mut self.snapshots[self.selected_snapshot_idx];
        
        snapshot.sessions.sort_by(|a, b| {
            let cmp = match col {
                SortColumn::Original => a.original_idx.cmp(&b.original_idx),
                SortColumn::Spid => {
                    let a_val: i32 = a.spid.trim().parse().unwrap_or(0);
                    let b_val: i32 = b.spid.trim().parse().unwrap_or(0);
                    a_val.cmp(&b_val)
                },
                SortColumn::Elapsed => {
                    let a_val: i64 = a.elapsed.trim().parse().unwrap_or(0);
                    let b_val: i64 = b.elapsed.trim().parse().unwrap_or(0);
                    a_val.cmp(&b_val)
                },
                SortColumn::Status => a.status.cmp(&b.status),
                SortColumn::WaitType => a.wait_type.cmp(&b.wait_type),
                SortColumn::WaitResource => a.wait_resource.cmp(&b.wait_resource),
                SortColumn::Blocking => {
                    let a_val: i32 = a.blocking_spid.trim().parse().unwrap_or(0);
                    let b_val: i32 = b.blocking_spid.trim().parse().unwrap_or(0);
                    a_val.cmp(&b_val)
                },
                SortColumn::Sql => a.executing_sql.cmp(&b.executing_sql),
            };
            
            // 決定是升冪還是降冪
            if asc { cmp } else { cmp.reverse() }
        });
    }

    // 匯出目前選擇的快照時段
    fn export_current_snapshot(&self) {
        if self.snapshots.is_empty() || self.loaded_file_path.is_empty() { return; }
        let data = &self.snapshots[self.selected_snapshot_idx];
        
        // 解析 Capture Time 取得 HHMMSS，例如 "2026-08-24 13:34:09" -> "133409"
        let time_parts: Vec<&str> = data.capture_time.split(' ').collect();
        let time_suffix = if time_parts.len() == 2 {
            time_parts[1].replace(':', "")
        } else {
            "000000".to_string()
        };

        // 清理 Server 名稱，避免成為無效的檔名字元
        let safe_server_name = data.server.replace(|c: char| !c.is_alphanumeric() && c != '_' && c != '-', "_");

        let path = std::path::Path::new(&self.loaded_file_path);
        let stem = path.file_stem().unwrap_or_default().to_string_lossy();
        let parent = path.parent().unwrap_or(std::path::Path::new(""));
        
        // 組合新檔名：原檔名_HHMMSS_servername.log
        let new_filename = format!("{}_{}_{}.log", stem, time_suffix, safe_server_name);
        let export_path = parent.join(new_filename);

        // 重構並匯出與原本一致的 Log 內容
        let mut log_content = String::new();
        log_content.push_str("==================================================\n");
        log_content.push_str(&format!("Server: {}\n", data.server));
        log_content.push_str(&format!("Capture Time: {}\n", data.capture_time));
        log_content.push_str("--------------------------------------------------\n");
        log_content.push_str(&format!("Workers: {}\n", data.workers));
        log_content.push_str(&format!("Logical Connections: {}\n", data.logical_connections));
        log_content.push_str(&format!("Page Life Expectancy: {}\n", data.ple));
        log_content.push_str(&format!("Active Temp Tables: {}\n", data.active_temp_tables));
        log_content.push_str(&format!("Transactions: {}\n", data.transactions));
        log_content.push_str(&format!("Request Sessions: {}\n", data.request_sessions_count));
        log_content.push_str("--------------------------------------------------\n");
        log_content.push_str("Captured Sessions Details:\n\n");

        if data.sessions.is_empty() {
            log_content.push_str("No sessions captured under current filter criteria.\n");
        } else {
            // 使用目前介面上排序好的資料匯出
            for (i, session) in data.sessions.iter().enumerate() {
                log_content.push_str(&format!("--- [Session {}] ---\n", i + 1));
                log_content.push_str(&format!("SPID: {} | Elapsed Time: {} ms | Status: {}\n", session.spid, session.elapsed, session.status));
                log_content.push_str(&format!("Wait Type: {} ({} ms) | Last Wait: {}\n", session.wait_type, session.wait_time_ms, session.last_wait_type));
                log_content.push_str(&format!("Wait Resource: {} | Blocking SPID: {}\n", session.wait_resource, session.blocking_spid));
                log_content.push_str(&format!("CPU Time: {} ms | Logical Reads: {} | Physical Reads: {} | Writes: {}\n", session.cpu_time_ms, session.logical_reads, session.physical_reads, session.writes));
                log_content.push_str(&format!("Row Count: {} | Open Trans: {} | DOP: {}\n", session.row_count, session.open_trans, session.dop));
                log_content.push_str(&format!("Client Address: {} | Login Name: {} | DB Name: {}\n", session.client_address, session.login_name, session.db_name));
                log_content.push_str(&format!("Command: {} | Start Time: {}\n", session.command, session.start_time));
                log_content.push_str(&format!("Executing SQL:\n{}\n", session.executing_sql.trim()));
                log_content.push_str(&format!("Parent Batch SQL:\n{}\n\n", session.parent_batch_sql.trim()));
            }
        }
        log_content.push_str("==================================================\n\n");

        if let Ok(mut file) = std::fs::File::create(&export_path) {
            let _ = file.write_all(log_content.as_bytes());
        }
    }
}

impl eframe::App for SnapshotViewerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        
        // 拖曳檔案處理
        ctx.input(|i| {
            if let Some(dropped_file) = i.raw.dropped_files.first() {
                if let Some(path) = &dropped_file.path {
                    if let Ok(content) = fs::read_to_string(path) {
                        self.loaded_file_name = format!("載入成功: {}", path.display());
                        self.loaded_file_path = path.to_string_lossy().to_string(); // 記錄路徑
                        self.parse_snapshot_content(&content);
                    } else {
                        self.loaded_file_name = format!("無法讀取檔案: {}", path.display());
                        self.loaded_file_path = String::new();
                    }
                }
            }
        });

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.heading("📸 Snapshot 狀態還原工具");
                ui.add_space(20.0);
                ui.label(egui::RichText::new(&self.loaded_file_name).color(egui::Color32::from_rgb(22, 101, 192)).strong());
            });
            ui.add_space(10.0);
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            if self.snapshots.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label(egui::RichText::new("請將 Snapshot Log 檔案拖曳至此視窗").size(24.0).color(egui::Color32::GRAY));
                });
                return;
            }

            // --- 選擇快照與重現狀態列 ---
            ui.horizontal(|ui| {
                ui.label("選擇快照時段:");
                let current_snap = &self.snapshots[self.selected_snapshot_idx];
                let combo_label = format!("🖥 {} | 🕒 {}", current_snap.server, current_snap.capture_time);
                
                let prev_idx = self.selected_snapshot_idx;
                
                egui::ComboBox::from_id_source("snapshot_combo")
                    .selected_text(combo_label)
                    .width(350.0)
                    .show_ui(ui, |ui| {
                        for (i, snap) in self.snapshots.iter().enumerate() {
                            let label = format!("🖥 {} | 🕒 {}", snap.server, snap.capture_time);
                            ui.selectable_value(&mut self.selected_snapshot_idx, i, label);
                        }
                    });
                    
                // 如果切換了快照，重新套用一次目前的排序條件
                if prev_idx != self.selected_snapshot_idx {
                    self.apply_sort();
                }

                // 【新增：匯出按鈕】利用 right_to_left 將按鈕貼齊右上角
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(egui::RichText::new("💾 匯出此快照").strong()).on_hover_text("將目前顯示的快照時段獨立匯出成 Log 檔").clicked() {
                        self.export_current_snapshot();
                    }
                });
            });
            ui.add_space(10.0);

            // 還原儀表板狀態
            let data = &self.snapshots[self.selected_snapshot_idx];
            egui::Frame::group(ui.style())
                .fill(egui::Color32::from_rgb(248, 249, 250))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let item_width = (ui.available_width() - 100.0) / 6.0; 
                        
                        ui.allocate_ui_with_layout(egui::vec2(item_width, 50.0), egui::Layout::top_down(egui::Align::Center), |ui| {
                            ui.label(egui::RichText::new("Workers").strong());
                            ui.strong(&data.workers);
                        });
                        ui.separator();
                        ui.allocate_ui_with_layout(egui::vec2(item_width, 50.0), egui::Layout::top_down(egui::Align::Center), |ui| {
                            ui.label(egui::RichText::new("Conns").strong());
                            ui.add_space(3.0);
                            draw_badge(ui, &data.logical_connections, egui::Color32::from_rgb(66, 133, 244), egui::Color32::WHITE);
                        });
                        ui.separator();
                        ui.allocate_ui_with_layout(egui::vec2(item_width, 50.0), egui::Layout::top_down(egui::Align::Center), |ui| {
                            ui.label(egui::RichText::new("PLE").strong());
                            ui.add_space(3.0);
                            draw_badge(ui, &data.ple, egui::Color32::from_rgb(52, 168, 83), egui::Color32::WHITE);
                        });
                        ui.separator();
                        ui.allocate_ui_with_layout(egui::vec2(item_width, 50.0), egui::Layout::top_down(egui::Align::Center), |ui| {
                            ui.label(egui::RichText::new("Temp Tbls").strong());
                            ui.add_space(3.0);
                            ui.strong(&data.active_temp_tables);
                        });
                        ui.separator();
                        ui.allocate_ui_with_layout(egui::vec2(item_width, 50.0), egui::Layout::top_down(egui::Align::Center), |ui| {
                            ui.label(egui::RichText::new("Trans").strong());
                            ui.add_space(3.0);
                            ui.strong(&data.transactions);
                        });
                        ui.separator();
                        ui.allocate_ui_with_layout(egui::vec2(item_width, 50.0), egui::Layout::top_down(egui::Align::Center), |ui| {
                            ui.label(egui::RichText::new("Req Sessions").strong().color(egui::Color32::from_rgb(22, 101, 192)));
                            ui.add_space(3.0);
                            ui.strong(&data.request_sessions_count);
                        });
                    });
                });
            
            ui.add_space(10.0);

            // --- 準備繪製 Session 表格與可排序標題 ---
            let mut clicked_col = None;
            let current_sort_col = self.sort_col.clone();
            let current_sort_asc = self.sort_asc;

            TableBuilder::new(ui)
                .striped(true)
                .resizable(true)
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .column(Column::initial(60.0).at_least(60.0))   // Action (Reset)
                .column(Column::initial(60.0).at_least(50.0))   // SPID
                .column(Column::initial(80.0).at_least(60.0))   // Elapsed
                .column(Column::initial(80.0).at_least(80.0))   // Status
                .column(Column::initial(150.0).at_least(100.0)) // Wait Type / Info
                .column(Column::initial(120.0).at_least(100.0)) // Wait Resource
                .column(Column::initial(80.0).at_least(70.0))   // Blocking SPID
                .column(Column::remainder().at_least(150.0))    // SQL
                .min_scrolled_height(0.0)
                .header(28.0, |mut header| {
                    header.col(|ui| { 
                        // 第一欄當作「還原預設排序」的按鈕
                        if ui.add_sized(ui.available_size(), egui::Button::new(egui::RichText::new("Action").strong()).frame(false)).on_hover_text("點擊還原原始排序").clicked() {
                            clicked_col = Some(SortColumn::Original);
                        }
                    });
                    
                    // 使用乾淨的輔助函式，避免借用衝突
                    header.col(|ui| { if let Some(c) = ui_sortable_header(ui, "SPID", SortColumn::Spid, &current_sort_col, current_sort_asc) { clicked_col = Some(c); } });
                    header.col(|ui| { if let Some(c) = ui_sortable_header(ui, "Elapsed", SortColumn::Elapsed, &current_sort_col, current_sort_asc) { clicked_col = Some(c); } });
                    header.col(|ui| { if let Some(c) = ui_sortable_header(ui, "Status", SortColumn::Status, &current_sort_col, current_sort_asc) { clicked_col = Some(c); } });
                    header.col(|ui| { if let Some(c) = ui_sortable_header(ui, "Wait Type", SortColumn::WaitType, &current_sort_col, current_sort_asc) { clicked_col = Some(c); } });
                    header.col(|ui| { if let Some(c) = ui_sortable_header(ui, "Wait Resource", SortColumn::WaitResource, &current_sort_col, current_sort_asc) { clicked_col = Some(c); } });
                    header.col(|ui| { if let Some(c) = ui_sortable_header(ui, "Blocking", SortColumn::Blocking, &current_sort_col, current_sort_asc) { clicked_col = Some(c); } });
                    header.col(|ui| { if let Some(c) = ui_sortable_header(ui, "Executing SQL", SortColumn::Sql, &current_sort_col, current_sort_asc) { clicked_col = Some(c); } });
                })
                .body(|mut body| {
                    for session in &data.sessions {
                        body.row(30.0, |mut row| {
                            row.col(|ui| {
                                ui.style_mut().visuals.widgets.inactive.weak_bg_fill = egui::Color32::from_rgb(226, 232, 240);
                                if ui.add_sized([45.0, 22.0], egui::Button::new(egui::RichText::new("詳細").color(egui::Color32::from_rgb(30, 64, 175)))).clicked() {
                                    self.selected_session = Some(session.clone());
                                }
                            });
                            row.col(|ui| { ui.label(&session.spid); });
                            row.col(|ui| { ui.label(&session.elapsed); });
                            row.col(|ui| { ui.label(&session.status); });
                            row.col(|ui| { 
                                if session.wait_type.trim().is_empty() || session.wait_type == "-" {
                                    ui.label("-");
                                } else {
                                    draw_badge(ui, &format!("{} ({}ms)", session.wait_type, session.wait_time_ms), egui::Color32::from_rgb(241, 245, 249), egui::Color32::from_rgb(51, 65, 85));
                                }
                            });
                            row.col(|ui| { ui.label(&session.wait_resource); });
                            row.col(|ui| { ui.label(&session.blocking_spid); });
                            row.col(|ui| { 
                                let clean_sql = session.executing_sql.replace('\n', " ").replace('\r', "");
                                ui.add(egui::Label::new(clean_sql).truncate(true));
                            });
                        });
                    }
                });

            // --- 處理排序狀態變更 ---
            if let Some(col) = clicked_col {
                if self.sort_col == col {
                    // 如果點擊的是原本正在排序的欄位，則反轉升降冪
                    self.sort_asc = !self.sort_asc;
                } else {
                    // 切換到新欄位，預設為升冪，但 Elapsed 跟 Blocking 預設降冪較符合除錯直覺
                    self.sort_col = col.clone();
                    if col == SortColumn::Elapsed || col == SortColumn::Blocking {
                        self.sort_asc = false; 
                    } else {
                        self.sort_asc = true;
                    }
                }
                self.apply_sort(); // 觸發排序
            }
        });

        // 詳細內容視窗
        if let Some(session) = &self.selected_session {
            let mut is_open = true;
            egui::Window::new(format!("Snapshot Session Detail - SPID: {}", session.spid))
                .open(&mut is_open)
                .default_size([800.0, 600.0])
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        if ui.button("📋 複製所有內容").clicked() {
                            let copy_text = format!(
                                "SPID: {}\nElapsed Time: {} ms\nStatus: {}\nWait Type: {} ({} ms)\nLast Wait Type: {}\nWait Resource: {}\nWait Session ID (Blocking): {}\nCPU Time: {} ms\nLogical Reads: {}\nPhysical Reads: {}\nWrites: {}\nRow Count: {}\nOpen Trans: {}\nDOP: {}\nClient Address: {}\nLogin Name: {}\nDB Name: {}\nCommand: {}\nStart Time: {}\n\n--- Executing SQL ---\n{}\n\n--- Parent Batch SQL ---\n{}",
                                session.spid, session.elapsed, session.status, session.wait_type, session.wait_time_ms,
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
                            ui.label("SPID:"); ui.label(&session.spid); ui.end_row();
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

// --- 載入系統字型 (解決中文方塊與模糊問題) ---
fn setup_custom_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    if let Ok(font_data) = std::fs::read("C:\\Windows\\Fonts\\msjh.ttc") {
        fonts.font_data.insert(
            "msjh".to_owned(),
            egui::FontData::from_owned(font_data),
        );

        if let Some(prop) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
            prop.insert(0, "msjh".to_owned());
        }
        if let Some(mono) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
            mono.insert(0, "msjh".to_owned());
        }
    }

    ctx.set_fonts(fonts);
}

// --- 應用程式進入點 ---
fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1200.0, 800.0]),
        ..Default::default()
    };
    
    eframe::run_native(
        "Snapshot Viewer",
        options,
        Box::new(|cc| {
            setup_custom_fonts(&cc.egui_ctx);
            Box::new(SnapshotViewerApp::default())
        }),
    )
}