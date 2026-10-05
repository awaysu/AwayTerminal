//! 內建 SSH 後端（`russh`）。
//!
//! 舊版是**呼叫系統 `ssh.exe`**，所以「照抄舊版」在協定層不成立。基準分兩半：
//!
//! ### 使用流程照舊版（`MainWindow.xaml.cs`）
//! | 舊版 | 這裡 |
//! |---|---|
//! | 分頁標題先是 `host`，輸入帳號後變 `user@host` | 同 |
//! | 終端機裡問 `login as: `（不是跳視窗），Backspace 退格、Enter 送出 | 同（見 [`Login`]） |
//! | 關分頁送 Ctrl+D ×3 | 同（[`GRACEFUL_EXIT_BYTES`]） |
//! | 連不上時不要留一個「打字全被吞」的死分頁 | 同：連線失敗會走 `on_exit`，分頁回到可重連狀態 |
//!
//! ### 協定層照 PuTTY
//! | PuTTY | 這裡 |
//! |---|---|
//! | 連上之後才在終端機裡問 `login as:` | 同（**與舊版的差別**：舊版是連線前就問，因為帳號要放進 `ssh.exe` 的命令列） |
//! | 密碼提示 `user@host's password: `，不回顯 | 同 |
//! | keyboard-interactive 的提示由伺服器給，逐題問 | 同（`echo` 旗標決定要不要回顯） |
//! | 主機金鑰快取 + 第一次／變更兩種對話框 | 見 [`hostkey`] |
//!
//! ### 還沒做（TASK-007）
//! 演算法順序與弱演算法警告、keepalive、斷線自動重連、完整的 SSH 連線對話框。
//! 現在用 `russh` 的預設演算法清單（安全的那組），所以**很舊的設備可能連不上**——
//! 那正是 `CLAUDE.md` 風險 3 要用使用者的設備實測的部分，見 `docs/SSH.md`。

pub mod algos;
pub mod conn;
pub mod hostkey;
pub mod prompt;

use crate::i18n::{t, tf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use russh::client::{self, KeyboardInteractiveAuthResponse};
use russh::keys::ssh_key;
use russh::keys::PublicKeyOrCertificate;
use russh::ChannelMsg;
use tokio::runtime::Runtime;
use tokio::sync::{mpsc, watch};

use crate::session::{ExitInfo, OnExit, OnOutput, TerminalSession};

/// 關分頁時送的「優雅結束」位元組：**Ctrl+D ×3**（同舊版 SSH 分頁的 `GracefulExitBytes`）。
pub const GRACEFUL_EXIT_BYTES: [u8; 3] = [0x04, 0x04, 0x04];

/// 關閉時等遠端反應的時間（同 ConPTY 那邊的 60ms 量級）。
const CLOSE_WAIT: Duration = Duration::from_millis(120);

/// TCP 連線這一段的逾時（稽核 E15；不含交握與主機金鑰對話框）。同 Telnet 的 20 秒。
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

/// SSH 用的 tokio runtime。
///
/// 為什麼要自己開一個：`TerminalSession` 是同步介面（`write`／`resize`／`close`），
/// 而 `russh` 是 async。共用一個 runtime 讓 `ssh_probe` 這種非 tauri 的程式也能用同一份程式碼。
fn runtime() -> &'static Runtime {
    static RT: OnceLock<Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("awayterm-ssh")
            .enable_all()
            .build()
            .unwrap_or_else(|e| panic!("{}: {e}", t("err.sshRuntime")))
    })
}

// --------------------------------------------------------------- 對外設定

/// 使用者對這條連線指定的驗證素材。全部可省略——省略就是純密碼登入。
#[derive(Clone, Default)]
pub struct SshAuth {
    /// 私鑰檔路徑（OpenSSH 格式或 **`.ppk`**，`russh` 兩種都認）。
    pub key_path: Option<String>,
    /// 私鑰的密碼（有加密的金鑰才需要）。
    pub key_passphrase: Option<String>,
    /// 要不要試 Windows Pageant／SSH agent。找不到就安靜跳過。
    pub use_agent: bool,
    /// 連線視窗填的登入密碼（2.0.7）。有的話用它回答**第一次**的密碼提示——keyboard-interactive
    /// 第一個不回顯的題目、或 password 驗證的第一次——不必在終端機裡打；被拒絕就回到當場問
    /// （同 PuTTY 的 `-pw`）。`None`＝一律當場問。
    pub password: Option<String>,
}

pub struct SshOptions {
    pub host: String,
    pub port: u16,
    /// 已知的帳號。`None` ⇒ 在終端機裡問 `login as: `（PuTTY 式）。
    pub user: Option<String>,
    pub cols: u16,
    pub rows: u16,
    pub auth: SshAuth,
    /// 這條連線的演算法覆寫（空的＝用 PuTTY 式的預設順序，見 [`algos`]）。
    pub algos: algos::AlgoOverride,
    /// keepalive 間隔（分鐘）。0＝關閉。同舊版「保持連線」的設定。
    pub keepalive_mins: u32,
    /// 要送給遠端的環境變數（SSH `env` request）。舊版是 `ssh.exe` 的 `-o SendEnv=…`。
    /// 伺服器多半設了 `AcceptEnv` 白名單，被拒絕只印一行灰字、**不擋連線**。
    pub env: Vec<(String, String)>,
}

/// 「等使用者回答」的 future。
///
/// ⚠️ 一定要是 async（稽核 E1）：這些詢問是在 russh 的 handler 裡、也就是 SSH runtime 的
/// worker 上被等的；runtime 只有 2 條 worker，同步等對話框會把**所有** SSH 分頁一起凍住。
pub type Asking<'a, T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>>;

