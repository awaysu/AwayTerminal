//! SSH 後端的端到端驗證，**完全不靠網路、不連任何外部主機**。
//!
//! 做法：用 `russh` 自己的 server 端在同一支程式裡起一台測試 sshd（綁 127.0.0.1 的
//! 臨時埠），再用我們的 client（`awayterminal_lib::ssh`）連上去。驗的項目：
//!
//! 1. 密碼驗證（含 PuTTY 式的 `login as:` 與 `user@host's password:` 互動）
//! 2. publickey：**`.ppk`**（PuTTY 格式）與加密的 `.ppk`（密碼 `123`）
//! 3. 開 shell channel、拿到遠端輸出
//! 4. `resize` 真的送出 window-change
//! 5. 主機金鑰：第一次接受 → 存檔 → 第二次比對通過 → **故意換金鑰要被擋下**
//! 6. 關閉時送出 Ctrl+D ×3
//!
//! 用法：`cargo run --example ssh_probe`

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use awayterminal_lib::session::{OnExit, OnOutput, TerminalSession};
use awayterminal_lib::ssh::hostkey::{HostKeyStore, Verdict};
use awayterminal_lib::ssh::{self, FixedDecider, HostKeyAnswer, SshAuth, SshOptions};
use russh::keys::ssh_key;
use russh::server::{Auth, Msg, Server as _, Session};
use russh::keys::ssh_encoding::bytes::Bytes;
use russh::{Channel, ChannelId};

const USER: &str = "tester";
const PASSWORD: &str = "hunter2";
/// ssh-key 專案的測試向量（`tests/keys/`），加密那把的密碼是 `123`。
const PPK_PASSPHRASE: &str = "123";

