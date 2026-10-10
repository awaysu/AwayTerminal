//! 連接埠（COM）——搬移舊版 `Sessions/SerialSession.cs`（119 行，`System.IO.Ports`）。
//!
//! 行為對照表在 `docs/COM.md`。重點結構：
//!
//! - [`SerialLink`]：「一條可以讀寫的線」。真的序列埠是它的一個實作，
//!   `com_probe` 用同程式內的管線當假裝置——**沒有硬體也能驗完整條 session 邏輯**。
//! - [`ComSession`]：讀取執行緒 + **寫入執行緒（有佇列）**。寫入要獨立執行緒的理由照舊版註解：
//!   流量控制被對方擋住（CTS 沒接、XOFF、裝置沒電）時寫入會等滿 `WriteTimeout`（2 秒）才失敗，
//!   在 IPC 執行緒上直接寫＝每個按鍵凍 2 秒、貼上凍好幾秒。
//!
//! | 舊版 | 這裡 |
//! |---|---|
//! | `SerialPort(port, baud, parity, dataBits, stopBits)` + `Handshake` | [`open`] |
//! | `DtrEnable = true` | 同 |
//! | `RtsEnable = true` **只在** flow 是 None／XOnXOff 時設（硬體流控時驅動自己管 RTS） | 同 |
//! | `WriteTimeout = 2000` | 同 |
//! | 讀取執行緒 blocking read（比 `DataReceived` 即時） | 短逾時輪詢，見 [`READ_POLL`] 的說明 |
//! | 寫入佇列 + 專用執行緒，逾時就丟掉那一筆、不卡任何人 | 同 |
//! | 拔線／裝置消失 → 發 `Exited`（自動重連接手） | 同 |
//! | 使用者關閉 → `Dispose` 自己發 `Exited` | 同（[`ComSession::close`]） |
//! | `Resize` 是空的 | 同（序列埠沒有視窗大小的概念） |
//! | `SendReset()` 是 TODO（**舊版沒有定義要送什麼**，目前不送） | 同（保留註解，不自己發明行為） |
//! | `ProcessId => 0` | 同 |
//! | 開埠失敗 → 分頁**移除**＋跳錯誤視窗（`Open()` 是同步的） | 同（`session_create` 回 `Err`，前端跳對話框） |

use crate::i18n::{t, tf};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

// `open_native()` 回的是具體型別，DTR／RTS 那幾個方法要把 trait 帶進來
use serialport::SerialPort;

use crate::session::{ExitInfo, OnExit, OnOutput, TerminalSession};

/// 讀取的輪詢間隔。
///
/// ⚠️ 舊版用 `ReadTimeout = InfiniteTimeout` 的 blocking read（註解說「比 `DataReceived`
/// 事件即時、無延遲」）。我們改成短逾時輪詢，因為 `serialport` 的讀取要靠逾時才回得來——
/// 沒有逾時的話，關分頁時那條執行緒會永遠卡在 `read` 裡（Windows 上關 handle 不保證叫醒它）。
/// 25ms 對終端機回顯感覺不出來，而閒置時一秒 40 次喚醒的成本可以忽略。
const READ_POLL: Duration = Duration::from_millis(25);

/// 寫入逾時（舊版 `WriteTimeout = 2000`）。
const WRITE_TIMEOUT: Duration = Duration::from_millis(2000);

/// 一條 COM 連線的參數。
///
/// 欄位與**字串值**都照舊版 `settings.json`（`ComParity` ＝ `None`／`Odd`／`Even`…，
/// `ComStopBits` ＝ `One`／`Two`／`OnePointFive`，`ComFlow` ＝ `None`／`XOnXOff`／
/// `RequestToSend`／`RequestToSendXOnXOff`），這樣舊設定匯入時不必轉換。
/// ⚠️ 其中有幾個值 `serialport` crate 不支援，見 [`map_parity`]／[`map_stop`]／[`map_flow`]。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ComParams {
    /// 埠名稱（Windows `COM5`；mac/Linux 是 `/dev/...`）。
    pub port: String,
    pub baud: u32,
    pub data_bits: u8,
    pub parity: String,
    pub stop_bits: String,
    pub flow: String,
    /// 斷線（拔線）自動重連。舊版的 COM 對話框也有這個勾選，與 SSH／Telnet 共用設定。
    pub auto_reconnect: bool,
}