/// 主機金鑰要不要接受。由呼叫端提供——app 端跳對話框問使用者，`ssh_probe` 直接給答案。
pub trait HostKeyDecider: Send + Sync + 'static {
    /// `verdict` 是與 `known_hosts` 比對的結果；回傳 [`HostKeyAnswer`]。
    ///
    /// 分頁關閉時 SSH 任務會直接丟掉這個 future（當成 `Reject`），實作端不必自己處理取消。
    fn decide<'a>(
        &'a self,
        host: &'a str,
        port: u16,
        verdict: &'a hostkey::Verdict,
        fp: &'a hostkey::Fingerprints,
    ) -> Asking<'a, HostKeyAnswer>;

    /// 交握**實際協商到**警告線以下的演算法時呼叫（PuTTY 的 warn-below-this-line）。
    ///
    /// 實作端要負責「這台主機已經接受過這組演算法就不要再問」，但**不要在這裡寫記錄**：
    /// 這時主機金鑰還沒驗證（russh 先呼叫 `kex_done` 才呼叫 `check_server_key`）。
    /// 要記住的話回 [`WeakAnswer::AcceptAndRemember`]，驗證通過後會呼叫 [`Self::remember_weak`]。
    /// 預設 `Reject`（安全預設：沒有人回答就當成不接受）。
    fn accept_weak<'a>(
        &'a self,
        _host: &'a str,
        _port: u16,
        _weak: &'a [(String, String)],
    ) -> Asking<'a, WeakAnswer> {
        Box::pin(async { WeakAnswer::Reject })
    }

    /// 主機金鑰驗證通過之後，把使用者選「記住」的弱演算法寫下來（稽核 E9）。
    fn remember_weak(&self, _host: &str, _port: u16, _weak: &[(String, String)]) {}
}

/// 弱演算法警告的答案。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeakAnswer {
    /// 取消連線
    Reject,
    /// 繼續，不必記（只這次，或是之前已經記過）
    Accept,
    /// 繼續，主機金鑰驗證通過後記住這台主機＋這組演算法
    AcceptAndRemember,
}

/// PuTTY 對話框的三個選項。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HostKeyAnswer {
    /// 接受並儲存（PuTTY 的 Accept）
    AcceptAndStore,
    /// 只這次（PuTTY 的 Connect Once）
    AcceptOnce,
    /// 取消連線
    Reject,
}

// ------------------------------------------------------------------ session

enum Cmd {
    Write(Vec<u8>),
    Resize(u16, u16),
    Close,
}

/// 一條 SSH 連線。實作與 ConPTY 相同的 [`TerminalSession`] 介面，
/// 所以輸出走同一條二進位 channel、`session_resize` 走同一個指令。
pub struct SshSession {
    tx: mpsc::UnboundedSender<Cmd>,
    closed: AtomicBool,
    /// 遠端沒有本機 PID。舊版的狀態燈對遠端連線也不看 PID（只看「近期有輸出」）。
    pid: AtomicU32,
}

impl TerminalSession for SshSession {
    fn write(&self, data: &[u8]) {
        let _ = self.tx.send(Cmd::Write(data.to_vec()));
    }

    fn resize(&self, cols: u16, rows: u16) {
        let _ = self.tx.send(Cmd::Resize(cols, rows));
    }

    fn pid(&self) -> u32 {
        self.pid.load(Ordering::Relaxed)
    }

    fn backend_name(&self) -> &'static str {
        "russh"
    }

    fn close(&self) {
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        // 先送優雅結束鍵（Ctrl+D ×3＝登出遠端 shell），短暫等待後才真的收線。
        let _ = self.tx.send(Cmd::Write(GRACEFUL_EXIT_BYTES.to_vec()));
        std::thread::sleep(CLOSE_WAIT);
        let _ = self.tx.send(Cmd::Close);
    }
}

impl Drop for SshSession {
    fn drop(&mut self) {
        TerminalSession::close(self);
    }
}

/// 帳號確定之後回報一次（舊版：`login as:` 輸入完就把分頁標題改成 `user@host`）。
pub type OnUser = Arc<dyn Fn(&str) + Send + Sync>;

/// **shell 真的開起來了**才回報一次（重連的退避次數靠這個歸零）。
///
/// 舊版是「一收到輸出就歸零」（`OnSessionOutput` 的第一行）。我們不能照那樣做：
/// 我們自己的狀態訊息（「連線到 host:port …」、`login as:`）也是走同一條輸出 callback，
/// 會被誤認成「連上了」而讓退避永遠停在 3 秒（實測踩到）。
/// 改成「shell channel 開成功」——語意更精確，而且同樣達到
/// 「連成功過就不要繼續拉長退避」的目的。
pub type OnConnected = Arc<dyn Fn() + Send + Sync>;

