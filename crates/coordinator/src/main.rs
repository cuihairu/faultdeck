use faultdeck_coordinator::serve_with_token;
use std::sync::{Arc, Mutex};

fn main() -> anyhow::Result<()> {
    let mut bind = "127.0.0.1:7001".to_string();
    let mut token: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--bind" => {
                bind = args
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("--bind needs value"))?
            }
            "--token" => {
                token = Some(
                    args.next()
                        .ok_or_else(|| anyhow::anyhow!("--token needs value"))?,
                )
            }
            _ => anyhow::bail!("unknown arg {arg}"),
        }
    }
    let state = Arc::new(Mutex::new(
        faultdeck_coordinator::CoordinatorState::default(),
    ));
    serve_with_token(state, &bind, token)?;
    Ok(())
}