impl Default for ComParams {
    fn default() -> Self {
        // 舊版 AppSettings 的預設值：COM5 / 115200 / 8 / None / One / None
        Self {
            port: "COM5".to_string(),
            baud: 115_200,
            data_bits: 8,
            parity: "None".to_string(),
            stop_bits: "One".to_string(),
            flow: "None".to_string(),
            auto_reconnect: false,
        }
    }
}

impl ComParams {
    /// 分頁標題（舊版 `OpenComDirect`：`$"{port} {baud}"`）。
    pub fn title(&self) -> String {
        format!("{} {}", self.port, self.baud)
    }
}

/// 舊版鮑率清單（`ComDialog.Bauds`），順序照抄。
pub const BAUD_RATES: &[u32] = &[9600, 19200, 38400, 57600, 115_200, 230_400, 460_800, 921_600];

/// 舊版資料位元清單。
pub const DATA_BITS: &[u8] = &[5, 6, 7, 8];

// ---------------------------------------------------------------- 參數對映

/// 對映結果：`serialport` 的值 + 「有沒有被降級」的說明（要印在終端機上）。
pub struct Mapped<T> {
    pub value: T,
    /// `Some(訊息)` ＝這個值 crate 不支援，已經退到別的值。
    pub warning: Option<String>,
}

/// 同位檢查。⚠️ `serialport` 只有 None／Odd／Even——舊版的 **Mark／Space 沒有對應**。
pub fn map_parity(name: &str) -> Mapped<serialport::Parity> {
    match name {
        "Odd" => Mapped { value: serialport::Parity::Odd, warning: None },
        "Even" => Mapped { value: serialport::Parity::Even, warning: None },
        "None" => Mapped { value: serialport::Parity::None, warning: None },
        other => Mapped {
            value: serialport::Parity::None,
            warning: Some(tf("com.parityUnsupported", &[other])),
        },
    }
}

/// 停止位元。⚠️ `serialport` 只有 One／Two——舊版的 **1.5（`OnePointFive`）沒有對應**。
pub fn map_stop(name: &str) -> Mapped<serialport::StopBits> {
    match name {
        "Two" => Mapped { value: serialport::StopBits::Two, warning: None },
        "One" => Mapped { value: serialport::StopBits::One, warning: None },
        other => Mapped {
            value: serialport::StopBits::One,
            warning: Some(tf("com.stopBitsUnsupported", &[other])),
        },
    }
}

/// 流量控制。⚠️ `serialport` 只有 None／Software／Hardware——舊版的
/// **`RequestToSendXOnXOff`（硬體＋軟體一起）沒有對應**，退成硬體。
pub fn map_flow(name: &str) -> Mapped<serialport::FlowControl> {
    match name {
        "XOnXOff" => Mapped { value: serialport::FlowControl::Software, warning: None },
        "RequestToSend" => Mapped { value: serialport::FlowControl::Hardware, warning: None },
        "RequestToSendXOnXOff" => Mapped {
            value: serialport::FlowControl::Hardware,
            warning: Some(t("com.flowRtsXonUnsupported").to_string()),
        },
        "None" => Mapped { value: serialport::FlowControl::None, warning: None },
        other => Mapped {
            value: serialport::FlowControl::None,
            warning: Some(tf("com.flowUnknown", &[other])),
        },
    }
}

