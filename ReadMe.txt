此監控系統為運作於Windows上的desktop程式，可以查看SQL Server的:
    session執行狀況
    worker thread使用量
    連線數(Conns)
    資料頁面停留於記憶體的時間(PLE, page life expectancy)
    暫存table的數量(Temp Tbls)
    開啟的交易數量(Trans)
    當前執行中的Request Session數量


同時最多可以監控4台主機(可重複，四個監控都可以指向同一台)

設定檔請編輯config.toml，在設定檔中可以設定:
    被監控主機名稱(自訂義)
    ip / fqdn
    port
    帳號密碼
    設定顯示的session須執行超過多少秒才顯示(filter_elapsed_ms)
    設定執行時間超過多久會跳出提醒並且寫入alert log。(alert_elapsed_ms)
    設定資料更新頻率(default_interval_sec)
    設定worker使用量大於多少百分比會告警(alert_worker_percent)
    設定自動記錄snapshot的session數量閥值 (snapshot_sessions)
    設定該機器預設是否會自動記錄snapshot (pause_snapshot)

上述設定除了寫在config檔中，也可於程式執行期間於介面上動態修改。

可修改參數或功能:
    暫停監控      --  停止對目標主機發送監控查詢，同時也不進行log紀錄。
    Interval      --  設定每次查詢間隔，單位為"秒"
    Filter        --  設定要顯示在下方的session需要執行時間超過多少ms才進行顯示(不影響上方的Req. Sessions數量統計)
    Alert         --  當Session執行時間超過多少秒會被自動記錄到alert log。
    Worker Alert  --  設定active worker數量超過多少最大數量的百分比時，記錄一筆近alert log。
    Req. Snap     --  設定當Req. Sessions的數量超過多少條時，自動寫入snapshot log。
    暫停快照      --  監控持續，但當Req. Sessions大於Req. Snap設定的閥值時，不會寫Snapshot Log。



每台主機顯示執行時間最長的前10條session，對於每條session可以點"詳細"，去看更多的細節。


執行過程中，會自動記錄log，log有兩種，分別為
    alert log  --- 執行時間超過該主機設定值(Alert)的session
    snapshot log  --- 當該主機當下同時執行的request sessions > 設定值(Req. Snap)。可藉由點選"暫停快照"停止自動記錄

Snapshot log除了會自動記錄外，也可手動點擊"快照"進行記錄。

Log會產生在與監控程式相同目錄下，名稱為:alert_log_YYYYMMDD.log以及snapshot_log_YYYYMMDD.log。

此兩log可以分別使用AlertLogViewer.exe/SnapshotViewer.exe進行讀取，開啟後將log拖曳到程式中即可。




--------
程式架構
--------
MonitorSQL/
├── Cargo.toml               # 1. 專案依賴套件設定
├── config.toml              # 2. 您的應用程式外部設定檔
├── .cargo/
│   └── config.toml          # 3. Rust 編譯設定 (用來達成 Portable)
└── src/
    ├── main.rs              # 4. 程式入口與 GUI 啟動
    ├── app.rs               # 5. UI 畫面與背景監控任務邏輯
    └── db.rs                # 6. SQL Server 連線與 T-SQL 查詢處理


AlertLogViewer/
├── Cargo.toml               # 1. 專案依賴套件設定
└── src/
    └── main.rs              # 2. 程式入口與 GUI 啟動

--------
編譯方式
--------
在powershell中，切換目錄到MonitorSQL底下執行:
    cargo build --release

執行完畢會產出可執行檔，置於:
    MonitorSQL\target\release\

可直接取出MonitorSQL.exe以及將config.toml放在同一目錄下即可執行。



--------
編譯錯誤對應
--------

==========================================================
PS D:\MonitorSQL> cargo build --release

    Updating crates.io index

warning: spurious network error (3 tries remaining): [35] SSL connect error (schannel: next InitializeSecurityContext failed: CRYPT_E_NO_REVOCATION_CHECK (0x80092012) - 撤銷功能無法檢查憑證的撤銷。)


這個錯誤 (CRYPT_E_NO_REVOCATION_CHECK) 在 Windows 環境下非常常見。這通常發生在公司內部網路、使用代理伺服器 (Proxy) 或是防火牆阻擋了連線，導致 Windows 內建的 SSL 機制 (Schannel) 無法連線到憑證機構去驗證「憑證是否被撤銷」。

method 1:
    $env:CARGO_HTTP_CHECK_REVOKE="false"
    cargo build --release


method 2:(一勞永逸的方法)
    修改.cargo/config.toml，加入: 
  $env:CARGO_HTTP_CHECK_REVOKE="false"
  cargo build --release

=========================================================


   Compiling khronos_api v3.1.0

   Compiling icu_normalizer_data v2.2.0

error: linker `link.exe` not found

  |

  = note: program not found



note: the msvc targets depend on the msvc linker but `link.exe` was not found



note: please ensure that Visual Studio 2017 or later, or Build Tools for Visual Studio were installed with the Visual C++ option



這個錯誤表示您的系統目前缺少微軟的 C++ 編譯工具 (C++ Build Tools)。只需要安裝輕量級的「建置工具」即可。請按照以下步驟解決：

前往微軟官方下載頁面：
請到瀏覽器搜尋「Visual Studio C++ Build Tools」或直接點擊此連結：
https://visualstudio.microsoft.com/visual-cpp-build-tools/

下載安裝程式：
點擊頁面上的「下載 Build Tools (Download Build Tools)」按鈕。

執行安裝並勾選必要元件：
打開下載好的安裝程式 (Visual Studio Installer)，在工作負載 (Workloads) 的選單中，請務必勾選左上角的：
👉 「使用 C++ 的桌面開發」 (Desktop development with C++)

點擊安裝：
右下角會顯示需要下載幾 GB 的空間，點擊「安裝」並等待完成。

重啟終端機 (非常重要)：
安裝完成後，請務必關閉您目前的 PowerShell 視窗，然後重新開一個新的 PowerShell 視窗（這樣系統才會載入最新的環境變數 link.exe）。

