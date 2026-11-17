//! All text the user sees goes through `t`, so that other languages can be added later
//! by swapping the table.

/// The text for a key. Keys are the English texts themselves; a missing translation
/// simply shows the key.
pub fn t(key: &'static str) -> &'static str {
    key
}
