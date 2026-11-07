mod route;
mod usecase;

pub(super) use route::login;
pub use usecase::LocalLogin;

#[cfg(test)]
mod tests;
