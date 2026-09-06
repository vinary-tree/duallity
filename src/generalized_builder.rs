use libdictenstein::{Dictionary, DictionaryNode};
use liblevenshtein::transducer::{OperationSet, OperationSetBuilder};

use crate::generalized_wfst::GeneralizedWfst;

/// Builder for Generalized WFST.
pub struct GeneralizedWfstBuilder<'a, D>
where
    D: Dictionary + Clone + Send + Sync,
    D::Node: DictionaryNode<Unit = char>,
{
    dictionary: &'a D,
    query: Option<String>,
    max_distance: u8,
    operations: OperationSet,
}

impl<'a, D> GeneralizedWfstBuilder<'a, D>
where
    D: Dictionary + Clone + Send + Sync,
    D::Node: DictionaryNode<Unit = char>,
{
    /// Create a new builder.
    pub fn new(dictionary: &'a D) -> Self {
        Self {
            dictionary,
            query: None,
            max_distance: 2,
            operations: OperationSet::standard(),
        }
    }

    /// Set the query string.
    pub fn query(mut self, query: &str) -> Self {
        self.query = Some(query.to_string());
        self
    }

    /// Set the maximum distance.
    pub fn max_distance(mut self, distance: u8) -> Self {
        self.max_distance = distance;
        self
    }

    /// Use standard operations.
    pub fn with_standard_ops(mut self) -> Self {
        self.operations = OperationSet::standard();
        self
    }

    /// Add transposition support.
    pub fn with_transposition(mut self) -> Self {
        self.operations = OperationSet::with_transposition();
        self
    }

    /// Add merge/split support.
    pub fn with_merge_split(mut self) -> Self {
        self.operations = OperationSet::with_merge_split();
        self
    }

    /// Use custom operations.
    pub fn with_operations(mut self, operations: OperationSet) -> Self {
        self.operations = operations;
        self
    }

    /// Add phonetic digraph operations.
    pub fn with_phonetic_digraphs(mut self) -> Self {
        use liblevenshtein::transducer::phonetic::consonant_digraphs;

        let phonetic_ops = consonant_digraphs();
        let mut builder = OperationSetBuilder::new().with_standard_ops();

        for op in phonetic_ops.operations() {
            builder = builder.with_operation(op.clone());
        }

        self.operations = builder.build();
        self
    }

    /// Build the WFST.
    pub fn build(self) -> Result<GeneralizedWfst<D>, String> {
        let query = self.query.ok_or_else(|| "Query not set".to_string())?;
        GeneralizedWfst::try_new(self.dictionary, &query, self.max_distance, self.operations)
            .map_err(|error| error.to_string())
    }
}
