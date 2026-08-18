use eframe::egui;
use egui_extras::{Column, TableBuilder};
use serde::Deserialize;
use std::fs::OpenOptions;
use std::io::Write;
use chrono::Local;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};
use tokio::sync::mpsc;

use crate::db;

fn default_port() -> u16 { 1433 }
fn default_alert_worker_percent() -> u64 { 40 } 

#[derive(Deserialize, Clone)]
pub struct AppConfig {
    pub servers: Vec<ServerConfig>,
}

#[derive(Deserialize, Clone)]
pub struct ServerConfig {
    pub id: String,
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    pub username: String,
    pub password: String,
    pub filter_elapsed_ms: u64,
    pub alert_elapsed_ms: u64,
    #[serde(default = "default_alert_worker_percent")]
    pub alert_worker_percent: u64,
    pub default_interval_sec: u64,
}

#[derive(Clone)]
pub struct ServerController {
    pub is_paused: Arc<AtomicBool>,
    pub interval_sec: Arc<AtomicU64>,
    pub filter_elapsed_ms: Arc<AtomicU64>,
    pub alert_elapsed_ms: Arc<AtomicU64>,
    pub alert_worker_percent: Arc<AtomicU64>, 
}

#[derive(Clone, Default)]
pub struct DashboardData {
    pub capture_time: String,
    pub sessions: Vec<SessionData>,
    pub active_workers: u32,
    pub max_threads: u32,
    pub workers_percent: String,
    pub conn_logical: i64,
    pub page_life_expectancy: i64,
    pub active_temp_tables: i64,
    pub transactions: i64,
    pub raw_session_count: usize, 
    pub is_alerting: bool,
}

#[derive(Clone, Default)]
pub struct SessionData {
    pub session_id: i16,
    pub elapsed_time_ms: i32,
    pub status: String,
    pub wait_type: String,
    pub wait_time_ms: i64,
    pub executing_sql: String,
    pub wait_session_id: i16,
    pub last_wait_type: String,
    pub wait_resource: String,
    pub parent_batch_sql: String,
    pub cpu_time_ms: i32,
    pub logical_reads: i64,
    pub physical_reads: i64,
    pub writes: i64,
    pub row_count: i64,
    pub open_transaction_count: i32,
    pub dop: i16,
    pub client_net_address: String,
    pub login_name: String,
    pub database_name: String,
    pub command: String,
    pub start_time: String,
    pub capture_time: String,
}

pub struct MonitorApp {
    server_configs: Vec<ServerConfig>,
    server_data: Vec<DashboardData>,
    controllers: Vec<ServerController>,
    receivers: Vec<mpsc::Receiver<DashboardData>>,
    selected_session: Option<SessionData>,
    _rt: tokio::runtime::Runtime,
}

fn draw_badge(ui: &mut egui::Ui, text: &str, bg_color: egui::Color32, text_color: egui::Color32) {
    egui::Frame::none()
        .fill(bg_color)
        .rounding(4.0)
        .inner_margin(egui::Margin::symmetric(6.0, 1.0)) 
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text).color(text_color).strong());
        });
}

