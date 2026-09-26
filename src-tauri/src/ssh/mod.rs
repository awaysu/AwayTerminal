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
pub mod hostkey;
pub mod prompt;

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use russh::client::{self, KeyboardInteractiveAuthResponse};
use russh::keys::ssh_key;
use russh::keys::PublicKeyOrCertificate;
use russh::ChannelMsg;
use tokio::runtime::Runtime;
use tokio::sync::mpsc;

use crate::session::{ExitInfo, OnExit, OnOutput, TerminalSession};

/// 關分頁時送的「優雅結束」位元組：**Ctrl+D ×3**（同舊版 SSH 分頁的 `GracefulExitBytes`）。
pub const GRACEFUL_EXIT_BYTES: [u8; 3] = [0x04, 0x04, 0x04];

/// 關閉時等遠端反應的時間（同 ConPTY 那邊的 60ms 量級）。
const CLOSE_WAIT: Duration = Duration::from_millis(120);

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
            .expect("建立 SSH runtime 失敗")
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
}

/// 主機金鑰要不要接受。由呼叫端提供——app 端跳對話框問使用者，`ssh_probe` 直接給答案。
pub trait HostKeyDecider: Send + Sync + 'static {
    /// `verdict` 是與 `known_hosts` 比對的結果；回傳 [`HostKeyAnswer`]。
    fn decide(
        &self,
        host: &str,
        port: u16,
        verdict: &hostkey::Verdict,
        fp: &hostkey::Fingerprints,
    ) -> HostKeyAnswer;

    /// 交握**實際協商到**警告線以下的演算法時呼叫（PuTTY 的 warn-below-this-line）。
    ///
    /// 回傳 `true`＝繼續連。實作端要負責「這台主機已經接受過就不要再問」。
    /// 預設 `false`（安全預設：沒有人回答就當成不接受）。
    fn accept_weak(&self, _host: &str, _port: u16, _weak: &[(&'static str, String)]) -> bool {
        false
    }
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

/// 開一條 SSH 連線。**立刻回傳**，連線與驗證在背景進行，過程中的提示走 `on_output`。
pub fn spawn(
    opts: SshOptions,
    store: Arc<hostkey::HostKeyStore>,
    decider: Arc<dyn HostKeyDecider>,
    on_output: OnOutput,
    on_exit: OnExit,
    on_user: Option<OnUser>,
) -> Arc<SshSession> {
    let (tx, rx) = mpsc::unbounded_channel();
    let session = Arc::new(SshSession {
        tx,
        closed: AtomicBool::new(false),
        pid: AtomicU32::new(0),
    });

    let out = on_output.clone();
    runtime().spawn(async move {
        let result = run(opts, store, decider, out.clone(), rx, on_user).await;
        if let Err(msg) = &result {
            // 錯誤一律印在終端機裡（黃字），不要只進 log——使用者要看得到為什麼連不上
            echo(&out, &format!("\r\n\x1b[33m{msg}\x1b[0m\r\n"));
        }
        on_exit(ExitInfo {
            exit_code: match &result {
                Ok(code) => *code,
                Err(_) => None,
            },
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
        let weak = algos::weak_ones(
            names.kex.as_ref(),
            names.key.as_str(),
            names.cipher.as_ref(),
            names.server_mac.as_ref(),
        );
        // 每條連線只問一次（rekey 也會進來這裡）
        if weak.is_empty() || self.weak_asked {
            return Ok(());
        }
        self.weak_asked = true;
        if self.decider.accept_weak(&self.host, self.port, &weak) {
            let list = weak
                .iter()
                .map(|(k, n)| format!("{k}={n}"))
                .collect::<Vec<_>>()
                .join(" ");
            echo(
                &self.on_output,
                &format!("[90m（使用了較舊的加密演算法：{list}）[0m
"),
            );
            Ok(())
        } else {
            echo(
                &self.on_output,
                "
[33m已取消：這條連線會用到較舊、已知較弱的加密演算法。[0m
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
                "\r\n\x1b[33m這台主機用 OpenSSH 憑證當主機金鑰，目前還不支援。\x1b[0m\r\n",
            );
            return Ok(false);
        };

        let verdict = self.store.check(&self.host, self.port, key);
        if verdict == hostkey::Verdict::Known {
            return Ok(true);
        }

        let fp = hostkey::fingerprints(key);
        let answer = self.decider.decide(&self.host, self.port, &verdict, &fp);
        match answer {
            HostKeyAnswer::AcceptAndStore => {
                if let Err(e) = self.store.learn(&self.host, self.port, key) {
                    // 存不起來就講出來，但這次連線照使用者的意思繼續
                    echo(&self.on_output, &format!("\r\n\x1b[33m{e}\x1b[0m\r\n"));
                }
                Ok(true)
            }
            HostKeyAnswer::AcceptOnce => Ok(true),
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
    mut rx: mpsc::UnboundedReceiver<Cmd>,
    on_user: Option<OnUser>,
) -> Result<Option<i32>, String> {
    // 演算法順序照 PuTTY（B4）：舊演算法在清單裡但排最後，協商到就跳警告。
    let (preferred, unknown) = algos::preferred(&opts.algos);
    if !unknown.is_empty() {
        echo(
            &on_output,
            &format!(
                "[33m設定裡有認不出來的演算法名稱，已略過：{}[0m
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
    };

    echo(
        &on_output,
        &format!("\x1b[90m連線到 {}:{} …\x1b[0m\r\n", opts.host, opts.port),
    );

    let mut handle = client::connect(config, (opts.host.as_str(), opts.port), handler)
        .await
        .map_err(|e| format!("連線失敗：{e}"))?;

    // ---- 帳號：PuTTY 式在終端機裡問 ----
    let user = match opts.user.clone() {
        Some(u) if !u.trim().is_empty() => u,
        _ => {
            let u = prompt_line(&on_output, &mut rx, "login as: ", true).await?;
            if u.trim().is_empty() {
                return Err("沒有輸入帳號，連線取消。".to_string());
            }
            u.trim().to_string()
        }
    };
    // 舊版：帳號確定就把分頁標題改成 user@host（並送 `t` 同步 pane 標題）
    if let Some(cb) = &on_user {
        cb(&user);
    }

    authenticate(&mut handle, &user, &opts, &on_output, &mut rx).await?;

    // ---- 開 shell channel ----
    let channel = handle
        .channel_open_session()
        .await
        .map_err(|e| format!("開啟 session 失敗：{e}"))?;
    channel
        .request_pty(
            false,
            "xterm-256color",
            opts.cols as u32,
            opts.rows as u32,
            0,
            0,
            &[],
        )
        .await
        .map_err(|e| format!("請求 PTY 失敗：{e}"))?;
    channel
        .request_shell(false)
        .await
        .map_err(|e| format!("開啟 shell 失敗：{e}"))?;

    pump(channel, on_output, rx).await
}

/// 連上 shell 之後的主迴圈：本機輸入 → 遠端，遠端輸出 → 畫面。
async fn pump(
    mut channel: russh::Channel<client::Msg>,
    on_output: OnOutput,
    mut rx: mpsc::UnboundedReceiver<Cmd>,
) -> Result<Option<i32>, String> {
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
    rx: &mut mpsc::UnboundedReceiver<Cmd>,
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
        match load_key(path, opts.auth.key_passphrase.as_deref(), on_output, rx).await {
            Ok(key) => {
                let hash_alg = handle.best_supported_rsa_hash().await.ok().flatten().flatten();
                let with_hash = russh::keys::PrivateKeyWithHashAlg::new(Arc::new(key), hash_alg);
                match handle.authenticate_publickey(user, with_hash).await {
                    Ok(r) if r.success() => return Ok(()),
                    Ok(_) => echo(on_output, "\x1b[90m金鑰被拒絕，改用其他方式。\x1b[0m\r\n"),
                    Err(e) => echo(on_output, &format!("\x1b[90m金鑰驗證失敗（{e}）。\x1b[0m\r\n")),
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
                        answers.push(prompt_line(on_output, rx, &p.prompt, p.echo).await?);
                    }
                    resp = handle
                        .authenticate_keyboard_interactive_respond(answers)
                        .await
                        .map_err(|e| format!("驗證失敗：{e}"))?;
                }
            }
        },
        Err(e) => println!("[AwayTerminal] keyboard-interactive 不可用：{e}"),
    }

    // ---- 4. 密碼（PuTTY 的提示文字）----
    for attempt in 1..=3 {
        let prompt = format!("{}@{}'s password: ", user, opts.host);
        let pw = prompt_line(on_output, rx, &prompt, false).await?;
        match handle.authenticate_password(user, pw).await {
            Ok(r) if r.success() => return Ok(()),
            Ok(_) => {
                echo(on_output, "Access denied\r\n");
                if attempt == 3 {
                    return Err("密碼錯誤三次，連線結束。".to_string());
                }
            }
            Err(e) => return Err(format!("驗證失敗：{e}")),
        }
    }
    Err("驗證失敗。".to_string())
}

/// 讀一把私鑰。有密碼保護而使用者沒給時，在終端機裡問（不回顯）。
async fn load_key(
    path: &str,
    passphrase: Option<&str>,
    on_output: &OnOutput,
    rx: &mut mpsc::UnboundedReceiver<Cmd>,
) -> Result<ssh_key::PrivateKey, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("讀不到金鑰檔 {path}：{e}"))?;
    match russh::keys::decode_secret_key(&text, passphrase) {
        Ok(k) => Ok(k),
        Err(_) if passphrase.is_none() => {
            // OpenSSH 與 .ppk 的加密金鑰都會走到這裡
            let pw = prompt_line(on_output, rx, "Passphrase for key: ", false).await?;
            russh::keys::decode_secret_key(&text, Some(&pw))
                .map_err(|e| format!("金鑰解密失敗：{e}"))
        }
        Err(e) => Err(format!("金鑰讀取失敗：{e}")),
    }
}

/// 用 Pageant（Windows）／SSH agent 的金鑰驗證。回傳 `Ok(false)` ＝有 agent 但沒有一把能用。
///
/// `russh` 已經替 `AgentClient` 實作了 `Signer`（`src/auth.rs`），所以不必自己包一層。
async fn try_agent(handle: &mut client::Handle<Handler>, user: &str) -> Result<bool, String> {
    #[cfg(windows)]
    let mut agent = russh::keys::agent::client::AgentClient::connect_pageant()
        .await
        .map_err(|e| format!("找不到 Pageant：{e}"))?;
    #[cfg(not(windows))]
    let mut agent = {
        let path = std::env::var("SSH_AUTH_SOCK").map_err(|_| "沒有 SSH_AUTH_SOCK".to_string())?;
        let stream = tokio::net::UnixStream::connect(path)
            .await
            .map_err(|e| format!("連不上 ssh-agent：{e}"))?;
        russh::keys::agent::client::AgentClient::connect(stream)
    };

    let identities = agent
        .request_identities()
        .await
        .map_err(|e| format!("agent 沒有回應身分清單：{e}"))?;
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

/// 從輸入流讀一行。`echo = false` 時完全不回顯（密碼）。
async fn prompt_line(
    on_output: &OnOutput,
    rx: &mut mpsc::UnboundedReceiver<Cmd>,
    prompt: &str,
    echo_input: bool,
) -> Result<String, String> {
    echo(on_output, prompt);
    let mut buf = String::new();
    loop {
        let Some(cmd) = rx.recv().await else {
            return Err("連線已取消。".to_string());
        };
        let data = match cmd {
            Cmd::Write(d) => d,
            Cmd::Resize(..) => continue, // 問答期間也可能改變視窗大小
            Cmd::Close => return Err("連線已取消。".to_string()),
        };
        for ch in String::from_utf8_lossy(&data).chars() {
            match ch {
                '\r' | '\n' => {
                    echo(on_output, "\r\n");
                    return Ok(buf);
                }
                '\u{7f}' | '\u{8}' => {
                    if buf.pop().is_some() && echo_input {
                        echo(on_output, "\u{8} \u{8}"); // 同舊版的 "\b \b"
                    }
                }
                // Ctrl+C：取消整條連線（PuTTY 在登入階段按 Ctrl+C 也是斷線）
                '\u{3}' => return Err("已取消。".to_string()),
                c if c.is_control() => {}
                c => {
                    buf.push(c);
                    if echo_input {
                        echo(on_output, &c.to_string());
                    }
                }
            }
        }
    }
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
    fn decide(
        &self,
        _host: &str,
        _port: u16,
        verdict: &hostkey::Verdict,
        _fp: &hostkey::Fingerprints,
    ) -> HostKeyAnswer {
        self.seen
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(verdict.clone());
        self.answer
    }

    fn accept_weak(&self, _host: &str, _port: u16, _weak: &[(&'static str, String)]) -> bool {
        self.weak_asks
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.accept_weak
            .load(std::sync::atomic::Ordering::SeqCst)
    }
}
