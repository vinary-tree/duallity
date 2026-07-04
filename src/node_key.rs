/// Compact exact key for dictionary nodes discovered by path traversal.
///
/// Dictionary nodes do not expose stable identities, so lazy state sources
/// identify a child by the exact `(parent_node_id, edge_label)` path step that
/// reached it. Unicode scalar values need at most 21 bits, leaving enough room
/// to pack a 32-bit parent ID and label into one `u64`; `u64::MAX` is reserved
/// for the root and is unreachable for packed child keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct DictionaryNodeKey(u64);

const CHAR_BITS: u64 = 21;
const CHAR_MASK: u64 = (1 << CHAR_BITS) - 1;

impl DictionaryNodeKey {
    pub(crate) const ROOT: Self = Self(u64::MAX);

    #[inline]
    pub(crate) fn child(parent_id: u32, edge_label: char) -> Self {
        let codepoint = edge_label as u64;
        debug_assert!(codepoint <= CHAR_MASK);
        Self((u64::from(parent_id) << CHAR_BITS) | codepoint)
    }
}

#[cfg(test)]
mod tests {
    use super::DictionaryNodeKey;

    #[test]
    fn child_keys_are_distinct_from_root() {
        assert_ne!(DictionaryNodeKey::ROOT, DictionaryNodeKey::child(0, '\0'));
        assert_ne!(
            DictionaryNodeKey::ROOT,
            DictionaryNodeKey::child(u32::MAX, char::MAX)
        );
    }

    #[test]
    fn child_keys_encode_parent_and_label() {
        assert_ne!(
            DictionaryNodeKey::child(1, 'a'),
            DictionaryNodeKey::child(2, 'a')
        );
        assert_ne!(
            DictionaryNodeKey::child(1, 'a'),
            DictionaryNodeKey::child(1, 'b')
        );
        assert_eq!(
            DictionaryNodeKey::child(1, 'a'),
            DictionaryNodeKey::child(1, 'a')
        );
    }
}