/// 舊版只在 flow 是 `None`／`XOnXOff` 時把 RTS 拉起來——硬體流控時 RTS 由驅動管，
/// 自己設會出錯（舊版註解：「當 Handshake 為 RTS 類型時，設定 RtsEnable 會丟例外」）。
fn rts_should_be_set(flow: &str) -> bool {
    matches!(flow, "None" | "XOnXOff")
}

// ---------------------------------------------------------------- 埠列舉

/// 一個可選的埠。
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortInfo {
    /// 真正要拿去開的名稱（`COM5`）。
    pub name: String,
    /// 下拉裡顯示的字（`COM5 — USB-SERIAL CH340`）。舊版只顯示 `name`，見 `docs/COM.md`。
    pub label: String,
    /// USB 裝置的描述（沒有就空）。
    pub detail: String,
}

/// 目前看得到的埠。
///
/// 舊版用 `SerialPort.GetPortNames()`（**只有名稱**）。這裡多用 `available_ports()` 的 USB
/// 資訊組一個友善名稱——PM 在 TASK-011 指定的加值，顯示用而已，拿去開的還是 `name`。
pub fn list_ports() -> Vec<PortInfo> {
    let mut out: Vec<PortInfo> = match serialport::available_ports() {
        Ok(ports) => ports
            .into_iter()
            // 平台特有的過濾（`CLAUDE.md` 平台差異表）：
            //   mac  只留 `cu.*`／`tty.*`，並且把藍牙那個節點濾掉
            //   Linux 只留 `ttyUSB*`／`ttyACM*`／`ttyS*`／`ttyAMA*`
            //   Windows `COM<n>`
            // `serialport` 在 mac 會把 `/dev/tty.Bluetooth-Incoming-Port` 也列出來，
            // 那不是使用者要的東西。
            .filter(|p| awayterm_platform::serial::looks_like_serial(&p.port_name))
            .map(describe)
            .collect(),
        Err(e) => {
            println!("[AwayTerminal] 列舉連接埠失敗：{e}");
            Vec::new()
        }
    };
    // 舊版是 `OrderBy(p => p)`；`COM10` 要排在 `COM9` 後面，所以照數字排
    out.sort_by_key(|p| natural_key(&p.name));
    out
}

fn describe(p: serialport::SerialPortInfo) -> PortInfo {
    let name = p.port_name.clone();
    let detail = match &p.port_type {
        serialport::SerialPortType::UsbPort(u) => {
            let mut parts = Vec::new();
            if let Some(prod) = u.product.as_ref().filter(|s| !s.trim().is_empty()) {
                parts.push(prod.trim().to_string());
            }
            if let Some(man) = u.manufacturer.as_ref().filter(|s| !s.trim().is_empty()) {
                if !parts.iter().any(|p| p.contains(man.trim())) {
                    parts.push(man.trim().to_string());
                }
            }
            parts.push(format!("VID:PID {:04X}:{:04X}", u.vid, u.pid));
            parts.join(" · ")
        }
        serialport::SerialPortType::BluetoothPort => "Bluetooth".to_string(),
        serialport::SerialPortType::PciPort => "PCI".to_string(),
        serialport::SerialPortType::Unknown => String::new(),
    };
    let label = if detail.is_empty() {
        name.clone()
    } else {
        format!("{name} — {detail}")
    };
    PortInfo { name, label, detail }
}

/// `COM9` < `COM10`：把數字部分當數字比。
fn natural_key(name: &str) -> (String, u64) {
    let digits: String = name.chars().rev().take_while(|c| c.is_ascii_digit()).collect();
    let n = digits.chars().rev().collect::<String>().parse().unwrap_or(0);
    let prefix = name[..name.len() - digits.len()].to_string();
    (prefix, n)
}

