pub mod config;
pub mod error;
pub mod http;
pub mod keyfiles;
pub mod migration;
pub mod password;
pub mod sqlxdb;
pub mod tracing;

pub use error::{Error, Result};

pub fn add(left: u64, right: u64) -> u64 {
    left + right
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}
