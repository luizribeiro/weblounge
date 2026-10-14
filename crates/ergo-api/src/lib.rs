//! A typed client for [Ergo's HTTP API][api].
//!
//! Every endpoint is a request type implementing [`Endpoint`], which names
//! its path and response type; [`Client::call`] sends any of them. The
//! convenience methods on [`Client`] wrap the calls concierge makes, turning
//! Ergo's expected refusals (a wrong password, a taken name) into values.
//!
//! [api]: https://github.com/ergochat/ergo/blob/v2.19.1/docs/API.md

/// Declares the path a request type is sent to and the response it gets.
macro_rules! endpoint {
    ($request:ty, $path:literal => $response:ty) => {
        impl $crate::Endpoint for $request {
            const PATH: &'static str = $path;
            type Response = $response;
        }
    };
}

mod client;
mod endpoints;
mod error;
mod methods;
mod names;
#[cfg(test)]
mod stub;

pub use client::{Client, Endpoint};
pub use endpoints::*;
pub use error::{Error, ErrorCode, Result};
pub use methods::Registration;
pub use names::{AccountName, InvalidPassphrase, Passphrase};
pub use reqwest::{StatusCode, Url};
