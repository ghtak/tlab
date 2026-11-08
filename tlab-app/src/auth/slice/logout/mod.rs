mod route;
mod usecase;

pub(in crate::auth) use route::{logout, web_logout};
#[cfg(test)]
pub(in crate::auth) use usecase::execute;

#[cfg(test)]
mod tests;
