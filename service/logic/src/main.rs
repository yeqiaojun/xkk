use xkk_logic::{Config, ServiceError, config_path, run};

#[tokio::main]
async fn main() -> Result<(), ServiceError> {
    let config = Config::load(config_path()?)?;
    run(config).await
}
