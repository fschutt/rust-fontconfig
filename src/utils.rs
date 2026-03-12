/// Normalize a family/font name for comparison: lowercase, strip all non-alphanumeric characters.
///
/// This ensures consistent matching regardless of spaces, hyphens, underscores, or casing.
pub(crate) fn normalize_family_name(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}
