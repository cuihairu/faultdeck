use faultdeck_coordinator::serve;
use std::sync::{Arc, Mutex};

fn main() -> anyhow::Result<()> {
    let bind = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:7001".to_string());
    let state = Arc::new(Mutex::new(
        faultdeck_coordinator::CoordinatorState::default(),
    ));
    serve(state, &bind)?;
    Ok(())
}
