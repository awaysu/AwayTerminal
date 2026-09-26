// Windows release build 不要開額外的主控台視窗
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    awayterminal_lib::run()
}