fn main() {
    let mut pass = 0usize;
    let mut fail = 0usize;
    let mut report = |name: &str, ok: bool, detail: String| {
        if ok {
            pass += 1;
            println!("PASS  {name}：{detail}");
        } else {
            fail += 1;
            println!("FAIL  {name}：{detail}");
        }
    };

    println!("== AwayTerminal2 ssh_probe ==");

    // 測試伺服器要自己的 runtime（client 端的 runtime 在 ssh 模組裡）
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("建立 runtime");

    let dir = std::env::temp_dir().join(format!("awayterm-ssh-probe-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("建立暫存資料夾");
    let store = Arc::new(HostKeyStore::new(dir.join("known_hosts")));

    // ---------------------------------------------------------------- 伺服器
    let server_key =
        ssh_key::PrivateKey::random(&mut rand::rng(), ssh_key::Algorithm::Ed25519).unwrap();
    let server_pub = server_key.public_key().clone();
    let log = Arc::new(Mutex::new(ServerLog::default()));
    let port = rt
        .block_on(start_server(server_key, log.clone()))
        .expect("啟動測試 sshd");
    println!("測試 sshd：127.0.0.1:{port}（主機金鑰 {}）", ssh::hostkey::fingerprints(&server_pub).sha256);

    // ---------------------------------------------------- 1. 密碼 + login as:
    {
        let c = Client::connect(
            port,
            None,
            SshAuth::default(),
            store.clone(),
            HostKeyAnswer::AcceptAndStore,
        );
        // PuTTY 式互動：先 login as:，再密碼
        c.expect("login as: ", 5);
        c.session.write(format!("{USER}\r").as_bytes());
        let asked_password = c.expect("password: ", 5);
        c.session.write(format!("{PASSWORD}\r").as_bytes());
        let banner = c.expect("AWAY_SSH_OK", 10);
        report(
            "密碼驗證 + login as: 互動",
            asked_password && banner,
            format!(
                "login as: 有出現、密碼提示有出現、遠端 banner 收到（共 {} bytes）",
                c.output().len()
            ),
        );

        // 密碼不可以被回顯（PuTTY 打密碼時畫面完全不動）
        let text = c.text();
        report(
            "密碼不回顯",
            !text.contains(PASSWORD),
            "輸出裡找不到密碼字串".to_string(),
        );

        // 第一次連線應該是 Unknown（沒有記錄過）
        let seen = c.decider.seen.lock().unwrap().clone();
        report(
            "主機金鑰：第一次連線問使用者",
            seen == vec![Verdict::Unknown],
            format!("{seen:?}"),
        );

        // ---- resize → window-change ----
        c.session.resize(100, 40);
        let winch = c.expect("WINCH 100x40", 5);
        report("resize 送出 window-change", winch, "伺服器收到 100x40".to_string());

        // ---- 遠端執行指令拿輸出 ----
        c.session.write(b"echo hello\r");
        let echoed = c.expect("GOT:echo hello", 5);
        report("送指令並拿到輸出", echoed, "伺服器回報收到指令".to_string());

        // ---- 關閉送 Ctrl+D ×3 ----
        c.close();
        std::thread::sleep(Duration::from_millis(400));
        let got_eot = log.lock().unwrap().eot_bytes;
        report(
            "關閉送 Ctrl+D",
            got_eot >= 3,
            format!("伺服器收到 {got_eot} 個 0x04（優雅結束鍵 ×3）"),
        );
    }

    // ------------------------------------------- 2. 主機金鑰：第二次要直接通過
    {
        let c = Client::connect(
            port,
            Some(USER.to_string()),
            SshAuth::default(),
            store.clone(),
            // 這次故意給「拒絕」：如果程式問了使用者，連線就會失敗＝測出沒有用快取
            HostKeyAnswer::Reject,
        );
        c.expect("password: ", 5);
        c.session.write(format!("{PASSWORD}\r").as_bytes());
        let ok = c.expect("AWAY_SSH_OK", 10);
        let asked = c.decider.seen.lock().unwrap().len();
        report(
            "主機金鑰：已記錄過就不再問",
            ok && asked == 0,
            format!("連線成功={ok}、被問次數={asked}"),
        );
        c.close();
    }

    // ------------------------------------------- 3. 主機金鑰換掉 → 必須被擋下
    {
        // 在同一個 host:port 記一把**不同**的金鑰，模擬「伺服器金鑰被換掉」
        let other = ssh_key::PrivateKey::random(&mut rand::rng(), ssh_key::Algorithm::Ed25519)
            .unwrap()
            .public_key()
            .clone();
        let store2 = Arc::new(HostKeyStore::new(dir.join("known_hosts_changed")));
        store2.learn("127.0.0.1", port, &other).unwrap();

        let c = Client::connect(
            port,
            Some(USER.to_string()),
            SshAuth::default(),
            store2.clone(),
            // 使用者按「取消」——這是被警告之後該有的選擇
            HostKeyAnswer::Reject,
        );
        let exited = c.wait_exit(10);
        let seen = c.decider.seen.lock().unwrap().clone();
        let was_changed = matches!(seen.first(), Some(Verdict::Changed { .. }));
        let no_shell = !c.text().contains("AWAY_SSH_OK");
        report(
            "主機金鑰換掉 → 被擋下",
            exited && was_changed && no_shell,
            format!("判定={seen:?}、連線已結束={exited}、沒有進到 shell={no_shell}"),
        );

        // 同樣的情況若使用者選「只這次」則應該連得上（但不寫檔）
        let before = std::fs::read_to_string(store2.path()).unwrap_or_default();
        let c2 = Client::connect(
            port,
            Some(USER.to_string()),
            SshAuth::default(),
            store2.clone(),
            HostKeyAnswer::AcceptOnce,
        );
        c2.expect("password: ", 5);
        c2.session.write(format!("{PASSWORD}\r").as_bytes());
        let ok = c2.expect("AWAY_SSH_OK", 10);
        let after = std::fs::read_to_string(store2.path()).unwrap_or_default();
        report(
            "主機金鑰：只這次＝連得上但不寫檔",
            ok && before == after,
            format!("連線成功={ok}、known_hosts 沒有變動={}", before == after),
        );
        c2.close();
    }

    // ----------------------------------------------------- 4. publickey / .ppk
    for (name, file, passphrase) in [
        (".ppk（未加密）", "id_ed25519.ppk", None),
        (".ppk（加密，密碼 123）", "id_ed25519_enc.ppk", Some(PPK_PASSPHRASE)),
    ] {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("keys")
            .join(file);
        if !path.exists() {
            report(name, false, format!("找不到測試金鑰 {}", path.display()));
            continue;
        }
        let auth = SshAuth {
            key_path: Some(path.to_string_lossy().to_string()),
            key_passphrase: passphrase.map(|s| s.to_string()),
            use_agent: false,
        };
        let c = Client::connect(
            port,
            Some(USER.to_string()),
            auth,
            store.clone(),
            HostKeyAnswer::AcceptAndStore,
        );
        let ok = c.expect("AWAY_SSH_OK", 10);
        let used_key = log.lock().unwrap().publickey_users.contains(USER);
        report(
            name,
            ok && used_key,
            format!("連線成功={ok}、伺服器確認走的是 publickey={used_key}"),
        );
        c.close();
        log.lock().unwrap().publickey_users.clear();
    }

    // --------------------------------------------- 5. 真的 sshd（有就跑，沒有就跳過）
    println!("SKIP  真連線測試：需要本機 OpenSSH sshd 與一組帳號密碼，見 docs/SSH.md「待真機驗證」");

    println!();
    println!("known_hosts：{}", store.path().display());
    if let Ok(text) = std::fs::read_to_string(store.path()) {
        for line in text.lines() {
            // 只印前 40 個字元，金鑰本體不必完整入 log
            println!("  | {}…", line.chars().take(40).collect::<String>());
        }
    }
    let _ = std::fs::remove_dir_all(&dir);

    println!();
    println!("RESULT: {pass} PASS / {fail} FAIL");
    if fail > 0 {
        std::process::exit(1);
    }
}

// ------------------------------------------------------------------ client 包裝

struct Client {
    session: Arc<ssh::SshSession>,
    out: Arc<Mutex<Vec<u8>>>,
    exited: Arc<AtomicBool>,
    decider: Arc<FixedDecider>,
}

impl Client {
    fn connect(
        port: u16,
        user: Option<String>,
        auth: SshAuth,
        store: Arc<HostKeyStore>,
        answer: HostKeyAnswer,
    ) -> Self {
        let out = Arc::new(Mutex::new(Vec::new()));
        let exited = Arc::new(AtomicBool::new(false));
        let decider = FixedDecider::new(answer);

        let on_output: OnOutput = {
            let out = out.clone();
            Arc::new(move |bytes: &[u8]| {
                out.lock().unwrap().extend_from_slice(bytes);
            })
        };
        let on_exit: OnExit = {
            let exited = exited.clone();
            Arc::new(move |_| exited.store(true, Ordering::SeqCst))
        };

        let session = ssh::spawn(
            SshOptions {
                host: "127.0.0.1".to_string(),
                port,
                user,
                cols: 80,
                rows: 24,
                auth,
            },
            store,
            decider.clone(),
            on_output,
            on_exit,
            None, // 分頁標題的回報是 app 端的事，probe 不需要
        );

        Self {
            session,
            out,
            exited,
            decider,
        }
    }

    fn output(&self) -> Vec<u8> {
        self.out.lock().unwrap().clone()
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.output()).to_string()
    }

    /// 等到輸出裡出現 `needle`（最多 `secs` 秒）。
    fn expect(&self, needle: &str, secs: u64) -> bool {
        let deadline = Instant::now() + Duration::from_secs(secs);
        while Instant::now() < deadline {
            if self.text().contains(needle) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        println!("      （等不到 {needle:?}，目前輸出：{:?}）", tail(&self.text(), 160));
        false
    }

    fn wait_exit(&self, secs: u64) -> bool {
        let deadline = Instant::now() + Duration::from_secs(secs);
        while Instant::now() < deadline {
            if self.exited.load(Ordering::SeqCst) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    }

    fn close(&self) {
        TerminalSession::close(&*self.session);
    }
}

fn tail(s: &str, n: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= n {
        return s.to_string();
    }
    format!("…{}", chars[chars.len() - n..].iter().collect::<String>())
}

// ------------------------------------------------------------------ 測試 sshd

#[derive(Default)]
struct ServerLog {
    /// 收到幾個 0x04（Ctrl+D）——驗「關閉送優雅結束鍵」。
    eot_bytes: usize,
    /// 哪些帳號成功走了 publickey。
    publickey_users: HashSet<String>,
}

async fn start_server(key: ssh_key::PrivateKey, log: Arc<Mutex<ServerLog>>) -> std::io::Result<u16> {
    let config = Arc::new(russh::server::Config {
        inactivity_timeout: Some(Duration::from_secs(120)),
        auth_rejection_time: Duration::from_millis(50),
        auth_rejection_time_initial: Some(Duration::from_millis(0)),
        keys: vec![key],
        ..Default::default()
    });

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
    let port = listener.local_addr()?.port();

    let mut server = TestServer { log };
    tokio::spawn(async move {
        if let Err(e) = server.run_on_socket(config, &listener).await {
            eprintln!("測試 sshd 結束：{e}");
        }
    });
    Ok(port)
}

#[derive(Clone)]
struct TestServer {
    log: Arc<Mutex<ServerLog>>,
}

impl russh::server::Server for TestServer {
    type Handler = TestHandler;
    fn new_client(&mut self, _: Option<std::net::SocketAddr>) -> TestHandler {
        TestHandler {
            log: self.log.clone(),
        }
    }
}

struct TestHandler {
    log: Arc<Mutex<ServerLog>>,
}

impl russh::server::Handler for TestHandler {
    type Error = russh::Error;

    async fn auth_password(&mut self, user: &str, password: &str) -> Result<Auth, Self::Error> {
        if user == USER && password == PASSWORD {
            Ok(Auth::Accept)
        } else {
            Ok(Auth::reject())
        }
    }

    async fn auth_publickey(
        &mut self,
        user: &str,
        _key: &ssh_key::PublicKey,
    ) -> Result<Auth, Self::Error> {
        // 測試伺服器接受任何金鑰：這裡要驗的是「我們的 client 讀得懂 .ppk 並簽得出來」
        self.log
            .lock()
            .unwrap()
            .publickey_users
            .insert(user.to_string());
        Ok(Auth::Accept)
    }

    async fn channel_open_session(
        &mut self,
        _channel: Channel<Msg>,
        reply: russh::server::ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        reply.accept().await;
        Ok(())
    }

    async fn pty_request(
        &mut self,
        _channel: ChannelId,
        _term: &str,
        _cols: u32,
        _rows: u32,
        _pw: u32,
        _ph: u32,
        _modes: &[(russh::Pty, u32)],
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        Ok(())
    }

    async fn shell_request(
        &mut self,
        channel: ChannelId,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.data(channel, Bytes::from_static(b"AWAY_SSH_OK\r\n$ "))?;
        Ok(())
    }

    async fn window_change_request(
        &mut self,
        channel: ChannelId,
        cols: u32,
        rows: u32,
        _pw: u32,
        _ph: u32,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.data(channel, Bytes::from(format!("WINCH {cols}x{rows}\r\n")))?;
        Ok(())
    }

    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        {
            let mut log = self.log.lock().unwrap();
            log.eot_bytes += data.iter().filter(|&&b| b == 0x04).count();
        }
        // 回報收到什麼（去掉換行），讓 client 端可以斷言
        let text = String::from_utf8_lossy(data).replace(['\r', '\n'], "");
        if !text.is_empty() {
            session.data(channel, Bytes::from(format!("GOT:{text}\r\n")))?;
        }
        Ok(())
    }
}
