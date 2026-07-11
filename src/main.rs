mod compiler;
mod config;
mod resolver;
mod templates;

use anyhow::Result;

fn main() -> Result<()> {
    let config = config::Config::load(std::path::Path::new("config.toml"))?;
    config.check_templates_dir()?;
    println!("{config:?}");
    Ok(())
}