/// 開一條 SSH 連線。**立刻回傳**，連線與驗證在背景進行，過程中的提示走 `on_output`。
pub fn spawn(
    opts: SshOptions,
    store: Arc<hostkey::HostKeyStore>,
    decider: Arc<dyn HostKeyDecider>,
    on_output: OnOutput,
    on_exit: OnExit,
    on_user: Option<OnUser>,
    on_connected: Option<OnConnected>,
) -> Arc<SshSession> {
    let (tx, rx) = mpsc::unbounded_channel();
    let session = Arc::new(SshSession {
        tx,
        closed: AtomicBool::new(false),
        pid: AtomicU32::new(0),
    });

    // E3：使用者**自己取消**（主機金鑰／弱演算法按「取消」、登入提示按 Ctrl+C）要和一般斷線
    // 分得出來，否則自動重連 3 秒後又問一次、無限循環。主機金鑰與弱演算法的答案從 decider
    // 包一層記下來；Ctrl+C 看 run() 回的錯誤訊息（`prompt_line` 回 `err.cancelled`）。
    struct CancelRecorder {
        inner: Arc<dyn HostKeyDecider>,
        cancelled: Arc<AtomicBool>,
    }
    // （E1 之後 decider 是 async，這層跟著改成包 future；remember_weak 也要轉給 inner，
    //   否則 E9 的「驗證通過後才記住」會被預設的空實作吞掉。）
    impl HostKeyDecider for CancelRecorder {
        fn decide<'a>(
            &'a self,
            host: &'a str,
            port: u16,
            verdict: &'a hostkey::Verdict,
            fp: &'a hostkey::Fingerprints,
        ) -> Asking<'a, HostKeyAnswer> {
            Box::pin(async move {
                let answer = self.inner.decide(host, port, verdict, fp).await;
                if answer == HostKeyAnswer::Reject {
                    self.cancelled.store(true, Ordering::SeqCst);
                }
                answer
            })
        }
        fn accept_weak<'a>(
            &'a self,
            host: &'a str,
            port: u16,
            weak: &'a [(String, String)],
        ) -> Asking<'a, WeakAnswer> {
            Box::pin(async move {
                let answer = self.inner.accept_weak(host, port, weak).await;
                if answer == WeakAnswer::Reject {
                    self.cancelled.store(true, Ordering::SeqCst);
                }
                answer
            })
        }
        fn remember_weak(&self, host: &str, port: u16, weak: &[(String, String)]) {
            self.inner.remember_weak(host, port, weak);
        }
    }
    let cancelled = Arc::new(AtomicBool::new(false));
    let decider: Arc<dyn HostKeyDecider> = Arc::new(CancelRecorder {
        inner: decider,
        cancelled: cancelled.clone(),
    });

    let out = on_output.clone();
    runtime().spawn(async move {
        let result = run(opts, store, decider, out.clone(), rx, on_user, on_connected).await;
        if let Err(msg) = &result {
            // 錯誤一律印在終端機裡（黃字），不要只進 log——使用者要看得到為什麼連不上
            echo(&out, &format!("\r\n\x1b[33m{msg}\x1b[0m\r\n"));
        }
        let user_cancelled = match &result {
            Ok(_) => false,
            Err(msg) => cancelled.load(Ordering::SeqCst) || *msg == t("err.cancelled"),
        };
        on_exit(ExitInfo {
            exit_code: match &result {
                Ok(code) => *code,
                Err(_) => None,
            },
            user_cancelled,
        });
    });

    session
}

fn echo(on_output: &OnOutput, text: &str) {
    on_output(text.as_bytes());
}

// ------------------------------------------------------------- 主機金鑰驗證

/// `russh` 的 client handler。只負責主機金鑰；其餘事件用預設行為。
struct Handler {
    host: String,
    port: u16,
    store: Arc<hostkey::HostKeyStore>,
    decider: Arc<dyn HostKeyDecider>,
    on_output: OnOutput,
    /// 弱演算法警告每條連線只問一次（rekey 時 `kex_done` 會再進來）。
    weak_asked: bool,
    /// 使用者選「記住」的弱演算法；**主機金鑰驗證通過後**才寫進設定（稽核 E9）。
    weak_to_remember: Option<Vec<(String, String)>>,
    /// 分頁關閉／連線放棄的訊號（`true` 或 sender 被丟掉＝取消）。
    /// 對話框開著時分頁被關掉，靠這個讓 handler 立刻當成「取消」收掉，不必等 180 秒逾時。
    cancel: watch::Receiver<bool>,
}

/// 等到「取消」為止（sender 被丟掉也算）。
async fn until_cancelled(cancel: &mut watch::Receiver<bool>) {
    loop {
        if *cancel.borrow_and_update() {
            return;
        }
        if cancel.changed().await.is_err() {
            return;
        }
    }
}

/// 等使用者回答；中途被取消回 `None`。
async fn or_cancel<T>(ask: Asking<'_, T>, cancel: &mut watch::Receiver<bool>) -> Option<T> {
    tokio::select! {
        v = ask => Some(v),
        _ = until_cancelled(cancel) => None,
    }
}

impl Handler {
    /// 主機金鑰驗證通過了：這時才把使用者選「記住」的弱演算法寫下來（稽核 E9）。
    fn host_verified(&mut self) {
        if let Some(weak) = self.weak_to_remember.take() {
            self.decider.remember_weak(&self.host, self.port, &weak);
        }
    }
}

impl client::Handler for Handler {
    type Error = russh::Error;

    /// 伺服器在驗證前送的公告文字（有些舊設備會送一大段）。照 PuTTY 顯示在畫面上。
    async fn auth_banner(
        &mut self,
        banner: &str,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        if !banner.trim().is_empty() {
            // 遠端的換行可能只有 LF，終端機要 CR LF 才會回到行首
            let text = banner.replace("\r\n", "\n").replace('\n', "\r\n");
            echo(&self.on_output, &text);
        }
        Ok(())
    }

