mod route;
mod usecase;

pub(in crate::auth) use route::create_local_user;
#[cfg(test)]
pub(in crate::auth) use usecase::execute;

#[cfg(test)]
mod tests;
