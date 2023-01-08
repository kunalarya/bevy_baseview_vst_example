use std::fs::File;
use std::ops::Deref;
use std::panic;
use std::path::Path;

use log::error;
use simplelog::{LevelFilter, WriteLogger};

// https://stackoverflow.com/a/42457596
pub fn setup_panic_handling() {
    log::info!("Setting up panic handler");
    panic::set_hook(Box::new(|panic_info| {
        log::info!("A panic occurred!");
        let (filename, line) = panic_info
            .location()
            .map(|loc| (loc.file(), loc.line()))
            .unwrap_or(("<unknown>", 0));

        let cause = panic_info
            .payload()
            .downcast_ref::<String>()
            .map(String::deref);

        let cause = cause.unwrap_or_else(|| {
            panic_info
                .payload()
                .downcast_ref::<&str>()
                .copied()
                .unwrap_or("<cause unknown>")
        });

        error!("A panic occurred at {}:{}: {}", filename, line, cause);
    }));
}

pub fn setup_tmp_log() {
    let log_file = Path::new("/tmp/").join("baseview_demo.log");
    let f = File::create(&log_file);
    if let Ok(file) = f {
        let config = simplelog::ConfigBuilder::new()
            .add_filter_ignore("wgpu".to_string())
            .build();
        // Ignore result.
        let _ = WriteLogger::init(LevelFilter::Info, config, file);
    }
}
