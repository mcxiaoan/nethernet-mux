mod config;
use std::error::Error;
use std::net::SocketAddr;
// use std::sync::Arc;
use tokio::net::{TcpListener, UdpSocket};
use tokio::task::JoinHandle;

// stores all running service tasks.
struct RunningServices {
    tasks: Vec<JoinHandle<()>>,
}

impl RunningServices {
    // stop all service tasks.
    async fn stop(self) {
        for task in &self.tasks {
            task.abort();
        }
        for task in self.tasks {
            let _ = task.await;
        }
    }
}

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
    // get all config.
    let configs = config::initial_config()?;
    // bind all TCP and UDP sockets.
    let bound = bind_services(configs).await?;
    // start all services.
    let services = start_services(bound);
    // print running state.
    println!("Service is Running, Press Ctrl+C exit.");
    // wait exit signal
    wait_for_shutdown_signal().await;
    // stop all service.
    println!("Service stoping...");
    services.stop().await;
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

/// bind TCP and UDP sockets for all BDS instances.
async fn bind_services(
    configs: Vec<config::Config>,
) -> Result<Vec<(config::Config, TcpListener, UdpSocket)>, Box<dyn Error>> {
    // bind all sockets before starting any service tasks.
    let mut bound: Vec<(config::Config, TcpListener, UdpSocket)> =
        Vec::with_capacity(configs.len());
    for config in configs {
        let tcp_addr = SocketAddr::from(([0, 0, 0, 0], config.listen_tcp));
        let udp_addr = SocketAddr::from(([0, 0, 0, 0], config.listen_mux));
        let tcp = TcpListener::bind(tcp_addr).await?;
        let udp = UdpSocket::bind(udp_addr).await?;
        println!(
            "BDS {}:{} | TCP: {} | UDP: {}",
            config.bds_host, config.bds_tcp, tcp_addr, udp_addr
        );
        bound.push((config, tcp, udp));
    }
    Ok(bound)
}

/// start TCP signaling and UDP mux services.
fn start_services(bound: Vec<(config::Config, TcpListener, UdpSocket)>) -> RunningServices {
    let mut tasks: Vec<JoinHandle<()>> = Vec::with_capacity(bound.len() * 2);
    for (config, tcp, udp) in bound {
        // create an independent session table for each BDS instance.
        // let sessions = Arc::new(SessionTable::default());

        // TCP and UDP services share this BDS instance's session table.
        // let http_sessions = Arc::clone(&sessions);

        let http_config = config.clone();

        // start TCP signaling service.
        tasks.push(tokio::spawn(async move {
            // if let Err(error) = signaling::serve(tcp, http_config, http_sessions).await {
            //     eprintln!("TCP signaling service exited: {error}");
            // }
        }));

        // start UDP mux service.
        // tasks.push(tokio::spawn(async move {
        // mux::serve(udp, config, sessions).await;
        // }));
    }
    RunningServices { tasks }
}