// 實作快照寫入功能
fn write_snapshot_log(server_id: &str, data: &DashboardData) {
    let now = Local::now();
    let file_name = now.format("snapshot_log_%Y%m%d.log").to_string();

    let mut log_content = String::new();
    log_content.push_str("==================================================\n");
    log_content.push_str(&format!("Server: {}\n", server_id));
    log_content.push_str(&format!("Capture Time: {}\n", data.capture_time));
    log_content.push_str("--------------------------------------------------\n");
    log_content.push_str(&format!("Workers: {} / {} ({})\n", data.active_workers, data.max_threads, data.workers_percent));
    log_content.push_str(&format!("Logical Connections: {}\n", data.conn_logical));
    log_content.push_str(&format!("Page Life Expectancy: {}\n", data.page_life_expectancy));
    log_content.push_str(&format!("Active Temp Tables: {}\n", data.active_temp_tables));
    log_content.push_str(&format!("Transactions: {}\n", data.transactions));
    log_content.push_str(&format!("Request Sessions: {}\n", data.raw_session_count));
    log_content.push_str("--------------------------------------------------\n");
    log_content.push_str("Captured Sessions Details:\n\n");

    if data.sessions.is_empty() {
        log_content.push_str("No sessions captured under current filter criteria.\n");
    } else {
        for (i, session) in data.sessions.iter().enumerate() {
            log_content.push_str(&format!("--- [Session {}] ---\n", i + 1));
            log_content.push_str(&format!("SPID: {} | Elapsed Time: {} ms | Status: {}\n", session.session_id, session.elapsed_time_ms, session.status));
            log_content.push_str(&format!("Wait Type: {} ({} ms) | Last Wait: {}\n", session.wait_type, session.wait_time_ms, session.last_wait_type));
            log_content.push_str(&format!("Wait Resource: {} | Blocking SPID: {}\n", session.wait_resource, session.wait_session_id));
            log_content.push_str(&format!("CPU Time: {} ms | Logical Reads: {} | Physical Reads: {} | Writes: {}\n", session.cpu_time_ms, session.logical_reads, session.physical_reads, session.writes));
            log_content.push_str(&format!("Row Count: {} | Open Trans: {} | DOP: {}\n", session.row_count, session.open_transaction_count, session.dop));
            log_content.push_str(&format!("Client Address: {} | Login Name: {} | DB Name: {}\n", session.client_net_address, session.login_name, session.database_name));
            log_content.push_str(&format!("Command: {} | Start Time: {}\n", session.command, session.start_time));
            log_content.push_str(&format!("Executing SQL:\n{}\n", session.executing_sql.trim()));
            log_content.push_str(&format!("Parent Batch SQL:\n{}\n\n", session.parent_batch_sql.trim()));
        }
    }
    log_content.push_str("==================================================\n\n");

    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(file_name) {
        let _ = file.write_all(log_content.as_bytes());
    }
}

impl MonitorApp {
    pub fn new(config: AppConfig) -> Self {
        let rt = tokio::runtime::Runtime::new().expect("無法建立 Tokio Runtime");
        
        let mut server_configs = config.servers;
        server_configs.truncate(4);
        let server_count = server_configs.len();

        let server_data = vec![DashboardData::default(); server_count];
        let mut controllers = Vec::with_capacity(server_count);
        let mut receivers = Vec::with_capacity(server_count);

        for (_, srv_cfg) in server_configs.iter().enumerate() {
            let (tx, rx) = mpsc::channel(10);
            
            let controller = ServerController {
                is_paused: Arc::new(AtomicBool::new(false)),
                interval_sec: Arc::new(AtomicU64::new(srv_cfg.default_interval_sec)),
                filter_elapsed_ms: Arc::new(AtomicU64::new(srv_cfg.filter_elapsed_ms)),
                alert_elapsed_ms: Arc::new(AtomicU64::new(srv_cfg.alert_elapsed_ms)),
                alert_worker_percent: Arc::new(AtomicU64::new(srv_cfg.alert_worker_percent)),
            };

            controllers.push(controller.clone());
            receivers.push(rx);

            let cfg = srv_cfg.clone();
            let ctrl = controller.clone();
            rt.spawn(async move {
                db::monitor_server(cfg, ctrl, tx).await;
            });
        }

        Self {
            server_configs,
            server_data,
            controllers,
            receivers,
            selected_session: None,
            _rt: rt,
        }
    }
}

