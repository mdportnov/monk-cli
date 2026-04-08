use miette::Result;
use monk::cli;

#[tokio::main]
async fn main() -> Result<()> {
    cli::run().await
}
