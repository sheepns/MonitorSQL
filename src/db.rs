use chrono::Local;
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::atomic::Ordering;
use tiberius::{AuthMethod, Client, Config};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration};
use tokio_util::compat::TokioAsyncWriteCompatExt;

use crate::app::{DashboardData, ServerConfig, ServerController, SessionData};

const SQL_SESSION: &str = r#"
SELECT 
    r.session_id, r.total_elapsed_time, r.status, r.blocking_session_id, r.wait_type, 
    r.wait_time, r.last_wait_type, r.wait_resource, 
    SUBSTRING(t.[text], (r.statement_start_offset / 2) + 1, 
      ((CASE r.statement_end_offset WHEN -1 THEN DATALENGTH(t.[text]) 
        ELSE r.statement_end_offset END - r.statement_start_offset) / 2) + 1) AS executing_sql, 
    t.[text] AS parent_batch_sql, r.cpu_time, r.logical_reads, r.reads, r.writes, 
    r.row_count, r.open_transaction_count, r.dop, c.client_net_address, 
    s.login_name, DB_NAME(r.database_id) AS database_name, r.command, r.start_time, 
    getdate() as CaptureTime
FROM sys.dm_exec_requests AS r
INNER JOIN sys.dm_exec_sessions AS s ON r.session_id = s.session_id
-- 修正這裡：加入 c.connection_id = r.connection_id 排除多重連線造成的重複資料
INNER JOIN sys.dm_exec_connections c ON c.session_id = r.session_id AND c.connection_id = r.connection_id
CROSS APPLY sys.dm_exec_sql_text(r.sql_handle) AS t
WHERE r.session_id <> @@SPID 
AND t.[text] <> 'sp_server_diagnostics'
ORDER BY r.total_elapsed_time desc;
"#;

const SQL_WORKER: &str = r#"
select getdate() as CaptureTime,
(select max_workers_count from sys.dm_os_sys_info) as MaxThreads,
sum(Active_workers_count) As ActiveWorkers,
FORMAT(ROUND((sum(Active_workers_count) * 1.0 / (select max_workers_count from sys.dm_os_sys_info)) ,5),'P2') as WorkersPercent
from sys.dm_os_schedulers 
where status='VISIBLE ONLINE'
"#;

const SQL_COUNTERS: &str = r#"
select counter_name, cntr_value
from sys.dm_os_performance_counters
where cntr_type =65792
and RTRIM(counter_name) in ('Logical connections','Page life expectancy','Transactions','Active Temp Tables')
and object_name in ('SQLServer:Buffer Manager','SQLServer:General Statistics')
"#;

