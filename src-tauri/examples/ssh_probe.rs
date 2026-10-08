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

    println!("== AwayTerminal ssh_probe ==");

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

        // 稽核 E2：紅框按「接受並儲存」之後，下一次（例如自動重連）就不可以再問
        let c3 = Client::connect(
            port,
            Some(USER.to_string()),
            SshAuth::default(),
            store2.clone(),
            HostKeyAnswer::AcceptAndStore,
        );
        c3.expect("password: ", 5);
        c3.session.write(format!("{PASSWORD}\r").as_bytes());
        let ok3 = c3.expect("AWAY_SSH_OK", 10);
        c3.close();
        let c4 = Client::connect(
            port,
            Some(USER.to_string()),
            SshAuth::default(),
            store2.clone(),
            // 真的又問了就會被拒絕而連不上
            HostKeyAnswer::Reject,
        );
        c4.expect("password: ", 5);
        c4.session.write(format!("{PASSWORD}\r").as_bytes());
        let ok4 = c4.expect("AWAY_SSH_OK", 10);
        let asked4 = c4.decider.seen.lock().unwrap().len();
        let lines = std::fs::read_to_string(store2.path())
            .unwrap_or_default()
            .lines()
            .filter(|l| !l.trim().is_empty())
            .count();
        report(
            "金鑰變更 → 接受並儲存之後不再問",
            ok3 && ok4 && asked4 == 0 && lines == 1,
            format!("接受後連上={ok3}、下一次連上={ok4}、被問={asked4}（要 0）、known_hosts 行數={lines}（要 1）"),
        );
        c4.close();
    }

    // ------------------------------- 3b. 一次貼上帳號＋密碼（稽核 E7：第一版只拿到帳號）
    {
        let c = Client::connect(port, None, SshAuth::default(), store.clone(), HostKeyAnswer::AcceptAndStore);
        c.expect("login as: ", 5);
        c.session.write(format!("{USER}\n{PASSWORD}\n").as_bytes());
        let ok = c.expect("AWAY_SSH_OK", 10);
        report(
            "一次貼上 帳號\\n密碼\\n",
            ok && !c.text().contains(PASSWORD),
            format!("登入成功={ok}（密碼仍不回顯）"),
        );
        c.close();
    }

    // ------------------------ 3c. 連線中關分頁要立刻收掉（稽核 E15：第一版等 TCP 逾時）
    {
        // 只 accept、永遠不說話的伺服器（交握卡在等 SSH 版本字串）
        let (silent_port, _keep) = rt.block_on(async {
            let l = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
            let p = l.local_addr().unwrap().port();
            let h = tokio::spawn(async move {
                let mut held = Vec::new();
                while let Ok((s, _)) = l.accept().await {
                    held.push(s);
                }
            });
            (p, h)
        });
        let c = Client::connect(silent_port, Some(USER.to_string()), SshAuth::default(), store.clone(), HostKeyAnswer::Reject);
        std::thread::sleep(Duration::from_millis(300));
        let t0 = Instant::now();
        c.close();
        let exited = c.wait_exit(5);
        let took = t0.elapsed();
        report(
            "連線中關分頁立刻取消",
            exited && took < Duration::from_secs(2),
            format!("結束事件={exited}、花了 {} ms", took.as_millis()),
        );
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
            password: None,
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

    // ------------------------------------ 5. 舊演算法伺服器（風險 3 / B4 的核心）
    {
        // 伺服器**只**接受 group14-sha1 + aes128-cbc + hmac-sha1 + ssh-rsa。
        // 用 russh 預設清單的 client 會在交握就失敗；我們的 PuTTY 式清單要連得上。
        let legacy_key =
            ssh_key::PrivateKey::random(&mut rand::rng(), ssh_key::Algorithm::Rsa { hash: None })
                .unwrap();
        let legacy_log = Arc::new(Mutex::new(ServerLog::default()));
        match rt.block_on(start_server_with(legacy_key, legacy_log.clone(), true)) {
            Ok((lport, _handle)) => {
                println!("舊演算法 sshd：127.0.0.1:{lport}（group14-sha1 / aes128-cbc / hmac-sha1 / ssh-rsa）");
                let store3 = Arc::new(HostKeyStore::new(dir.join("known_hosts_legacy")));
                let c = Client::connect(
                    lport,
                    Some(USER.to_string()),
                    SshAuth::default(),
                    store3.clone(),
                    HostKeyAnswer::AcceptAndStore,
                );
                c.expect("password: ", 10);
                c.session.write(format!("{PASSWORD}\r").as_bytes());
                let ok = c.expect("AWAY_SSH_OK", 15);
                report(
                    "只支援舊演算法的伺服器也連得上",
                    ok,
                    "kex=group14-sha1 cipher=aes128-cbc mac=hmac-sha1 hostkey=ssh-rsa".to_string(),
                );
                // 弱演算法警告要被問到（PuTTY 的 warn-below-this-line）
                report(
                    "弱演算法警告有跳出來",
                    c.decider.weak_asks() >= 1,
                    format!("被問 {} 次", c.decider.weak_asks()),
                );
                c.close();

                // 接受過之後：app 端是記在 settings 裡不再問。probe 的 decider 沒有那份記錄，
                // 所以這裡驗的是「第二次連線仍然連得上」；不再問那半由單元測試與 app 端負責。
                let c2 = Client::connect(
                    lport,
                    Some(USER.to_string()),
                    SshAuth::default(),
                    store3.clone(),
                    HostKeyAnswer::Reject, // 主機金鑰已記錄 → 不該再問
                );
                c2.expect("password: ", 10);
                c2.session.write(format!("{PASSWORD}\r").as_bytes());
                let ok2 = c2.expect("AWAY_SSH_OK", 15);
                report(
                    "舊演算法伺服器：第二次連線（主機金鑰已記錄）",
                    ok2,
                    format!("主機金鑰被問 {} 次（要是 0）", c2.decider.seen.lock().unwrap().len()),
                );
                c2.close();

                // 使用者按「取消」→ 不可以連上
                let c3 = Client::connect(
                    lport,
                    Some(USER.to_string()),
                    SshAuth::default(),
                    store3.clone(),
                    HostKeyAnswer::AcceptAndStore,
                );
                c3.decider.set_accept_weak(false);
                let exited = c3.wait_exit(15);
                report(
                    "弱演算法警告按取消 → 連線中止",
                    exited && !c3.text().contains("AWAY_SSH_OK"),
                    format!("已結束={exited}、沒進到 shell={}", !c3.text().contains("AWAY_SSH_OK")),
                );
                c3.close();
            }
            Err(e) => report("啟動舊演算法 sshd", false, e.to_string()),
        }
    }

    // ------------------------------------------------ 6. 伺服器斷線 → client 收尾
    {
        // 起一台可以主動關掉的伺服器，驗「伺服器不見了，client 會結束而不是掛住」。
        // 自動重連本身是分頁層的行為（`ssh/reconnect`），這裡驗的是 session 層的收尾。
        let key2 =
            ssh_key::PrivateKey::random(&mut rand::rng(), ssh_key::Algorithm::Ed25519).unwrap();
        let log2 = Arc::new(Mutex::new(ServerLog::default()));
        match rt.block_on(start_server_with(key2, log2, false)) {
            Ok((p2, handle)) => {
                let store4 = Arc::new(HostKeyStore::new(dir.join("known_hosts_drop")));
                let c = Client::connect(
                    p2,
                    Some(USER.to_string()),
                    SshAuth::default(),
                    store4,
                    HostKeyAnswer::AcceptAndStore,
                );
                c.expect("password: ", 10);
                c.session.write(format!("{PASSWORD}\r").as_bytes());
                let up = c.expect("AWAY_SSH_OK", 15);
                // 伺服器主動關掉
                rt.block_on(async { handle.shutdown("probe shutdown".into()) });
                let exited = c.wait_exit(15);
                report(
                    "伺服器斷線 → session 正常結束（不會掛住）",
                    up && exited,
                    format!("連上過={up}、結束事件有來={exited}"),
                );
                c.close();
            }
            Err(e) => report("啟動可斷線 sshd", false, e.to_string()),
        }
    }

    // ------------------------ 7. 伺服器在同一埠再起來 → 連回去（重連依賴的語意）
    {
        // 分頁層的自動重連（退避／世代／按 Enter）在 `ssh/reconnect.rs`，需要 Tauri 的
        // state 才跑得起來，所以那一層由單元測試 + app 的 `--verify` 驗。
        // 這裡驗的是重連**依賴的 session 層語意**：同一個 host:port 的伺服器重新起來之後，
        // 新 session 連得上、而且**主機金鑰是同一把所以不會再問**。
        let key3 =
            ssh_key::PrivateKey::random(&mut rand::rng(), ssh_key::Algorithm::Ed25519).unwrap();
        let log3 = Arc::new(Mutex::new(ServerLog::default()));
        let store5 = Arc::new(HostKeyStore::new(dir.join("known_hosts_restart")));

        // 固定一個埠：先問系統要一個空的，關掉之後同一個埠再起第二台
        let fixed_port = rt.block_on(async {
            let l = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
            l.local_addr().unwrap().port()
        });

        match rt.block_on(start_server_on(fixed_port, key3.clone(), log3.clone())) {
            Ok(handle) => {
                let c = Client::connect(
                    fixed_port,
                    Some(USER.to_string()),
                    SshAuth::default(),
                    store5.clone(),
                    HostKeyAnswer::AcceptAndStore,
                );
                c.expect("password: ", 10);
                c.session.write(format!("{PASSWORD}\r").as_bytes());
                let up = c.expect("AWAY_SSH_OK", 15);
                rt.block_on(async { handle.shutdown("restart test".into()) });
                let down = c.wait_exit(15);
                c.close();

                // 同一埠再起一台（**同一把主機金鑰**，模擬設備重開機）
                let mut restarted = None;
                for _ in 0..20 {
                    std::thread::sleep(Duration::from_millis(250));
                    if let Ok(h) = rt.block_on(start_server_on(fixed_port, key3.clone(), log3.clone()))
                    {
                        restarted = Some(h);
                        break;
                    }
                }
                if restarted.is_none() {
                    report("同一埠重新起 sshd", false, "埠一直被占著".to_string());
                } else {
                    let c2 = Client::connect(
                        fixed_port,
                        Some(USER.to_string()),
                        SshAuth::default(),
                        store5.clone(),
                        // 已記錄過 → 不該再問；真問了就會被拒絕而連不上
                        HostKeyAnswer::Reject,
                    );
                    c2.expect("password: ", 10);
                    c2.session.write(format!("{PASSWORD}\r").as_bytes());
                    let again = c2.expect("AWAY_SSH_OK", 15);
                    let asked = c2.decider.seen.lock().unwrap().len();
                    report(
                        "伺服器重開後連回去（沿用已接受的主機金鑰）",
                        up && down && again && asked == 0,
                        format!(
                            "第一次連上={up}、斷線事件={down}、重連成功={again}、主機金鑰被問={asked}（要 0）"
                        ),
                    );
                    c2.close();
                }
            }
            Err(e) => report("固定埠起 sshd", false, e.to_string()),
        }
    }

    // --------------------------------------------- 8. 真的 sshd（有就跑，沒有就跳過）
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
                algos: Default::default(),
                keepalive_mins: 0,
                env: Vec::new(),
            },
            store,
            decider.clone(),
            on_output,
            on_exit,
            None, // 分頁標題的回報是 app 端的事，probe 不需要
            None, // 退避歸零也是 app 端的事
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
    start_server_with(key, log, false).await.map(|(port, _)| port)
}