    /// 交握完成：檢查**實際協商到**的演算法有沒有在 PuTTY 的警告線以下。
    async fn kex_done(
        &mut self,
        _shared_secret: Option<&[u8]>,
        names: &russh::Names,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        // 兩個方向的 MAC 都要看（稽核 E10：第一版只看 server_mac）
        let weak = algos::weak_ones(
            names.kex.as_ref(),
            names.key.as_str(),
            names.cipher.as_ref(),
            &[names.client_mac.as_ref(), names.server_mac.as_ref()],
        );
        // 每條連線只問一次（rekey 也會進來這裡）
        if weak.is_empty() || self.weak_asked {
            return Ok(());
        }
        self.weak_asked = true;
        let answer = or_cancel(
            self.decider.accept_weak(&self.host, self.port, &weak),
            &mut self.cancel,
        )
        .await
        .unwrap_or(WeakAnswer::Reject);
        if answer == WeakAnswer::AcceptAndRemember {
            self.weak_to_remember = Some(weak.clone());
        }
        if answer != WeakAnswer::Reject {
            let list = weak
                .iter()
                .map(|(k, n)| format!("{k}={n}"))
                .collect::<Vec<_>>()
                .join(" ");
            echo(
                &self.on_output,
                &format!("\x1b[90m（使用了較舊的加密演算法：{list}）\x1b[0m
"),
            );
            Ok(())
        } else {
            echo(
                &self.on_output,
                "
\x1b[33m已取消：這條連線會用到較舊、已知較弱的加密演算法。\x1b[0m
",
            );
            // 拒絕交握＝斷線。用 Disconnect 讓上層的錯誤訊息合理
            Err(russh::Error::Disconnect)
        }
    }

    async fn check_server_key(
        &mut self,
        key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let PublicKeyOrCertificate::PublicKey { key, .. } = key else {
            // 憑證式主機金鑰要先知道信任哪個 CA，這個階段不支援 → 明確拒絕，不要默默放行
            echo(
                &self.on_output,
                &format!("\r\n\x1b[33m{}\x1b[0m\r\n", t("term.sshCertUnsupported")),
            );
            return Ok(false);
        };

        let verdict = self.store.check(&self.host, self.port, key);
        if verdict == hostkey::Verdict::Known {
            self.host_verified();
            return Ok(true);
        }

        let fp = hostkey::fingerprints(key);
        // 分頁在對話框開著時被關掉 → 當成取消（不必等 180 秒逾時）
        let answer = or_cancel(
            self.decider.decide(&self.host, self.port, &verdict, &fp),
            &mut self.cancel,
        )
        .await
        .unwrap_or(HostKeyAnswer::Reject);
        match answer {
            HostKeyAnswer::AcceptAndStore => {
                // `learn` 會先拿掉這台主機同型別的舊記錄（金鑰變更時「接受並儲存」才真的生效，稽核 E2）
                if let Err(e) = self.store.learn(&self.host, self.port, key) {
                    // 存不起來就講出來，但這次連線照使用者的意思繼續
                    echo(&self.on_output, &format!("\r\n\x1b[33m{e}\x1b[0m\r\n"));
                }
                self.host_verified();
                Ok(true)
            }
            HostKeyAnswer::AcceptOnce => {
                self.host_verified();
                Ok(true)
            }
            HostKeyAnswer::Reject => Ok(false),
        }
    }
}

// ------------------------------------------------------------------ 主流程

/// 回傳遠端 shell 的 exit code（拿不到時 `None`）。
async fn run(
    opts: SshOptions,
    store: Arc<hostkey::HostKeyStore>,
    decider: Arc<dyn HostKeyDecider>,
    on_output: OnOutput,
    rx: mpsc::UnboundedReceiver<Cmd>,
    on_user: Option<OnUser>,
    on_connected: Option<OnConnected>,
) -> Result<Option<i32>, String> {
    let mut input = Input::new(rx, opts.cols, opts.rows);
    // 取消訊號：run() 結束（不論成敗）時 sender 被丟掉，handler 裡還在等的對話框就當成取消
    let (cancel_tx, cancel_rx) = watch::channel(false);

    // 演算法順序照 PuTTY（B4）：舊演算法在清單裡但排最後，協商到就跳警告。
    let (preferred, unknown) = algos::preferred(&opts.algos);
    if !unknown.is_empty() {
        echo(
            &on_output,
            &format!(
                "\x1b[33m設定裡有認不出來的演算法名稱，已略過：{}\x1b[0m
",
                unknown.join("、")
            ),
        );
    }
    let config = Arc::new(client::Config {
        preferred,
        // 保持連線（同舊版的 ServerAliveInterval／ServerAliveCountMax=3）。
        // russh 送的是 `keepalive@openssh.com` global request，PuTTY 預設也是這條。
        keepalive_interval: (opts.keepalive_mins > 0)
            .then(|| Duration::from_secs(opts.keepalive_mins as u64 * 60)),
        keepalive_max: 3,
        ..Default::default()
    });

    let handler = Handler {
        host: opts.host.clone(),
        port: opts.port,
        store,
        decider,
        on_output: on_output.clone(),
        weak_asked: false,
        weak_to_remember: None,
        cancel: cancel_rx,
    };

    echo(
        &on_output,
        &format!(
            "\x1b[90m{}\x1b[0m\r\n",
            tf("term.sshConnecting", &[&opts.host, &opts.port.to_string()])
        ),
    );

    // 連線（含交握與主機金鑰對話框）期間也要聽分頁的指令（稽核 E15）：
    // 關分頁要能立刻取消；打字／改尺寸先收起來，給後面的提示與 shell 用。
    let connecting = open_transport(config, &opts.host, opts.port, handler);
    tokio::pin!(connecting);
    let mut handle = loop {
        tokio::select! {
            r = &mut connecting => break r?,
            cmd = input.rx.recv() => match cmd {
                Some(Cmd::Close) | None => {
                    let _ = cancel_tx.send(true);
                    return Err(t("err.connCancelled").to_string());
                }
                Some(other) => input.stash(other),
            },
        }
    };

    // ---- 帳號：PuTTY 式在終端機裡問 ----
    let user = match opts.user.clone() {
        Some(u) if !u.trim().is_empty() => u,
        _ => {
            let u = prompt_line(&on_output, &mut input, "login as: ", true).await?;
            if u.trim().is_empty() {
                return Err(t("err.sshNoUser").to_string());
            }
            u.trim().to_string()
        }
    };
    // 舊版：帳號確定就把分頁標題改成 user@host（並送 `t` 同步 pane 標題）
    if let Some(cb) = &on_user {
        cb(&user);
    }

    authenticate(&mut handle, &user, &opts, &on_output, &mut input).await?;

    // ---- 開 shell channel ----
    let channel = handle
        .channel_open_session()
        .await
        .map_err(|e| tf("err.sshSessionFailed", &[&e.to_string()]))?;
    // 用**最新**的尺寸（稽核 E6：`login as:`／密碼提示期間改了視窗大小，第一版會丟掉）
    let (cols, rows) = input.size;
    channel
        .request_pty(
            false,
            "xterm-256color",
            cols as u32,
            rows as u32,
            0,
            0,
            &[],
        )
        .await
        .map_err(|e| tf("err.sshPtyFailed", &[&e.to_string()]))?;

    // 送出環境變數（舊版 `ssh.exe` 的 `-o SendEnv=…`）。
    // 伺服器多半設了 `AcceptEnv` 白名單，沒放行就會回 failure——**只印一行灰字、不擋連線**
    // （PuTTY 也是這個態度）。`want_reply: false` 讓被拒時不會卡在等回覆。
    for (k, v) in &opts.env {
        if k.trim().is_empty() {
            continue;
        }
        if let Err(e) = channel.set_env(false, k.as_str(), v.as_str()).await {
            echo(
                &on_output,
                &format!(
                    "\x1b[90m{}\x1b[0m\r\n",
                    tf("term.sshEnvFailed", &[k.as_str(), &e.to_string()])
                ),
            );
        }
    }

    channel
        .request_shell(false)
        .await
        .map_err(|e| tf("err.sshShellFailed", &[&e.to_string()]))?;

    // 到這裡才算「真的連上了」：重連的退避次數在這裡歸零（見 OnConnected 的說明）
    if let Some(cb) = &on_connected {
        cb();
    }

    pump(channel, on_output, input).await
}

/// 連上 shell 之後的主迴圈：本機輸入 → 遠端，遠端輸出 → 畫面。
async fn pump(
    mut channel: russh::Channel<client::Msg>,
    on_output: OnOutput,
    input: Input,
) -> Result<Option<i32>, String> {
    let Input { mut rx, pending, .. } = input;
    // 提示期間多打（或多貼）的位元組照順序交給 shell（稽核 E7：第一版整段丟掉）
    if !pending.is_empty() && channel.data(&pending[..]).await.is_err() {
        return Ok(None);
    }
    let mut exit_code = None;
    loop {
        tokio::select! {
            cmd = rx.recv() => match cmd {
                Some(Cmd::Write(data)) => {
                    if channel.data(&data[..]).await.is_err() {
                        break;
                    }
                }
                Some(Cmd::Resize(cols, rows)) => {
                    // SSH 的 window-change（對應 ConPTY 的 ResizePseudoConsole）
                    let _ = channel.window_change(cols as u32, rows as u32, 0, 0).await;
                }
                // 分頁關閉或程式結束
                Some(Cmd::Close) | None => {
                    let _ = channel.eof().await;
                    let _ = channel.close().await;
                    break;
                }
            },
            msg = channel.wait() => match msg {
                Some(ChannelMsg::Data { data }) => on_output(&data),
                // stderr（extended data）也要顯示，否則遠端的錯誤訊息會不見
                Some(ChannelMsg::ExtendedData { data, .. }) => on_output(&data),
                Some(ChannelMsg::ExitStatus { exit_status }) => exit_code = Some(exit_status as i32),
                Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => break,
                Some(_) => {}
            },
        }
    }
    Ok(exit_code)
}

// -------------------------------------------------------------------- 驗證

async fn authenticate(
    handle: &mut client::Handle<Handler>,
    user: &str,
    opts: &SshOptions,
    on_output: &OnOutput,
    input: &mut Input,
) -> Result<(), String> {
    // PuTTY 也會先送一次 none：一方面有些設備真的不需要驗證，
    // 另一方面伺服器會在回覆裡列出它支援哪些方法。
    if let Ok(r) = handle.authenticate_none(user).await {
        if r.success() {
            return Ok(());
        }
    }

    // ---- 1. publickey（明確指定的金鑰檔；OpenSSH 或 .ppk）----
    if let Some(path) = opts.auth.key_path.as_deref() {
        match load_key(path, opts.auth.key_passphrase.as_deref(), on_output, input).await {
            Ok(key) => {
                let hash_alg = handle.best_supported_rsa_hash().await.ok().flatten().flatten();
                let with_hash = russh::keys::PrivateKeyWithHashAlg::new(Arc::new(key), hash_alg);
                match handle.authenticate_publickey(user, with_hash).await {
                    Ok(r) if r.success() => return Ok(()),
                    Ok(_) => echo(
                        on_output,
                        &format!("\x1b[90m{}\x1b[0m\r\n", t("term.sshKeyRejected")),
                    ),
                    Err(e) => echo(on_output, &format!(
                            "\x1b[90m{}\x1b[0m\r\n",
                            tf("term.sshKeyAuthFailed", &[&e.to_string()])
                        )),
                }
            }
            Err(e) => echo(on_output, &format!("\x1b[33m{e}\x1b[0m\r\n")),
        }
    }

    // ---- 2. Pageant / SSH agent（找不到就安靜跳過）----
    if opts.auth.use_agent {
        match try_agent(handle, user).await {
            Ok(true) => return Ok(()),
            Ok(false) => {}
            Err(e) => println!("[AwayTerminal] agent 驗證跳過：{e}"),
        }
    }

    // 連線視窗填的密碼：只用一次（第一個不回顯的題目，或第一次 password 驗證），
    // 之後不管對錯都回到當場問（同 PuTTY 的 `-pw`）
    let mut stored = opts.auth.password.clone().filter(|p| !p.is_empty());

    // ---- 3. keyboard-interactive（提示由伺服器給，逐題問）----
    match handle
        .authenticate_keyboard_interactive_start(user, None)
        .await
    {
        Ok(mut resp) => loop {
            match resp {
                KeyboardInteractiveAuthResponse::Success => return Ok(()),
                KeyboardInteractiveAuthResponse::Failure { .. } => break,
                KeyboardInteractiveAuthResponse::InfoRequest {
                    name,
                    instructions,
                    prompts,
                } => {
                    if !name.trim().is_empty() {
                        echo(on_output, &format!("{}\r\n", name.trim_end()));
                    }
                    if !instructions.trim().is_empty() {
                        echo(on_output, &format!("{}\r\n", instructions.trim_end()));
                    }
                    let mut answers = Vec::with_capacity(prompts.len());
                    for p in &prompts {
                        // 不回顯的題目＝密碼：有填就直接答，畫面上照樣留下提示（看得出用了哪一步）
                        if !p.echo {
                            if let Some(pw) = stored.take() {
                                echo(on_output, &format!("{}\r\n", p.prompt));
                                answers.push(pw);
                                continue;
                            }
                        }
                        answers.push(prompt_line(on_output, input, &p.prompt, p.echo).await?);
                    }
                    resp = handle
                        .authenticate_keyboard_interactive_respond(answers)
                        .await
                        .map_err(|e| tf("err.sshAuthFailed", &[&e.to_string()]))?;
                }
            }
        },
        Err(e) => println!("[AwayTerminal] keyboard-interactive 不可用：{e}"),
    }

    // ---- 4. 密碼（PuTTY 的提示文字）----
    for attempt in 1..=3 {
        let prompt = format!("{}@{}'s password: ", user, opts.host);
        let pw = match stored.take() {
            // 連線視窗填的密碼：第一次直接用（提示照樣印出來，不印密碼）
            Some(pw) => {
                echo(on_output, &format!("{prompt}\r\n"));
                pw
            }
            None => prompt_line(on_output, input, &prompt, false).await?,
        };
        match handle.authenticate_password(user, pw).await {
            Ok(r) if r.success() => return Ok(()),
            Ok(_) => {
                echo(on_output, "Access denied\r\n");
                if attempt == 3 {
                    return Err(t("err.sshBadPassword3").to_string());
                }
            }
            Err(e) => return Err(tf("err.sshAuthFailed", &[&e.to_string()])),
        }
    }
    Err(t("err.sshAuthFailedPlain").to_string())
}

/// 讀一把私鑰。有密碼保護而使用者沒給時，在終端機裡問（不回顯）。
async fn load_key(
    path: &str,
    passphrase: Option<&str>,
    on_output: &OnOutput,
    input: &mut Input,
) -> Result<ssh_key::PrivateKey, String> {
    let text = std::fs::read_to_string(path).map_err(|e| tf("err.sshKeyRead", &[path, &e.to_string()]))?;
    match russh::keys::decode_secret_key(&text, passphrase) {
        Ok(k) => Ok(k),
        Err(_) if passphrase.is_none() => {
            // OpenSSH 與 .ppk 的加密金鑰都會走到這裡
            let pw = prompt_line(on_output, input, "Passphrase for key: ", false).await?;
            russh::keys::decode_secret_key(&text, Some(&pw))
                .map_err(|e| tf("err.sshKeyDecrypt", &[&e.to_string()]))
        }
        Err(e) => Err(tf("err.sshKeyLoad", &[&e.to_string()])),
    }
}

/// 用 Pageant（Windows）／SSH agent 的金鑰驗證。回傳 `Ok(false)` ＝有 agent 但沒有一把能用。
///
/// `russh` 已經替 `AgentClient` 實作了 `Signer`（`src/auth.rs`），所以不必自己包一層。
async fn try_agent(handle: &mut client::Handle<Handler>, user: &str) -> Result<bool, String> {
    #[cfg(windows)]
    let mut agent = russh::keys::agent::client::AgentClient::connect_pageant()
        .await
        .map_err(|e| tf("err.pageantMissing", &[&e.to_string()]))?;
    #[cfg(not(windows))]
    let mut agent = {
        let path = std::env::var("SSH_AUTH_SOCK").map_err(|_| t("err.noAuthSock").to_string())?;
        let stream = tokio::net::UnixStream::connect(path)
            .await
            .map_err(|e| tf("err.agentConnect", &[&e.to_string()]))?;
        russh::keys::agent::client::AgentClient::connect(stream)
    };

    let identities = agent
        .request_identities()
        .await
        .map_err(|e| tf("err.agentIdentities", &[&e.to_string()]))?;
    if identities.is_empty() {
        return Ok(false);
    }
    let hash_alg = handle.best_supported_rsa_hash().await.ok().flatten().flatten();
    for id in identities {
        let russh::keys::agent::AgentIdentity::PublicKey { key, .. } = id else {
            continue; // 憑證身分這個階段不支援
        };
        match handle
            .authenticate_publickey_with(user, key, hash_alg, &mut agent)
            .await
        {
            Ok(r) if r.success() => return Ok(true),
            _ => continue,
        }
    }
    Ok(false)
}

// ------------------------------------------------------- 終端機裡的問答
//
// 舊版 `HandleLoginInput`：Enter 送出、Backspace 退格（回顯 `\b \b`）、其餘字元回顯。
// 這裡多了「不回顯」模式給密碼用（PuTTY 打密碼時畫面完全不動）。

/// 分頁送進來、但還沒交給 shell 的東西（連線與登入提示期間）。
///
/// - `pending`：一次寫入裡「這一行之後」的位元組（稽核 E7：貼 `user\npass\n` 時第一版只拿到 user，
///   其餘整段消失）。留給下一個提示；shell 開起來時還有剩就照順序送過去。
/// - `size`：提示期間最後一次的視窗大小（稽核 E6：第一版 `Resize => continue` 直接丟掉，
///   `request_pty` 用的還是開分頁當下的尺寸）。
struct Input {
    rx: mpsc::UnboundedReceiver<Cmd>,
    pending: Vec<u8>,
    size: (u16, u16),
}

impl Input {
    fn new(rx: mpsc::UnboundedReceiver<Cmd>, cols: u16, rows: u16) -> Self {
        Self {
            rx,
            pending: Vec::new(),
            size: (cols, rows),
        }
    }

    /// 連線中收到的指令先收起來（`Close` 由呼叫端自己處理）。
    fn stash(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Write(d) => self.pending.extend_from_slice(&d),
            Cmd::Resize(c, r) => {
                if c > 0 && r > 0 {
                    self.size = (c, r);
                }
            }
            Cmd::Close => {}
        }
    }
}

/// 一行讀到哪裡了。
#[derive(Debug, PartialEq, Eq)]
enum LineStep {
    /// 還沒到行尾，要等更多輸入。
    More,
    /// 讀到行尾（CR／LF）。
    Done,
    /// Ctrl+C
    Cancel,
}

/// 從 `pending` 前面吃位元組進 `buf`，直到行尾、Ctrl+C 或吃完。吃掉的位元組從 `pending` 移除，
/// **行尾之後的留著**給下一個提示（E7）。回傳（讀到哪裡, 要回顯的文字）。
///
/// UTF-8 字被切在兩次寫入之間時，不完整的尾巴留在 `pending` 等下一次（不可以先解成 U+FFFD）。
fn take_line(pending: &mut Vec<u8>, buf: &mut String, echo_input: bool) -> (LineStep, String) {
    let mut shown = String::new();
    let mut used = 0;
    let mut step = LineStep::More;
    'outer: while used < pending.len() {
        let rest = &pending[used..];
        let valid = match std::str::from_utf8(rest) {
            Ok(s) => s,
            Err(e) => {
                let v = std::str::from_utf8(&rest[..e.valid_up_to()]).unwrap_or_default();
                if !v.is_empty() {
                    v
                } else if let Some(n) = e.error_len() {
                    used += n; // 壞位元組直接跳過
                    continue;
                } else {
                    break; // 不完整的尾巴：留著等下一次
                }
            }
        };
        let mut chars = valid.char_indices().peekable();
        while let Some((i, ch)) = chars.next() {
            match ch {
                '\r' | '\n' => {
                    let mut len = i + 1;
                    // CR LF 算一個行尾（否則 LF 會變成下一個提示的空白答案）
                    if ch == '\r' && matches!(chars.peek(), Some((_, '\n'))) {
                        len += 1;
                    }
                    used += len;
                    step = LineStep::Done;
                    break 'outer;
                }
                '\u{7f}' | '\u{8}' => {
                    if buf.pop().is_some() && echo_input {
                        shown.push_str("\u{8} \u{8}"); // 同舊版的 "\b \b"
                    }
                }
                // Ctrl+C：取消整條連線（PuTTY 在登入階段按 Ctrl+C 也是斷線）
                '\u{3}' => {
                    used += i + 1;
                    step = LineStep::Cancel;
                    break 'outer;
                }
                c if c.is_control() => {}
                c => {
                    buf.push(c);
                    if echo_input {
                        shown.push(c);
                    }
                }
            }
        }
        used += valid.len();
    }
    pending.drain(..used);
    (step, shown)
}

/// 從輸入流讀一行。`echo = false` 時完全不回顯（密碼）。
async fn prompt_line(
    on_output: &OnOutput,
    input: &mut Input,
    prompt: &str,
    echo_input: bool,
) -> Result<String, String> {
    echo(on_output, prompt);
    let mut buf = String::new();
    loop {
        // 先吃上一次留下來的（同一筆寫入裡上一行之後的位元組）
        let (step, shown) = take_line(&mut input.pending, &mut buf, echo_input);
        if !shown.is_empty() {
            echo(on_output, &shown);
        }
        match step {
            LineStep::Done => {
                echo(on_output, "\r\n");
                return Ok(buf);
            }
            LineStep::Cancel => return Err(t("err.cancelled").to_string()),
            LineStep::More => {}
        }
        let Some(cmd) = input.rx.recv().await else {
            return Err(t("err.connCancelled").to_string());
        };
        match cmd {
            Cmd::Close => return Err(t("err.connCancelled").to_string()),
            // 問答期間也可能改變視窗大小：記下來給 request_pty（E6）
            other => input.stash(other),
        }
    }
}

/// 先 TCP 連線（有逾時），再交給 russh 交握（稽核 E15）。
///
/// 為什麼不直接用 `client::connect`：它裡面的 `TcpStream::connect` 沒有逾時，
/// 黑洞主機在 Windows 要等約 21 秒、Linux 最長 2 分鐘，而且這段期間關分頁也收不掉。
/// 逾時**只包 TCP 連線這一段**——交握裡還有主機金鑰對話框，那要等人按，不能算進去。
async fn open_transport(
    config: Arc<client::Config>,
    host: &str,
    port: u16,
    handler: Handler,
) -> Result<client::Handle<Handler>, String> {
    let tcp = match tokio::time::timeout(CONNECT_TIMEOUT, tokio::net::TcpStream::connect((host, port))).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => return Err(tf("err.sshConnectFailed", &[&e.to_string()])),
        Err(_) => {
            let why = tf("err.sshConnectTimeout", &[&CONNECT_TIMEOUT.as_secs().to_string()]);
            return Err(tf("err.sshConnectFailed", &[&why]));
        }
    };
    if config.nodelay {
        let _ = tcp.set_nodelay(true); // 同 `client::connect`
    }
    client::connect_stream(config, tcp, handler)
        .await
        .map_err(|e| tf("err.sshConnectFailed", &[&e.to_string()]))
}