/// 前端要列出可選的埠時呼叫。也回鮑率／資料位元清單，這樣對話框不用自己寫死。
#[tauri::command]
pub fn com_ports() -> ComCatalog {
    let ports = list_ports();
    println!(
        "[AwayTerminal] 連接埠列舉：{} 個{}",
        ports.len(),
        if ports.is_empty() {
            "（這台機器目前沒有序列埠）"
        } else {
            ""
        }
    );
    ComCatalog {
        ports,
        bauds: BAUD_RATES.to_vec(),
        data_bits: DATA_BITS.to_vec(),
        // ⚠️ 只列 `serialport` 真的支援的值。舊版還有 Mark／Space 同位與 1.5 停止位元、
        // 以及 RTS/CTS+XON/XOFF——這個函式庫沒有，列出來會騙人（見 docs/COM.md 的限制）。
        parities: vec!["None".into(), "Odd".into(), "Even".into()],
        stop_bits: vec!["One".into(), "Two".into()],
        flows: vec![
            "None".into(),
            "XOnXOff".into(),
            "RequestToSend".into(),
        ],
    }
}

/// 連接埠對話框按「開啟」時把六個欄位存回設定（舊版 `ComDialog` 按確定就寫 `AppSettings.Com*`），
/// 下次開對話框才會是「上次用的值」。
///
/// ⚠️ 2.0.10 以前漏了這一步：對話框永遠帶入從 1.x 匯入的值（例：`RequestToSendXOnXOff` 降成
/// `RequestToSend`），使用者每次都要手動改回 None 才開得起來。
/// 自動重連（`autoReconnect`）是 SSH／Telnet 共用的設定，這裡不動。
#[tauri::command]
pub fn com_remember(params: ComParams, settings: tauri::State<'_, Arc<crate::settings::SettingsStore>>) {
    let port = params.port.trim();
    if port.is_empty() {
        return;
    }
    settings.update(|s| {
        s.com_port = port.to_string();
        s.com_baud = params.baud;
        s.com_data_bits = params.data_bits;
        s.com_parity = params.parity.clone();
        s.com_stop_bits = params.stop_bits.clone();
        s.com_flow = params.flow.clone();
    });
}

/// `com_ports` 的回覆。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComCatalog {
    pub ports: Vec<PortInfo>,
    pub bauds: Vec<u32>,
    pub data_bits: Vec<u8>,
    pub parities: Vec<String>,
    pub stop_bits: Vec<String>,
    pub flows: Vec<String>,
}

// ---------------------------------------------------------------- 「一條線」

/// 「一條線」就是一組 std 的 [`Read`] + [`Write`]。
///
/// 真序列埠是 `serialport` 的兩個 handle；`com_probe` 用一對管線。抽這層的理由：
/// 序列埠的 session 邏輯（讀取執行緒、寫入佇列、結束事件只發一次、「連上了」只回報一次）
/// **沒有硬體也該驗得到**。讀寫分成兩個物件，兩條執行緒就不必共用鎖
/// （共用的話讀取那邊持鎖等逾時，寫入就會被它擋住）。
pub struct Link {
    pub reader: Box<dyn Read + Send>,
    pub writer: Box<dyn Write + Send>,
    /// 診斷用名稱（埠名／`fake`）。
    pub name: String,
}

