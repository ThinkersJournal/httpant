//! Constants shared by the app, the cases and the mutants, kept here so those modules do not import each other in a ring.

/// The body limit the `/echo` route reads with (1 MiB).
pub const BODY_LIMIT: usize = 1024 * 1024;