pub async fn monitor_server(
    config: ServerConfig,
    controller: ServerController,
    tx: mpsc::Sender<DashboardData>,
) {
    let mut tiberius_config = Config::new();
    tiberius_config.host(&config.host);
    tiberius_config.port(config.port);
    tiberius_config.authentication(AuthMethod::sql_server(&config.username, &config.password));
    tiberius_config.trust_cert();

    let mut client: Option<Client<tokio_util::compat::Compat<TcpStream>>> = None;

    loop {
        if controller.is_paused.load(Ordering::Relaxed) {
            sleep(Duration::from_millis(500)).await;
            continue;
        }

        if client.is_none() {
            println!("[{}] 嘗試建立 TCP 連線至 {}:{} ...", config.id, config.host, config.port);
            match TcpStream::connect(format!("{}:{}", config.host, config.port)).await {
                Ok(tcp) => {
                    let _ = tcp.set_nodelay(true);
                    println!("[{}] TCP 連線成功，嘗試 SQL 登入 (帳號: {}) ...", config.id, config.username);
                    match Client::connect(tiberius_config.clone(), tcp.compat_write()).await {
                        Ok(c) => {
                            println!("[{}] ✅ SQL 登入成功！", config.id);
                            client = Some(c);
                        }
                        Err(e) => println!("[{}] ❌ SQL 登入失敗: {:?}", config.id, e),
                    }
                }
                Err(e) => println!("[{}] ❌ TCP 連線失敗: {:?}", config.id, e),
            }
        }

        if let Some(ref mut c) = client {
            let mut dashboard = DashboardData::default();
            let mut connection_valid = true;

            dashboard.capture_time = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

            if let Ok(stream) = c.simple_query(SQL_WORKER).await {
                if let Ok(Some(row)) = stream.into_row().await {
                    dashboard.max_threads = row.try_get::<i32, _>("MaxThreads").unwrap_or(Some(0)).unwrap_or(0) as u32;
                    dashboard.active_workers = row.try_get::<i32, _>("ActiveWorkers").unwrap_or(Some(0)).unwrap_or(0) as u32;
                    dashboard.workers_percent = row.try_get::<&str, _>("WorkersPercent").unwrap_or(Some("0.00%")).unwrap_or("0.00%").to_string();
                    
                    let alert_worker_pct = controller.alert_worker_percent.load(Ordering::Relaxed);
                    let current_worker_pct = if dashboard.max_threads > 0 {
                        ((dashboard.active_workers as f64 / dashboard.max_threads as f64) * 100.0).round() as u64
                    } else { 0 };

                    if current_worker_pct >= alert_worker_pct {
                        dashboard.is_alerting = true;
                        write_worker_alert_log(&config.id, dashboard.active_workers, dashboard.max_threads, current_worker_pct);
                    }
                }
            } else { println!("[{}] ❌ Worker 查詢錯誤", config.id); connection_valid = false; }

            if connection_valid {
                if let Ok(stream) = c.simple_query(SQL_COUNTERS).await {
                    if let Ok(rows) = stream.into_first_result().await {
                        for row in rows {
                            let name: &str = row.try_get("counter_name").unwrap_or(Some("")).unwrap_or("").trim();
                            let val: i64 = row.try_get("cntr_value").unwrap_or(Some(0)).unwrap_or(0);
                            
                            if name.eq_ignore_ascii_case("Logical connections") {
                                dashboard.conn_logical = val;
                            } else if name.eq_ignore_ascii_case("Page life expectancy") {
                                dashboard.page_life_expectancy = val;
                            } else if name.eq_ignore_ascii_case("Transactions") {
                                dashboard.transactions = val;
                            } else if name.eq_ignore_ascii_case("Active Temp Tables") {
                                dashboard.active_temp_tables = val;
                            }
                        }
                    }
                } else { println!("[{}] ❌ Counters 查詢錯誤", config.id); connection_valid = false; }
            }

            if connection_valid {
                let current_filter = controller.filter_elapsed_ms.load(Ordering::Relaxed) as i32;
                let alert_threshold = controller.alert_elapsed_ms.load(Ordering::Relaxed) as i32;

                if let Ok(stream) = c.simple_query(SQL_SESSION).await {
                    if let Ok(rows) = stream.into_first_result().await {
                        // 【新增】記錄此 SQL 語法實際抓取到的所有列數 (不過濾)
                        dashboard.raw_session_count = rows.len();

                        for row in rows {
                            let elapsed = row.try_get::<i32, _>("total_elapsed_time").unwrap_or(Some(0)).unwrap_or(0);
                            
                            if elapsed >= current_filter {
                                let mut session = SessionData::default();
                                session.session_id = row.try_get("session_id").unwrap_or(Some(0)).unwrap_or(0);
                                session.elapsed_time_ms = elapsed;
                                session.status = row.try_get::<&str, _>("status").unwrap_or(Some("")).unwrap_or("").trim().to_string();
                                session.wait_type = row.try_get::<&str, _>("wait_type").unwrap_or(Some("")).unwrap_or("").trim().to_string();
                                session.wait_time_ms = row.try_get::<i64, _>("wait_time").unwrap_or(Some(0)).unwrap_or(0);
                                session.last_wait_type = row.try_get::<&str, _>("last_wait_type").unwrap_or(Some("")).unwrap_or("").trim().to_string();
                                session.wait_resource = row.try_get::<&str, _>("wait_resource").unwrap_or(Some("")).unwrap_or("").trim().to_string();
                                session.wait_session_id = row.try_get::<i16, _>("blocking_session_id").unwrap_or(Some(0)).unwrap_or(0);
                                session.executing_sql = row.try_get::<&str, _>("executing_sql").unwrap_or(Some("")).unwrap_or("").to_string();
                                session.parent_batch_sql = row.try_get::<&str, _>("parent_batch_sql").unwrap_or(Some("")).unwrap_or("").to_string();
                                session.cpu_time_ms = row.try_get("cpu_time").unwrap_or(Some(0)).unwrap_or(0);
                                session.logical_reads = row.try_get("logical_reads").unwrap_or(Some(0)).unwrap_or(0);
                                session.physical_reads = row.try_get("reads").unwrap_or(Some(0)).unwrap_or(0);
                                session.writes = row.try_get("writes").unwrap_or(Some(0)).unwrap_or(0);
                                session.row_count = row.try_get("row_count").unwrap_or(Some(0)).unwrap_or(0);
                                session.open_transaction_count = row.try_get("open_transaction_count").unwrap_or(Some(0)).unwrap_or(0);
                                session.dop = row.try_get("dop").unwrap_or(Some(0)).unwrap_or(0);
                                session.client_net_address = row.try_get::<&str, _>("client_net_address").unwrap_or(Some("")).unwrap_or("").to_string();
                                session.login_name = row.try_get::<&str, _>("login_name").unwrap_or(Some("")).unwrap_or("").to_string();
                                session.database_name = row.try_get::<&str, _>("database_name").unwrap_or(Some("")).unwrap_or("").to_string();
                                session.command = row.try_get::<&str, _>("command").unwrap_or(Some("")).unwrap_or("").to_string();
                                session.start_time = row.try_get::<chrono::NaiveDateTime, _>("start_time").map(|opt| opt.map_or(String::new(), |dt| dt.to_string())).unwrap_or_default();
                                session.capture_time = dashboard.capture_time.clone();

                                if elapsed >= alert_threshold {
                                    dashboard.is_alerting = true;
                                    write_alert_log(&config.id, &session);
                                }

                                dashboard.sessions.push(session);
                            }
                        }
                    }
                } else { println!("[{}] ❌ Session 查詢錯誤", config.id); connection_valid = false; }
            }

            if !connection_valid {
                client = None;
            } else {
                let _ = tx.send(dashboard).await;
            }
        }

        let wait_sec = controller.interval_sec.load(Ordering::Relaxed);
        sleep(Duration::from_secs(wait_sec)).await;
    }
}

