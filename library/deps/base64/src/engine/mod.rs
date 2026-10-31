// `base64::engine`: the [`Engine`] trait and the general-purpose engines.

pub mod general_purpose;

/// The import `use base64::Engine` (or `Engine as _`) names. `encode` and
/// `decode` are inherent on [`general_purpose::GeneralPurpose`].
pub trait Engine {}
