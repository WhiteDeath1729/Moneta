// Prevents additional console window on Windows in release
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

fn main() {
    std::panic::set_hook(Box::new(|info| {
        let backtrace = std::backtrace::Backtrace::capture();
        let msg = format!("PANIC: {}\nBacktrace:\n{:?}\n", info, backtrace);
        eprintln!("{}", msg);
        let _ = std::fs::write("moneta_crash.log", msg);
    }));
    moneta_app_lib::run();
}
