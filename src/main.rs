mod config;
use std::error::Error;

#[tokio::main]
async fn main() {
    // start asynchronous runtime
    if let Err(error) = launcher().await {
        eprintln!("Startup failed: {error}");
        #[cfg(windows)]
        {
            // pause when Windows fails to start
            // making it easier for users who double-click to start to view errors
            use std::io::BufRead;
            eprintln!("Press the enter key to exit.");
            let mut line = String::new();
            let _ = std::io::stdin().lock().read_line(&mut line);
        }
        std::process::exit(1);
    }
}

/// main task: initialize configuration, start TCP/UDP services
async fn launcher() -> Result<(), Box<dyn Error>> {
    let config = config::initial_config()?;
    println!("Service is Running, Press Ctrl+C exit.");
    // wait exit signal
    wait_for_shutdown_signal().await;
    // stop all service.
    println!("Service stoping...");
    println!("Service was stopped.");
    Ok(())
}

/// waiting for Ctrl+C
async fn wait_for_shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        if let Ok(mut terminate) = signal(SignalKind::terminate()) {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = terminate.recv() => {}
            }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}
