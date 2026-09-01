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
fn default_requests_snapshot() -> u64 { 0 } 
fn default_pause_snapshot() -> u64 { 0 } 

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
    #[serde(default = "default_requests_snapshot")]
    pub requests_snapshot: u64,
    #[serde(default = "default_pause_snapshot")] 
    pub pause_snapshot: u64,
    pub default_interval_sec: u64,
}

#[derive(Clone)]
pub struct ServerController {
    pub is_paused: Arc<AtomicBool>,
    pub interval_sec: Arc<AtomicU64>,
    pub filter_elapsed_ms: Arc<AtomicU64>,
    pub alert_elapsed_ms: Arc<AtomicU64>,
    pub alert_worker_percent: Arc<AtomicU64>, 
    pub requests_snapshot: Arc<AtomicU64>, 
    pub pause_snapshot: Arc<AtomicBool>, 
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
    unacknowledged_alert: bool,
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
                requests_snapshot: Arc::new(AtomicU64::new(srv_cfg.requests_snapshot)),
                pause_snapshot: Arc::new(AtomicBool::new(srv_cfg.pause_snapshot != 0)),
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
            unacknowledged_alert: false,
        }
    }
}

impl eframe::App for MonitorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(std::time::Duration::from_millis(500));

        let mut any_alert_this_tick = false;
        
        for (i, rx) in self.receivers.iter_mut().enumerate() {
            while let Ok(data) = rx.try_recv() {
                let snap_threshold = self.controllers[i].requests_snapshot.load(Ordering::Relaxed);
                let is_snap_paused = self.controllers[i].pause_snapshot.load(Ordering::Relaxed);
                
                if snap_threshold > 0 && data.raw_session_count as u64 >= snap_threshold {
                    if !is_snap_paused {
                        write_snapshot_log(&self.server_configs[i].id, &data);
                    }
                }

                self.server_data[i] = data;
            }
        }

        for data in &self.server_data {
            if data.is_alerting {
                any_alert_this_tick = true;
                break;
            }
        }

        let is_focused = ctx.input(|i| i.focused);

        if is_focused {
            self.unacknowledged_alert = false;
        } else if any_alert_this_tick && !self.unacknowledged_alert {
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(egui::UserAttentionType::Critical));
            self.unacknowledged_alert = true;
        } else if !any_alert_this_tick {
            self.unacknowledged_alert = false;
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
                                        ui.heading(egui::RichText::new(format!("🖥 {}", config.id)).color(egui::Color32::from_rgb(33, 37, 41)));
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            if ui.button("📸 快照").clicked() {
                                                write_snapshot_log(&config.id, data);
                                            }
                                            ui.label(egui::RichText::new(format!("🕒 {}", data.capture_time)).color(egui::Color32::GRAY));
                                        });
                                    });
                                    
                                    ui.separator();
                                    
                                    // 【最聰明的作法：使用 Scope 關閉自動間距，我們自己精準控制一切寬度】
                                    ui.scope(|ui| {
                                        // 關閉 egui 的自動水平間距
                                        ui.spacing_mut().item_spacing.x = 0.0;

                                        ui.horizontal(|ui| {
                                            let total_avail = ui.available_width();
                                            // 5 條分隔線，每條固定佔用 12.0px (含左右空間)
                                            let sep_width = 12.0; 
                                            // 剩下的寬度全部拿來依比例分配，保證 1px 都不會溢出
                                            let usable_width = (total_avail - (5.0 * sep_width)).max(0.0);
                                            
                                            let w_workers = (usable_width * 0.20).floor(); 
                                            let w_conns   = (usable_width * 0.15).floor(); 
                                            let w_ple     = (usable_width * 0.15).floor(); 
                                            let w_temp    = (usable_width * 0.15).floor(); 
                                            let w_trans   = (usable_width * 0.15).floor(); 
                                            let w_req     = usable_width - w_workers - w_conns - w_ple - w_temp - w_trans; 

                                            // 輔助閉包：用來畫出精準佔用 12px 的分隔線
                                            let draw_sep = |ui: &mut egui::Ui| {
                                                // 【修改】加入 egui::Direction::TopDown 解決編譯錯誤
                                                ui.allocate_ui_with_layout(egui::vec2(sep_width, 50.0), egui::Layout::centered_and_justified(egui::Direction::LeftToRight), |ui| {
                                                    ui.separator();
                                                });
                                            };
                                            
                                            // 1. Workers
                                            ui.allocate_ui_with_layout(egui::vec2(w_workers, 50.0), egui::Layout::top_down(egui::Align::Center), |ui| {
                                                ui.add(egui::Label::new(egui::RichText::new("Workers").strong()).wrap(false).truncate(true));
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
                                            
                                            draw_sep(ui);
                                            
                                            // 2. Conns
                                            ui.allocate_ui_with_layout(egui::vec2(w_conns, 50.0), egui::Layout::top_down(egui::Align::Center), |ui| {
                                                ui.add(egui::Label::new(egui::RichText::new("Conns").strong()).wrap(false).truncate(true));
                                                ui.add_space(3.0);
                                                draw_badge(ui, &data.conn_logical.to_string(), egui::Color32::from_rgb(66, 133, 244), egui::Color32::WHITE);
                                            });
                                            
                                            draw_sep(ui);
                                            
                                            // 3. PLE
                                            ui.allocate_ui_with_layout(egui::vec2(w_ple, 50.0), egui::Layout::top_down(egui::Align::Center), |ui| {
                                                ui.add(egui::Label::new(egui::RichText::new("PLE").strong()).wrap(false).truncate(true));
                                                ui.add_space(3.0);
                                                draw_badge(ui, &data.page_life_expectancy.to_string(), egui::Color32::from_rgb(52, 168, 83), egui::Color32::WHITE);
                                            });
                                            
                                            draw_sep(ui);
                                            
                                            // 4. Temp Tbls
                                            ui.allocate_ui_with_layout(egui::vec2(w_temp, 50.0), egui::Layout::top_down(egui::Align::Center), |ui| {
                                                ui.add(egui::Label::new(egui::RichText::new("Temp Tbls").strong()).wrap(false).truncate(true));
                                                ui.add_space(3.0);
                                                ui.strong(data.active_temp_tables.to_string());
                                            });
                                            
                                            draw_sep(ui);
                                            
                                            // 5. Trans
                                            ui.allocate_ui_with_layout(egui::vec2(w_trans, 50.0), egui::Layout::top_down(egui::Align::Center), |ui| {
                                                ui.add(egui::Label::new(egui::RichText::new("Trans").strong()).wrap(false).truncate(true));
                                                ui.add_space(3.0);
                                                ui.strong(data.transactions.to_string());
                                            });
                                            
                                            draw_sep(ui);
                                            
                                            // 6. Req Sessions
                                            ui.allocate_ui_with_layout(egui::vec2(w_req, 50.0), egui::Layout::top_down(egui::Align::Center), |ui| {
                                                ui.add(egui::Label::new(egui::RichText::new("Req. Sessions").strong().color(egui::Color32::from_rgb(22, 101, 192))).wrap(false).truncate(true));
                                                ui.add_space(3.0);
                                                ui.strong(data.raw_session_count.to_string());
                                            });
                                        });
                                    });

                                    ui.separator();

                                    ui.horizontal_wrapped(|ui| {
                                        ui.spacing_mut().item_spacing.x = 10.0;
                                        ui.spacing_mut().item_spacing.y = 8.0; 
                                        
                                        let is_paused = ctrl.is_paused.load(Ordering::Relaxed);
                                        let btn_text = if is_paused { 
                                            egui::RichText::new("▶ 啟動監控").color(egui::Color32::WHITE).strong()
                                        } else { 
                                            egui::RichText::new("⏸ 暫停監控") 
                                        };
                                        
                                        let mut btn = egui::Button::new(btn_text);
                                        if is_paused {
                                            btn = btn.fill(egui::Color32::from_rgb(220, 53, 69)); 
                                        }

                                        if ui.add(btn).clicked() {
                                            ctrl.is_paused.store(!is_paused, Ordering::Relaxed);
                                        }
                                        
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

                                        let mut filter_val = ctrl.filter_elapsed_ms.load(Ordering::Relaxed);
                                        ui.add(egui::DragValue::new(&mut filter_val).speed(100).suffix(" ms").prefix("Filter > "));
                                        ctrl.filter_elapsed_ms.store(filter_val, Ordering::Relaxed);
                                        
                                        let mut alert_val = ctrl.alert_elapsed_ms.load(Ordering::Relaxed);
                                        ui.add(egui::DragValue::new(&mut alert_val).speed(100).suffix(" ms").prefix("Alert > "));
                                        ctrl.alert_elapsed_ms.store(alert_val, Ordering::Relaxed);

                                        let mut worker_alert_val = ctrl.alert_worker_percent.load(Ordering::Relaxed);
                                        ui.add(egui::DragValue::new(&mut worker_alert_val).speed(1).clamp_range(1..=100).suffix(" %").prefix("Worker Alert > "));
                                        ctrl.alert_worker_percent.store(worker_alert_val, Ordering::Relaxed);

                                        let mut snap_val = ctrl.requests_snapshot.load(Ordering::Relaxed);
                                        ui.add(egui::DragValue::new(&mut snap_val).speed(1).prefix("Req. Snap > "));
                                        ctrl.requests_snapshot.store(snap_val, Ordering::Relaxed);

                                        let mut is_snap_paused = ctrl.pause_snapshot.load(Ordering::Relaxed);
                                        if ui.toggle_value(&mut is_snap_paused, "⏸ 暫停快照").changed() {
                                            ctrl.pause_snapshot.store(is_snap_paused, Ordering::Relaxed);
                                        }
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
                                        .column(Column::initial(45.0).at_least(45.0))  
                                        .column(Column::initial(40.0).at_least(35.0))  
                                        .column(Column::initial(50.0).at_least(45.0))  
                                        .column(Column::initial(60.0).at_least(50.0))  
                                        .column(Column::initial(100.0).at_least(80.0)) 
                                        .column(Column::remainder().at_least(60.0))   
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