/// 開一個真的序列埠。回傳 (線, 要印在終端機上的警告)。
///
/// 開不起來就回 `Err`——**同步失敗**，和舊版 `SerialPort.Open()` 一樣（呼叫端會把分頁收掉）。
pub fn open(params: &ComParams) -> Result<(Link, Vec<String>), String> {
    let mut warnings = Vec::new();
    // mac：`tty.xxx` 開的時候會等 DCD（可能永遠卡住），主動連出去要用 `cu.xxx`。
    // 其他平台原樣（見 `awayterm_platform::serial` 的說明）。
    let port_path = awayterm_platform::serial::prefer_call_unit(&params.port);
    let parity = map_parity(&params.parity);
    let stop = map_stop(&params.stop_bits);
    let flow = map_flow(&params.flow);
    for w in [parity.warning, stop.warning, flow.warning].into_iter().flatten() {
        warnings.push(w);
    }
    let data_bits = match params.data_bits {
        5 => serialport::DataBits::Five,
        6 => serialport::DataBits::Six,
        7 => serialport::DataBits::Seven,
        8 => serialport::DataBits::Eight,
        other => {
            warnings.push(tf("com.dataBitsUnsupported", &[&other.to_string()]));
            serialport::DataBits::Eight
        }
    };

    let mut port = serialport::new(&port_path, params.baud)
        .data_bits(data_bits)
        .parity(parity.value)
        .stop_bits(stop.value)
        .flow_control(flow.value)
        .timeout(READ_POLL)
        // 要拿平台原生型別（Windows 的 `COMPort`）才能另外設寫入逾時，見下面 E12 的說明
        .open_native()
        .map_err(|e| {
            // Linux 沒進 dialout 群組的話開埠會是「權限不足」——那是**設定問題不是壞掉**，
            // 錯誤訊息要直接告訴使用者要跑哪一行（`CLAUDE.md` 平台差異表的權限那一欄）。
            if awayterm_platform::serial::needs_group_membership()
                && e.kind() == serialport::ErrorKind::NoDevice
            {
                // serialport 把 EACCES 歸到 NoDevice，描述文字才看得出是權限
                if e.description.to_ascii_lowercase().contains("permission") {
                    // 拿不到 `$USER` 時就用字面的 `$USER`——那在 shell 裡照樣是對的，
                    // 使用者可以整行複製貼上（比塞一個「<你的帳號>」佔位字好）。
                    let user = std::env::var("USER").unwrap_or_else(|_| "$USER".to_string());
                    let hint = awayterm_platform::serial::dialout_hint(&user);
                    return tf("err.comDialoutGroup", &[&hint]);
                }
            }
            tf("err.comOpenFailed", &[&params.port, &e.description])
        })?;

    // 舊版：DTR 一律拉起來；RTS 只在沒有硬體流控時自己設
    if let Err(e) = port.write_data_terminal_ready(true) {
        warnings.push(tf("err.comDtrFailed", &[&e.to_string()]));
    }
    if rts_should_be_set(&params.flow) {
        if let Err(e) = port.write_request_to_send(true) {
            warnings.push(tf("err.comRtsFailed", &[&e.to_string()]));
        }
    }

    // 寫入要在另一條執行緒上做 → 需要第二個 handle（`try_clone`）。
    // 讀取那條要短逾時輪詢，寫入那條要給流量控制留 2 秒（舊版 WriteTimeout），兩者不能共用同一個值。
    let mut writer = port
        .try_clone_native()
        .map_err(|e| tf("err.comHandleFailed", &[&params.port, &e.description]))?;
    set_split_timeouts(&port, &mut writer);

    Ok((
        Link {
            reader: Box::new(port),
            writer: Box::new(writer),
            name: params.port.clone(),
        },
        warnings,
    ))
}