// Session 告警 Log
fn write_alert_log(server_id: &str, session: &SessionData) {
    let now = Local::now();
    let log_time = now.format("%Y-%m-%d %H:%M:%S");
    
    let file_name = now.format("alert_log_%Y%m%d.log").to_string();

    let log_msg = format!(
        "[{}] Server: {} | SPID: {} | Alert! Session Elapsed: {} ms\n\
        \tStatus: {}\n\
        \tWaitType: {} (Time: {} ms)\n\
        \tLastWaitType: {}\n\
        \tWaitResource: {}\n\
        \tWaitSessionID (Blocking): {}\n\
        \tCPU Time: {} ms\n\
        \tLogical Reads: {}\n\
        \tPhysical Reads: {}\n\
        \tWrites: {}\n\
        \tRowCount: {}\n\
        \tOpen Trans: {}\n\
        \tDOP: {}\n\
        \tClient Address: {}\n\
        \tLogin Name: {}\n\
        \tDB Name: {}\n\
        \tCommand: {}\n\
        \tStart Time: {}\n\
        \tExecuting SQL: {}\n\
        \tParent Batch SQL: {}\n\
        ==================================================\n",
        log_time, server_id, session.session_id, session.elapsed_time_ms,
        session.status, session.wait_type, session.wait_time_ms,
        session.last_wait_type, session.wait_resource, session.wait_session_id,
        session.cpu_time_ms, session.logical_reads, session.physical_reads,
        session.writes, session.row_count, session.open_transaction_count,
        session.dop, session.client_net_address, session.login_name,
        session.database_name, session.command, session.start_time,
        session.executing_sql.trim(), session.parent_batch_sql.trim()
    );

    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(file_name) {
        let _ = file.write_all(log_msg.as_bytes());
    }
}

// Worker Threads 告警 Log
fn write_worker_alert_log(server_id: &str, active_workers: u32, max_threads: u32, percent: u64) {
    let now = Local::now();
    let log_time = now.format("%Y-%m-%d %H:%M:%S");
    
    let file_name = now.format("alert_log_%Y%m%d.log").to_string();

    let log_msg = format!(
        "[{}] Server: {} | ALERT! Worker Threads usage is high: {}% ({} / {})\n==================================================\n",
        log_time, server_id, percent, active_workers, max_threads
    );

    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(file_name) {
        let _ = file.write_all(log_msg.as_bytes());
    }
}