// --------------------------------------------------------------- 給測試用

/// 固定答案的 decider（`ssh_probe` 用）。
pub struct FixedDecider {
    pub answer: HostKeyAnswer,
    /// 實際被問到的 verdict，測試拿來斷言。
    pub seen: Mutex<Vec<hostkey::Verdict>>,
    /// 弱演算法警告的答案，以及被問了幾次。
    accept_weak: AtomicBool,
    weak_asks: AtomicUsize,
}

impl FixedDecider {
    pub fn new(answer: HostKeyAnswer) -> Arc<Self> {
        Arc::new(Self {
            answer,
            seen: Mutex::new(Vec::new()),
            accept_weak: AtomicBool::new(true),
            weak_asks: AtomicUsize::new(0),
        })
    }
}

impl FixedDecider {
    /// `ssh_probe` 用：弱演算法要不要接受（以及記錄被問過幾次）。
    pub fn set_accept_weak(&self, yes: bool) {
        self.accept_weak
            .store(yes, std::sync::atomic::Ordering::SeqCst);
    }
    pub fn weak_asks(&self) -> usize {
        self.weak_asks
            .load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl HostKeyDecider for FixedDecider {
    fn decide<'a>(
        &'a self,
        _host: &'a str,
        _port: u16,
        verdict: &'a hostkey::Verdict,
        _fp: &'a hostkey::Fingerprints,
    ) -> Asking<'a, HostKeyAnswer> {
        self.seen
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(verdict.clone());
        let answer = self.answer;
        Box::pin(async move { answer })
    }