/// 讀取 25ms、寫入 2 秒的逾時。
///
/// ⚠️ 稽核 E12：Windows 的逾時（`SetCommTimeouts`）是**裝置層級**的——`try_clone` 出來的
/// handle 是同一個 file object，第一版對 writer 呼叫 `set_timeout(2s)` 其實把讀取那條也改成
/// 2 秒：讀取執行緒每次要等滿 2 秒才醒來看「關了沒」，關分頁後埠最多再被佔 2 秒（馬上重開會失敗）。
/// `serialport` 的 `set_timeout` 讀寫一起設，沒辦法分開，所以 Windows 自己呼叫一次
/// `SetCommTimeouts`：讀的欄位照 `serialport` 的設法（有資料立刻回、沒有就等 `READ_POLL`），
/// 寫的欄位給 `WRITE_TIMEOUT`。
///
/// mac／Linux 的逾時是 `serialport` 自己用 `poll` 做的、存在各自的物件上，本來就分得開。
#[cfg(windows)]
fn set_split_timeouts(port: &serialport::COMPort, _writer: &mut serialport::COMPort) {
    use std::os::windows::io::AsRawHandle;

    /// Win32 `COMMTIMEOUTS`（五個 DWORD）。只用這一個 API，不為它多開 windows-sys 的 feature。
    #[repr(C)]
    struct CommTimeouts {
        read_interval_timeout: u32,
        read_total_timeout_multiplier: u32,
        read_total_timeout_constant: u32,
        write_total_timeout_multiplier: u32,
        write_total_timeout_constant: u32,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn SetCommTimeouts(h: *mut std::ffi::c_void, t: *const CommTimeouts) -> i32;
    }

    let t = CommTimeouts {
        // 同 serialport 的 `set_timeout`：MAXDWORD/MAXDWORD/常數＝「有資料就回，沒有就等常數毫秒」
        read_interval_timeout: u32::MAX,
        read_total_timeout_multiplier: u32::MAX,
        read_total_timeout_constant: READ_POLL.as_millis() as u32,
        write_total_timeout_multiplier: 0,
        write_total_timeout_constant: WRITE_TIMEOUT.as_millis() as u32,
    };
    // SAFETY：handle 由 `port` 持有、在這個呼叫期間有效；`t` 是正確配置的 COMMTIMEOUTS。
    let ok = unsafe { SetCommTimeouts(port.as_raw_handle(), &t) };
    if ok == 0 {
        println!(
            "[AwayTerminal] SetCommTimeouts 失敗（讀寫逾時沿用 {}ms）：{}",
            READ_POLL.as_millis(),
            std::io::Error::last_os_error()
        );
    }
}

#[cfg(not(windows))]
fn set_split_timeouts(_port: &serialport::TTYPort, writer: &mut serialport::TTYPort) {
    let _ = writer.set_timeout(WRITE_TIMEOUT);
}

// ---------------------------------------------------------------- session

/// 開埠成功時回報一次——這就是 COM 的「連上了」（重連退避靠它歸零）。
///
/// ⚠️ **不可以等輸出**：序列裝置可能永遠不主動說話（等你先打字），
/// 用「有輸出」當條件的話退避永遠不會歸零。這是
/// `docs/REGRESSION-CHECKLIST.md`「隱含契約」那條總則的第三個應用
/// （SSH ＝shell channel 開成功、Telnet ＝從 socket 讀到第一批位元組、COM ＝**開埠成功**）。
pub type OnConnected = Arc<dyn Fn() + Send + Sync>;

enum Cmd {
    Write(Vec<u8>),
    Stop,
}

pub struct ComSession {
    tx: Mutex<Option<mpsc::Sender<Cmd>>>,
    closed: Arc<AtomicBool>,
    /// 結束事件的守門人：**保證只發一次**，而且誰先到誰發
    /// （關分頁的人，或是讀取執行緒發現裝置不在了）。
    exit: ExitOnce,
    name: String,
}

/// 「結束事件只發一次」的共用旗標。
///
/// 舊版是兩條路各自判斷：`Dispose` 自己發、`ReadLoop` 的 finally 在 `!_disposed` 時才發。
/// 我們收成一個旗標，效果一樣但不必兩邊互相猜——而且**關分頁時立刻發**，
/// 不必等讀取那邊的逾時醒過來（第一版就是這樣漏掉的：`com_probe` 抓到「關分頁後結束事件=0」）。
#[derive(Clone)]
struct ExitOnce {
    fired: Arc<AtomicBool>,
    on_exit: OnExit,
}

impl ExitOnce {
    fn fire(&self) {
        if !self.fired.swap(true, Ordering::SeqCst) {
            (self.on_exit)(ExitInfo::ended(None));
        }
    }
}

