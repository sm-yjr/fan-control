//! 可跨平台的风扇控制核心。所有硬件写入由宿主执行，并通过 acknowledge 确认。

mod adaptive;
mod config;
mod controller;
mod model;
mod policy;
mod thermal;
mod tuning;

pub use adaptive::*;
pub use config::*;
pub use controller::*;
pub use model::*;
pub use policy::*;
pub use thermal::*;
pub use tuning::*;