    fn accept_weak<'a>(
        &'a self,
        _host: &'a str,
        _port: u16,
        _weak: &'a [(String, String)],
    ) -> Asking<'a, WeakAnswer> {
        self.weak_asks
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let yes = self
            .accept_weak
            .load(std::sync::atomic::Ordering::SeqCst);
        Box::pin(async move {
            if yes {
                WeakAnswer::Accept
            } else {
                WeakAnswer::Reject
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(pending: &mut Vec<u8>) -> (LineStep, String) {
        let mut buf = String::new();
        let (step, _) = take_line(pending, &mut buf, true);
        (step, buf)
    }

    /// 稽核 E7：一次貼 `user\npass\n`，兩個提示各拿到自己那一行。
    #[test]
    fn pasted_lines_are_kept_for_next_prompt() {
        let mut p = b"root\r\nsecret\nls\n".to_vec();
        assert_eq!(line(&mut p), (LineStep::Done, "root".to_string()));
        assert_eq!(line(&mut p), (LineStep::Done, "secret".to_string()), "CR LF 只算一個行尾");
        assert_eq!(p, b"ls\n", "剩下的留給 shell");
    }

    #[test]
    fn partial_line_waits_for_more() {
        let mut p = b"ro".to_vec();
        let mut buf = String::new();
        assert_eq!(take_line(&mut p, &mut buf, true).0, LineStep::More);
        assert!(p.is_empty());
        p.extend_from_slice(b"ot\x7f\x7fot\r");
        assert_eq!(take_line(&mut p, &mut buf, true).0, LineStep::Done);
        assert_eq!(buf, "root");
    }

    /// 中文字被切在兩次寫入之間：不完整的尾巴要留著，不能變成 U+FFFD。
    #[test]
    fn utf8_split_is_kept() {
        let bytes = "使用者\r".as_bytes();
        let mut p = bytes[..4].to_vec();
        let mut buf = String::new();
        assert_eq!(take_line(&mut p, &mut buf, true).0, LineStep::More);
        assert_eq!(buf, "使");
        assert_eq!(p.len(), 1, "半個字留著");
        p.extend_from_slice(&bytes[4..]);
        assert_eq!(take_line(&mut p, &mut buf, true).0, LineStep::Done);
        assert_eq!(buf, "使用者");
    }

    #[test]
    fn ctrl_c_cancels() {
        let mut p = b"ab\x03cd".to_vec();
        assert_eq!(line(&mut p).0, LineStep::Cancel);
    }

    /// 稽核 E6：提示期間的 resize 要記下來。
    #[test]
    fn resize_during_prompt_is_remembered() {
        let (_tx, rx) = mpsc::unbounded_channel();
        let mut input = Input::new(rx, 80, 24);
        input.stash(Cmd::Resize(132, 50));
        input.stash(Cmd::Resize(0, 0)); // 0 不算
        input.stash(Cmd::Write(b"x".to_vec()));
        assert_eq!(input.size, (132, 50));
        assert_eq!(input.pending, b"x");
    }
}