impl TerminalSession for ComSession {
    fn write(&self, data: &[u8]) {
        if data.is_empty() || self.closed.load(Ordering::Relaxed) {
            return;
        }
        // 丟進佇列就回——寫入可能被流量控制擋住 2 秒，不能在呼叫端等（舊版同款）
        if let Some(tx) = self.tx.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            let _ = tx.send(Cmd::Write(data.to_vec()));
        }
    }

    fn resize(&self, _cols: u16, _rows: u16) {
        // 序列埠沒有視窗大小的概念（舊版的 Resize 也是空的）
    }

    fn pid(&self) -> u32 {
        0 // 遠端／裝置連線沒有本機子行程
    }

    fn backend_name(&self) -> &'static str {
        "serialport"
    }

    fn close(&self) {
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        // 舊版 Dispose：沒有優雅結束鍵，關掉就是關掉
        if let Some(tx) = self.tx.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = tx.send(Cmd::Stop);
        }
        // 舊版 `Dispose` 最後一行就是 `Exited?.Invoke()`——**不要等讀取執行緒醒過來**
        self.exit.fire();
    }
}

impl ComSession {
    pub fn port_name(&self) -> &str {
        &self.name
    }
}

impl Drop for ComSession {
    fn drop(&mut self) {
        self.close();
    }
}

/// 用一條現成的 [`Link`] 建 session。`com_probe` 用假裝置走的就是這條。
pub fn spawn_with_link(
    link: Link,
    on_output: OnOutput,
    on_exit: OnExit,
    on_connected: Option<OnConnected>,
) -> Arc<ComSession> {
    let Link {
        mut reader,
        mut writer,
        name,
    } = link;
    let closed = Arc::new(AtomicBool::new(false));
    let exit = ExitOnce {
        fired: Arc::new(AtomicBool::new(false)),
        on_exit,
    };
    let (tx, rx) = mpsc::channel::<Cmd>();
    let session = Arc::new(ComSession {
        tx: Mutex::new(Some(tx)),
        closed: closed.clone(),
        exit: exit.clone(),
        name: name.clone(),
    });

    // 開埠成功＝連上了（在起執行緒之前就回報，序列裝置可能永遠不說話）
    if let Some(cb) = &on_connected {
        cb();
    }

    // 寫入執行緒（舊版 WriteLoop）
    {
        let closed = closed.clone();
        std::thread::Builder::new()
            .name(format!("com-write-{name}"))
            .spawn(move || {
                while let Ok(cmd) = rx.recv() {
                    match cmd {
                        Cmd::Stop => break,
                        Cmd::Write(data) => {
                            if closed.load(Ordering::Relaxed) {
                                break;
                            }
                            // 逾時（對方擋住）或已關閉：丟掉這一筆，不卡任何人（舊版註解）
                            let _ = writer.write_all(&data).and_then(|()| writer.flush());
                        }
                    }
                }
            })
            .expect("spawn com write thread");
    }

    // 讀取執行緒（舊版 ReadLoop）
    {
        let closed = closed.clone();
        let exit = exit.clone();
        std::thread::Builder::new()
            .name(format!("com-read-{name}"))
            .spawn(move || {
                let mut buf = [0u8; 8192];
                loop {
                    if closed.load(Ordering::Relaxed) {
                        break;
                    }
                    match reader.read(&mut buf) {
                        Ok(0) => break, // 對端關閉（拔線／假裝置關掉）
                        Ok(n) => on_output(&buf[..n]),
                        // 這次沒資料（`serialport` 的逾時）→ 繼續等，順便看一下有沒有被關掉
                        Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {}
                        Err(_) => break, // 裝置消失／handle 關掉
                    }
                }
                // 拔線／裝置消失也要發結束事件，自動重連才會接手（舊版 ReadLoop 的 finally）。
                // 使用者關閉時 `close()` 已經發過了 → `ExitOnce` 會把這次吞掉。
                exit.fire();
            })
            .expect("spawn com read thread");
    }

    session
}

