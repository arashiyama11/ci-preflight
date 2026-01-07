use color_eyre::{Result, eyre::Ok, install};
mod actions_parser;
mod ast;

fn main() -> Result<()> {
    install()?;
    println!("Hello, world!");
    //sh_parser::parse_sh("");
    Ok(())
}
