// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use muiget_lib::extension_bridge::native_host;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // Chrome, native messaging host'unu bu bayrakla başlatıyor. O durumda
    // pencere açılmıyor: süreç yalnızca stdio köprüsünü işletip çıkıyor.
    if args.iter().any(|a| a == native_host::HOST_FLAG) {
        if let Err(e) = native_host::run_host() {
            // Chrome stderr'i kendi log'una yazıyor; tanılama için tek kanal bu.
            eprintln!("muiget native host hatası: {e}");
            std::process::exit(1);
        }
        return;
    }

    muiget_lib::run();
}
