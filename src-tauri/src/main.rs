// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use muiget_lib::extension_bridge::native_host;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // Chrome köprüyü, manifestteki komuta uzantı kimliğini argüman olarak
    // geçirerek başlatıyor (manifestin özel argüman alanı yok). O durumda
    // pencere açılmıyor: süreç yalnızca stdio köprüsünü işletip çıkıyor.
    if native_host::is_host_invocation(&args) {
        if let Err(e) = native_host::run_host() {
            // Chrome stderr'i kendi log'una yazıyor; tanılama için tek kanal bu.
            eprintln!("muiget native host hatası: {e}");
            std::process::exit(1);
        }
        return;
    }

    muiget_lib::run();
}
