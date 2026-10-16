mod configuration;
mod identity;
pub use configuration::configuration;
#[cfg(test)]
pub use identity::ProductIdentity;
pub use identity::ProductIdentityPlugin;
