#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Enable backtrace for crash diagnostics in dev builds
    #[cfg(debug_assertions)]
    if std::env::var("RUST_BACKTRACE").is_err() {
        std::env::set_var("RUST_BACKTRACE", "1");
    }

    // Custom panic hook to log before potential abort
    std::panic::set_hook(Box::new(|info| {
        eprintln!("Clyde: PANIC: {info}");
        if let Some(bt) = std::backtrace::Backtrace::force_capture()
            .to_string()
            .lines()
            .take(20)
            .collect::<Vec<_>>()
            .first()
            .copied()
        {
            let _ = bt; // backtrace already printed by default handler
        }
        eprintln!("{}", std::backtrace::Backtrace::force_capture());
    }));

    clyde_lib::run();
}
