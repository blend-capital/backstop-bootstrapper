mod comet;
pub use comet::Client as CometClient;
mod backstop;
pub use backstop::Client as BackstopClient;
#[cfg(any(test, feature = "testutils"))]
pub use backstop::WASM as BackstopContract;
pub use comet::WASM as CometContract;