impl eframe::App for MonitorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(std::time::Duration::from_millis(500));

        for (i, rx) in self.receivers.iter_mut().enumerate() {
            while let Ok(data) = rx.try_recv() {
                self.server_data[i] = data;
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            let spacing = 15.0;
            let available_width = ui.available_width();
            let server_count = self.server_configs.len();
            
            let num_columns = if server_count == 1 { 1 } else { 2 };
            
            let col_width = if num_columns == 1 {
                available_width - 10.0
            } else {
                (available_width - spacing - 10.0) / 2.0
            };

            egui::Grid::new("server_grid")
                .num_columns(num_columns)
                .spacing([spacing, spacing])
                .min_col_width(col_width)
                .max_col_width(col_width)
                .show(ui, |ui| {
                    for i in 0..server_count {
                        let config = &self.server_configs[i];
                        let data = &self.server_data[i];
                        let ctrl = &self.controllers[i];

                        let stroke_color = if data.is_alerting {
                            egui::Color32::from_rgb(220, 53, 69)
                        } else {
                            egui::Color32::from_gray(200)
                        };

                        egui::Frame::group(ui.style())
                            .stroke(egui::Stroke::new(if data.is_alerting { 2.5_f32 } else { 1.0_f32 }, stroke_color))
                            .fill(egui::Color32::WHITE)
                            .show(ui, |ui| {
                                ui.set_width(col_width - 15.0);
                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.heading(egui::RichText::new(format!("🖥️ {}", config.id)).color(egui::Color32::from_rgb(33, 37, 41)));
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            
                                            // 快照按鈕 (使用 right_to_left 排版，所以先加入的元件會在最右邊)
                                            if ui.button("📸 快照").clicked() {
                                                write_snapshot_log(&config.id, data);
                                            }
                                            
                                            // 原本的時間標籤，會出現在按鈕的左邊
                                            ui.label(egui::RichText::new(format!("🕒 {}", data.capture_time)).color(egui::Color32::GRAY));
                                        });
                                    });
                                    
                                    ui.separator();
                                    
                                    ui.horizontal(|ui| {
                                        let item_width = (col_width - 100.0) / 5.0; 
                                        
                                        ui.allocate_ui_with_layout(egui::vec2(item_width, 50.0), egui::Layout::top_down(egui::Align::Center), |ui| {
                                            ui.label(egui::RichText::new("Workers").strong());
                                            ui.strong(format!("{} / {}", data.active_workers, data.max_threads));
                                            
                                            let current_worker_pct = if data.max_threads > 0 {
                                                ((data.active_workers as f64 / data.max_threads as f64) * 100.0) as u64
                                            } else { 0 };
                                            let alert_worker_pct = ctrl.alert_worker_percent.load(Ordering::Relaxed);
                                            
                                            let pct_color = if current_worker_pct >= alert_worker_pct {
                                                egui::Color32::RED
                                            } else {
                                                egui::Color32::from_rgb(40, 167, 69)
                                            };
                                            ui.label(egui::RichText::new(&data.workers_percent).color(pct_color).strong());
                                        });
                                        ui.separator();
                                        ui.allocate_ui_with_layout(egui::vec2(item_width, 50.0), egui::Layout::top_down(egui::Align::Center), |ui| {
                                            ui.label(egui::RichText::new("Conns").strong());
                                            ui.add_space(3.0);
                                            draw_badge(ui, &data.conn_logical.to_string(), egui::Color32::from_rgb(66, 133, 244), egui::Color32::WHITE);
                                        });
                                        ui.separator();
                                        ui.allocate_ui_with_layout(egui::vec2(item_width, 50.0), egui::Layout::top_down(egui::Align::Center), |ui| {
                                            ui.label(egui::RichText::new("PLE").strong());
                                            ui.add_space(3.0);
                                            draw_badge(ui, &data.page_life_expectancy.to_string(), egui::Color32::from_rgb(52, 168, 83), egui::Color32::WHITE);
                                        });
                                        ui.separator();
                                        ui.allocate_ui_with_layout(egui::vec2(item_width, 50.0), egui::Layout::top_down(egui::Align::Center), |ui| {
                                            ui.label(egui::RichText::new("Temp Tbls").strong());
                                            ui.add_space(3.0);
                                            ui.strong(data.active_temp_tables.to_string());
                                        });
                                        ui.separator();
                                        ui.allocate_ui_with_layout(egui::vec2(item_width, 50.0), egui::Layout::top_down(egui::Align::Center), |ui| {
                                            ui.label(egui::RichText::new("Trans").strong());
                                            ui.add_space(3.0);
                                            ui.strong(data.transactions.to_string());
                                        });
                                    });

                                    ui.separator();

                                    ui.horizontal(|ui| {
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            ui.label(
                                                egui::RichText::new(format!("Request Sessions: {}", data.raw_session_count))
                                                    .strong()
                                                    .color(egui::Color32::from_rgb(22, 101, 192))
                                            );

                                            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true), |ui| {
                                                ui.spacing_mut().item_spacing.x = 10.0;
                                                
                                                // 1. 啟動 / 暫停按鈕
                                                let is_paused = ctrl.is_paused.load(Ordering::Relaxed);
                                                let btn_text = if is_paused { "▶ 啟動" } else { "⏸ 暫停" };
                                                if ui.button(btn_text).clicked() {
                                                    ctrl.is_paused.store(!is_paused, Ordering::Relaxed);
                                                }
                                                
                                                // 2. Interval (獨立的綠色風格)
                                                ui.scope(|ui| {
                                                    ui.visuals_mut().widgets.inactive.bg_fill = egui::Color32::from_rgb(209, 231, 221);
                                                    ui.visuals_mut().widgets.hovered.bg_fill = egui::Color32::from_rgb(163, 207, 187);
                                                    ui.visuals_mut().widgets.active.bg_fill = egui::Color32::from_rgb(117, 183, 152);
                                                    ui.visuals_mut().widgets.inactive.fg_stroke.color = egui::Color32::from_rgb(15, 81, 50);
                                                    ui.visuals_mut().widgets.hovered.fg_stroke.color = egui::Color32::from_rgb(15, 81, 50);

                                                    let mut interval_val = ctrl.interval_sec.load(Ordering::Relaxed);
                                                    ui.add(egui::DragValue::new(&mut interval_val).speed(1).suffix(" s").prefix("Interval: "));
                                                    ctrl.interval_sec.store(interval_val, Ordering::Relaxed);
                                                });

                                                // 3. Filter
                                                let mut filter_val = ctrl.filter_elapsed_ms.load(Ordering::Relaxed);
                                                ui.add(egui::DragValue::new(&mut filter_val).speed(100).suffix(" ms").prefix("Filter > "));
                                                ctrl.filter_elapsed_ms.store(filter_val, Ordering::Relaxed);
                                                
                                                // 4. Alert
                                                let mut alert_val = ctrl.alert_elapsed_ms.load(Ordering::Relaxed);
                                                ui.add(egui::DragValue::new(&mut alert_val).speed(100).suffix(" ms").prefix("Alert > "));
                                                ctrl.alert_elapsed_ms.store(alert_val, Ordering::Relaxed);

                                                // 5. Worker Alert
                                                let mut worker_alert_val = ctrl.alert_worker_percent.load(Ordering::Relaxed);
                                                ui.add(egui::DragValue::new(&mut worker_alert_val).speed(1).clamp_range(1..=100).suffix(" %").prefix("Worker Alert > "));
                                                ctrl.alert_worker_percent.store(worker_alert_val, Ordering::Relaxed);
                                            });
                                        });
                                    });

                                    ui.separator();

                                    let header_height = 28.0;
                                    let row_height = 28.0; 
                                    let visible_rows = 11.0; 
                                    let body_height = visible_rows * row_height;
                                    let total_table_height = header_height + body_height + 2.0;
                                    let table_width = col_width - 15.0;

                                    let (rect, _resp) = ui.allocate_exact_size(egui::Vec2::new(table_width, total_table_height), egui::Sense::hover());
                                    let mut child_ui = ui.child_ui(rect, *ui.layout());
                                    child_ui.set_clip_rect(rect);

                                    TableBuilder::new(&mut child_ui)
                                        .striped(true)
                                        .resizable(true)
                                        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                                        .column(Column::initial(55.0).at_least(55.0))  
                                        .column(Column::initial(40.0).at_least(40.0))  
                                        .column(Column::initial(60.0).at_least(60.0))  
                                        .column(Column::initial(80.0).at_least(80.0))  
                                        .column(Column::initial(180.0).at_least(100.0)) 
                                        .column(Column::remainder().at_least(100.0))   
                                        .min_scrolled_height(body_height) 
                                        .max_scroll_height(body_height)
                                        .header(header_height, |mut header| {
                                            header.col(|ui| { ui.strong("Action"); });
                                            header.col(|ui| { ui.strong("SPID"); });
                                            header.col(|ui| { ui.strong("Elapsed"); });
                                            header.col(|ui| { ui.strong("Status"); });
                                            header.col(|ui| { ui.strong("WaitType"); });
                                            header.col(|ui| { ui.strong("Executing SQL"); });
                                        })
                                        .body(|mut body| {
                                            for session in &data.sessions {
                                                body.row(row_height, |mut row| {
                                                    row.col(|ui| {
                                                        ui.style_mut().visuals.widgets.inactive.weak_bg_fill = egui::Color32::from_rgb(226, 232, 240); 
                                                        if ui.add_sized([45.0, 20.0], egui::Button::new(egui::RichText::new("詳細").color(egui::Color32::from_rgb(30, 64, 175)))).clicked() {
                                                            self.selected_session = Some(session.clone());
                                                        }
                                                    });
                                                    row.col(|ui| { ui.label(session.session_id.to_string()); });
                                                    row.col(|ui| { ui.label(session.elapsed_time_ms.to_string()); });
                                                    row.col(|ui| { ui.label(&session.status); });
                                                    
                                                    row.col(|ui| { 
                                                        if session.wait_type.is_empty() {
                                                            ui.label("-");
                                                        } else {
                                                            draw_badge(ui, &session.wait_type, egui::Color32::from_rgb(241, 245, 249), egui::Color32::from_rgb(51, 65, 85));
                                                        }
                                                    });
                                                    row.col(|ui| {
                                                        let clean_sql = session.executing_sql.replace('\n', " ").replace('\r', "");
                                                        ui.add(egui::Label::new(clean_sql).truncate(true));
                                                    });
                                                });
                                            }
                                        });
                                });
                            });

                        if (i + 1) % num_columns == 0 {
                            ui.end_row();
                        }
                    }
                });
        });

        if let Some(session) = &self.selected_session {
            let mut is_open = true;
            egui::Window::new(format!("Session Detail - SPID: {}", session.session_id))
                .open(&mut is_open)
                .default_size([800.0, 600.0])
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        if ui.button("📋 複製所有內容").clicked() {
                            let copy_text = format!(
                                "Capture Time: {}\nSPID: {}\nElapsed Time: {} ms\nStatus: {}\nWait Type: {} ({} ms)\nLast Wait Type: {}\nWait Resource: {}\nWait Session ID (Blocking): {}\nCPU Time: {} ms\nLogical Reads: {}\nPhysical Reads: {}\nWrites: {}\nRow Count: {}\nOpen Trans: {}\nDOP: {}\nClient Address: {}\nLogin Name: {}\nDB Name: {}\nCommand: {}\nStart Time: {}\n\n--- Executing SQL ---\n{}\n\n--- Parent Batch SQL ---\n{}",
                                session.capture_time, session.session_id, session.elapsed_time_ms, session.status, session.wait_type, session.wait_time_ms,
                                session.last_wait_type, session.wait_resource, session.wait_session_id, session.cpu_time_ms,
                                session.logical_reads, session.physical_reads, session.writes, session.row_count,
                                session.open_transaction_count, session.dop, session.client_net_address, session.login_name,
                                session.database_name, session.command, session.start_time,
                                session.executing_sql.trim(), session.parent_batch_sql.trim()
                            );
                            ui.output_mut(|o| o.copied_text = copy_text);
                        }
                    });
                    
                    ui.separator();

                    egui::ScrollArea::vertical().show(ui, |ui| {
                        egui::Grid::new("detail_grid").num_columns(2).striped(true).show(ui, |ui| {
                            ui.label("Capture Time:"); ui.label(&session.capture_time); ui.end_row();
                            ui.label("Elapsed Time (ms):"); ui.label(session.elapsed_time_ms.to_string()); ui.end_row();
                            ui.label("Status:"); ui.label(&session.status); ui.end_row();
                            ui.label("Wait Type:"); ui.label(&session.wait_type); ui.end_row();
                            ui.label("Wait Time (ms):"); ui.label(session.wait_time_ms.to_string()); ui.end_row();
                            ui.label("Last Wait Type:"); ui.label(&session.last_wait_type); ui.end_row();
                            ui.label("Wait Resource:"); ui.label(&session.wait_resource); ui.end_row();
                            ui.label("Wait Session ID (Blocking):"); ui.label(session.wait_session_id.to_string()); ui.end_row();
                            ui.label("CPU Time (ms):"); ui.label(session.cpu_time_ms.to_string()); ui.end_row();
                            ui.label("Logical Reads:"); ui.label(session.logical_reads.to_string()); ui.end_row();
                            ui.label("Physical Reads:"); ui.label(session.physical_reads.to_string()); ui.end_row();
                            ui.label("Writes:"); ui.label(session.writes.to_string()); ui.end_row();
                            ui.label("Row Count:"); ui.label(session.row_count.to_string()); ui.end_row();
                            ui.label("Open Trans:"); ui.label(session.open_transaction_count.to_string()); ui.end_row();
                            ui.label("DOP:"); ui.label(session.dop.to_string()); ui.end_row();
                            ui.label("Client Net Address:"); ui.label(&session.client_net_address); ui.end_row();
                            ui.label("Login Name:"); ui.label(&session.login_name); ui.end_row();
                            ui.label("Database Name:"); ui.label(&session.database_name); ui.end_row();
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