/// 在**指定的埠**起一台一般設定的測試 sshd（驗「伺服器重開後連回去」用）。
/// 埠還被占著時回 Err，呼叫端可以重試。
async fn start_server_on(
    port: u16,
    key: ssh_key::PrivateKey,
    log: Arc<Mutex<ServerLog>>,
) -> std::io::Result<russh::server::RunningServerHandle> {
    let config = Arc::new(russh::server::Config {
        inactivity_timeout: Some(Duration::from_secs(120)),
        auth_rejection_time: Duration::from_millis(50),
        auth_rejection_time_initial: Some(Duration::from_millis(0)),
        keys: vec![key],
        ..Default::default()
    });
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let mut server = TestServer { log };
        let running = server.run_on_socket(config, &listener);
        let _ = tx.send(running.handle());
        let _ = running.await;
    });
    rx.await
        .map_err(|_| std::io::Error::other("測試 sshd 沒有回報 handle"))
}

/// `legacy_only = true` 時，伺服器**只**接受舊演算法（`group14-sha1` + `aes128-cbc`
/// + `hmac-sha1` + `ssh-rsa`），用來驗「我們的 client 連得上舊設備」與弱演算法警告。
///
/// 回傳 (埠, handle)——handle 可以用來主動關掉伺服器（驗斷線重連用）。
async fn start_server_with(
    key: ssh_key::PrivateKey,
    log: Arc<Mutex<ServerLog>>,
    legacy_only: bool,
) -> std::io::Result<(u16, russh::server::RunningServerHandle)> {
    let preferred = if legacy_only {
        russh::Preferred {
            kex: std::borrow::Cow::Owned(vec![russh::kex::DH_G14_SHA1]),
            cipher: std::borrow::Cow::Owned(vec![russh::cipher::AES_128_CBC]),
            mac: std::borrow::Cow::Owned(vec![russh::mac::HMAC_SHA1]),
            key: std::borrow::Cow::Owned(vec![russh::keys::Algorithm::Rsa { hash: None }]),
            ..russh::Preferred::DEFAULT
        }
    } else {
        russh::Preferred::DEFAULT
    };
    let config = Arc::new(russh::server::Config {
        inactivity_timeout: Some(Duration::from_secs(120)),
        auth_rejection_time: Duration::from_millis(50),
        auth_rejection_time_initial: Some(Duration::from_millis(0)),
        keys: vec![key],
        preferred,
        ..Default::default()
    });

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
    let port = listener.local_addr()?.port();

    // `run_on_socket` 借用 server 與 listener，所以兩個都要搬進 task 裡；
    // handle 再用 oneshot 傳回來。
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let mut server = TestServer { log };
        let running = server.run_on_socket(config, &listener);
        let _ = tx.send(running.handle());
        if let Err(e) = running.await {
            eprintln!("測試 sshd 結束：{e}");
        }
    });
    let handle = rx
        .await
        .map_err(|_| std::io::Error::other("測試 sshd 沒有回報 handle"))?;
    Ok((port, handle))
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
