//! A typed client for [Ergo's HTTP API][api].
//!
//! [api]: https://github.com/ergochat/ergo/blob/v2.19.1/docs/API.md

mod error;

pub use error::{Error, ErrorCode, Result};
pub use reqwest::StatusCode;
