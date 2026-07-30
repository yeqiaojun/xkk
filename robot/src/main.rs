use std::process::ExitCode;

use xkk_config::RobotConfig;

#[tokio::main]
async fn main() -> ExitCode {
    let config = match load_config() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("xkk-robot: {error}");
            return ExitCode::FAILURE;
        }
    };

    match xkk_robot::login(&config).await {
        Ok(result) => {
            println!(
                "Robot login succeeded: account={} gid={} gate={}://{}:{} session={} logic={} public={}",
                config.account,
                result.gid,
                result.transport.as_str(),
                result.host,
                result.port,
                result.session_id,
                result.logic_id,
                result.public_id,
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("xkk-robot: {error}");
            ExitCode::FAILURE
        }
    }
}

fn load_config() -> xkk_config::Result<RobotConfig> {
    RobotConfig::load(xkk_config::config_path("xkk-robot")?)
}
