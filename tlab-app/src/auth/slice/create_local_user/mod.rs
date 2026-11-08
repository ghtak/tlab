mod route;
mod usecase;

pub(in crate::auth) use route::create_local_user;

#[cfg(test)]
mod tests;
