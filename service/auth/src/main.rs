#[tokio::main]
async fn main() -> Result<(), xkk_auth::ServiceError> {
    let config = xkk_auth::Config::load(xkk_auth::config_path()?)?;
    xkk_auth::run(config).await
}
