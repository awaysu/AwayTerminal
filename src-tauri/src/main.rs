// Windows release build 不要開額外的主控台視窗
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Claude Code 的狀態列指令（`quota.rs`）：每次更新狀態列都會跑一次，
    // 一定要在建立視窗、單一執行個體檢查這些之前就分流出去
    if std::env::args().nth(1).as_deref() == Some(awayterminal_lib::quota::STATUSLINE_ARG) {
        std::process::exit(awayterminal_lib::quota::run_statusline());
    }
    awayterminal_lib::run()
}
