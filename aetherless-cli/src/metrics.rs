use lazy_static::lazy_static;
use prometheus::{
    register_histogram_vec, register_int_counter_vec, register_int_gauge_vec, HistogramVec,
    IntCounterVec, IntGaugeVec,
};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;

lazy_static! {
    pub static ref FUNCTION_RESTORES: IntCounterVec = register_int_counter_vec!(
        "function_restores_total",
        "Total number of CRIU restores performed",
        &["function_id"]
    )
    .unwrap();
    pub static ref RESTORE_DURATION: HistogramVec = register_histogram_vec!(
        "function_restore_duration_seconds",
        "Time taken to restore a function from snapshot",
        &["function_id"],
        vec![0.001, 0.002, 0.005, 0.010, 0.015, 0.020, 0.050, 0.100] // Buckets focused on 15ms target
    )
    .unwrap();
    pub static ref WARM_POOL_SIZE: IntGaugeVec = register_int_gauge_vec!(
        "warm_pool_size",
        "Number of available warm snapshots",
        &["function_id"]
    )
    .unwrap();
    pub static ref COLD_STARTS: IntCounterVec = register_int_counter_vec!(
        "function_cold_starts_total",
        "Total number of full cold starts (no snapshot)",
        &["function_id"]
    )
    .unwrap();
}

/// Start the metrics server in a background task.
pub fn start_metrics_server(port: u16) {
    // Force initialization of metrics
    lazy_static::initialize(&FUNCTION_RESTORES);
    lazy_static::initialize(&RESTORE_DURATION);
    lazy_static::initialize(&WARM_POOL_SIZE);
    lazy_static::initialize(&COLD_STARTS);

    tokio::spawn(async move {
        let addr = format!("0.0.0.0:{}", port);
        match TcpListener::bind(&addr).await {
            Ok(listener) => {
                tracing::info!("Metrics server starting on {}", addr);
                loop {
                    if let Ok((mut socket, _)) = listener.accept().await {
                        tokio::spawn(async move {
                            let body = metrics_handler();
                            let response = format!(
                                "HTTP/1.0 200 OK\r\nConnection: close\r\nContent-Length: {}\r\nContent-Type: text/plain\r\n\r\n{}",
                                body.len(),
                                body
                            );
                            let _ = socket.write_all(response.as_bytes()).await;
                            let _ = socket.flush().await;
                        });
                    }
                }
            }
            Err(e) => {
                tracing::error!("Failed to bind metrics server: {}", e);
            }
        }
    });
}

fn metrics_handler() -> String {
    use prometheus::Encoder;
    let encoder = prometheus::TextEncoder::new();

    let mut buffer = Vec::new();
    if let Err(e) = encoder.encode(&prometheus::gather(), &mut buffer) {
        tracing::error!("Failed to encode metrics: {}", e);
    }

    String::from_utf8(buffer).unwrap_or_else(|_| String::from("Encoding error"))
}
