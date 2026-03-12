use anyhow::Result;
use astra_config::AppConfig;
use astra_runtime::PipelineRunner;
use tracing::info;

fn main() -> Result<()> {
    let config_path = std::env::args().nth(1);
    let config = config_path
        .as_deref()
        .map(AppConfig::load_from_path)
        .transpose()?
        .unwrap_or_default();

    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .or_else(|_| tracing_subscriber::EnvFilter::try_new(config.app.log_level.clone()))
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    tracing_subscriber::fmt().with_env_filter(env_filter).init();

    let runner = PipelineRunner::new(config);
    let selection = runner.selection();
    let report = runner.start()?;

    info!(
        config_path = ?config_path,
        mode = selection.mode.as_str(),
        serial_backend = selection.serial.as_str(),
        camera_source = selection.camera.as_str(),
        detector_backend = selection.detector.as_str(),
        ?report,
        "astra-app startup completed"
    );
    Ok(())
}
