//! Thread-affine native plugin runtime, independent of application UI and storage.
//! The application closes capability resources and calls `Runtime::shutdown`
//! before ending its graphics/COM lifetime. Accepted DLLs stay loaded until exit.
mod activation;
mod native;
mod runtime;

pub use activation::ActivationConfig;
pub use runtime::{Diagnostic, PluginInfo, PluginState, Runtime};
