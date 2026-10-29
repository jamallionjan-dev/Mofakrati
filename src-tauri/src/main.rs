// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Some(code) = app_lib::run_headless_reminder_from_args() {
        std::process::exit(code);
    }
    app_lib::run();
}
