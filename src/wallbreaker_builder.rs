use libdictenstein::substring::{BidirectionalDictionaryNode, SubstringDictionary};
use libdictenstein::{Dictionary, DictionaryNode};
use liblevenshtein::transducer::Algorithm;

use crate::wallbreaker_wfst::WallBreakerWfst;

/// Builder for WallBreaker WFST.
pub struct WallBreakerWfstBuilder<'a, D>
where
    D: Dictionary + SubstringDictionary + Clone + Send + Sync,
    D::Node: BidirectionalDictionaryNode,
    <D::Node as DictionaryNode>::Unit: Into<u32>,
{
    dictionary: &'a D,
    query: Option<String>,
    max_distance: usize,
    algorithm: Algorithm,
}

impl<'a, D> WallBreakerWfstBuilder<'a, D>
where
    D: Dictionary + SubstringDictionary + Clone + Send + Sync,
    D::Node: BidirectionalDictionaryNode,
    <D::Node as DictionaryNode>::Unit: Into<u32>,
{
    /// Create a new builder.
    pub fn new(dictionary: &'a D) -> Self {
        Self {
            dictionary,
            query: None,
            max_distance: 2,
            algorithm: Algorithm::Standard,
        }
    }

    /// Set the query string.
    pub fn query(mut self, query: &str) -> Self {
        self.query = Some(query.to_string());
        self
    }

    /// Set the maximum distance.
    pub fn max_distance(mut self, distance: usize) -> Self {
        self.max_distance = distance;
        self
    }

    /// Set the algorithm.
    pub fn algorithm(mut self, algorithm: Algorithm) -> Self {
        self.algorithm = algorithm;
        self
    }

    /// Use standard Levenshtein algorithm.
    pub fn standard(mut self) -> Self {
        self.algorithm = Algorithm::Standard;
        self
    }

    /// Use transposition (Damerau-Levenshtein) algorithm.
    pub fn transposition(mut self) -> Self {
        self.algorithm = Algorithm::Transposition;
        self
    }

    /// Use merge-and-split algorithm.
    pub fn merge_and_split(mut self) -> Self {
        self.algorithm = Algorithm::MergeAndSplit;
        self
    }

    /// Build the WFST.
    pub fn build(self) -> Result<WallBreakerWfst<'a, D>, String> {
        let query = self.query.ok_or_else(|| "Query not set".to_string())?;
        Ok(WallBreakerWfst::with_algorithm(
            self.dictionary,
            &query,
            self.max_distance,
            self.algorithm,
        ))
    }
}