/// 開一個真的序列埠並建 session。警告訊息（例如「1.5 停止位元不支援」）由呼叫端印出來。
pub fn spawn(
    params: &ComParams,
    on_output: OnOutput,
    on_exit: OnExit,
    on_connected: Option<OnConnected>,
) -> Result<(Arc<ComSession>, Vec<String>), String> {
    let (link, warnings) = open(params)?;
    Ok((
        spawn_with_link(link, on_output, on_exit, on_connected),
        warnings,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 預設值逐項對舊版 `AppSettings`：COM5 / 115200 / 8 / None / One / None。
    #[test]
    fn defaults_match_old_version() {
        let p = ComParams::default();
        assert_eq!(p.port, "COM5");
        assert_eq!(p.baud, 115_200);
        assert_eq!(p.data_bits, 8);
        assert_eq!(p.parity, "None");
        assert_eq!(p.stop_bits, "One");
        assert_eq!(p.flow, "None");
    }

    /// 分頁標題照舊版 `OpenComDirect`。
    #[test]
    fn title_is_port_and_baud() {
        assert_eq!(ComParams::default().title(), "COM5 115200");
    }

    /// 鮑率清單與順序照舊版 `ComDialog.Bauds`。
    #[test]
    fn baud_list_matches_old_dialog() {
        assert_eq!(
            BAUD_RATES,
            &[9600, 19200, 38400, 57600, 115_200, 230_400, 460_800, 921_600]
        );
    }

    /// crate 支援的值不該有警告。
    #[test]
    fn supported_values_map_cleanly() {
        for name in ["None", "Odd", "Even"] {
            assert!(map_parity(name).warning.is_none(), "{name}");
        }
        for name in ["One", "Two"] {
            assert!(map_stop(name).warning.is_none(), "{name}");
        }
        for name in ["None", "XOnXOff", "RequestToSend"] {
            assert!(map_flow(name).warning.is_none(), "{name}");
        }
    }

    /// **crate 的限制**：舊版有、`serialport` 沒有的三個值要降級**並且說出來**
    /// （不可以安靜地換掉——使用者會以為設定生效了）。
    #[test]
    fn unsupported_values_warn_and_fall_back() {
        let mark = map_parity("Mark");
        assert_eq!(mark.value, serialport::Parity::None);
        assert!(mark.warning.unwrap().contains("Mark"));

        let one_five = map_stop("OnePointFive");
        assert_eq!(one_five.value, serialport::StopBits::One);
        assert!(one_five.warning.unwrap().contains("OnePointFive"));

        let both = map_flow("RequestToSendXOnXOff");
        assert_eq!(both.value, serialport::FlowControl::Hardware);
        assert!(both.warning.is_some());
    }

    /// RTS 只在沒有硬體流控時自己設（舊版註解：硬體流控時設 RTS 會丟例外）。
    #[test]
    fn rts_only_without_hardware_flow() {
        assert!(rts_should_be_set("None"));
        assert!(rts_should_be_set("XOnXOff"));
        assert!(!rts_should_be_set("RequestToSend"));
        assert!(!rts_should_be_set("RequestToSendXOnXOff"));
    }

    /// `COM9` 要排在 `COM10` 前面（字串排序會反過來）。
    #[test]
    fn ports_sort_naturally() {
        let mut names = ["COM10", "COM2", "COM9", "COM1"];
        names.sort_by_key(|n| natural_key(n));
        assert_eq!(names, ["COM1", "COM2", "COM9", "COM10"]);
    }

    /// 參數裡沒有任何祕密欄位（我的最愛與恢復分頁都存這個結構）。
    #[test]
    fn params_have_no_secret_field() {
        let json = serde_json::to_string(&ComParams::default()).unwrap();
        for bad in ["password", "passwd", "passphrase", "secret"] {
            assert!(!json.contains(bad), "COM 參數不可以有 {bad}：{json}");
        }
    }
}
