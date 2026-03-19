use crate::FactId;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

const BM25_K1: f64 = 1.2;
const BM25_B: f64 = 0.75;

#[derive(Debug, Clone)]
pub(crate) struct LexicalDocument {
    pub(crate) id: FactId,
    pub(crate) content: String,
}

impl LexicalDocument {
    pub(crate) fn new(id: FactId, content: String) -> Self {
        Self { id, content }
    }
}

#[derive(Debug, Clone)]
struct IndexedDocument {
    id: FactId,
    length: usize,
}

#[derive(Debug, Default)]
struct LexicalIndex {
    documents: Vec<IndexedDocument>,
    postings: HashMap<String, Vec<(usize, usize)>>,
    doc_freqs: HashMap<String, usize>,
    vocabulary: Vec<String>,
    avg_doc_len: f64,
}

impl LexicalIndex {
    fn build(docs: &[LexicalDocument]) -> Self {
        let mut documents = Vec::with_capacity(docs.len());
        let mut postings: HashMap<String, Vec<(usize, usize)>> = HashMap::new();
        let mut doc_freqs: HashMap<String, usize> = HashMap::new();
        let mut total_doc_len = 0usize;

        for doc in docs {
            let tokens = tokenize(&doc.content);
            let doc_idx = documents.len();
            let mut term_freqs: HashMap<String, usize> = HashMap::new();
            for token in &tokens {
                *term_freqs.entry(token.clone()).or_insert(0) += 1;
            }

            for (term, tf) in &term_freqs {
                postings
                    .entry(term.clone())
                    .or_default()
                    .push((doc_idx, *tf));
                *doc_freqs.entry(term.clone()).or_insert(0) += 1;
            }

            total_doc_len += tokens.len();
            documents.push(IndexedDocument {
                id: doc.id.clone(),
                length: tokens.len(),
            });
        }

        let mut vocabulary: Vec<String> = doc_freqs.keys().cloned().collect();
        vocabulary.sort();

        let avg_doc_len = if documents.is_empty() {
            1.0
        } else {
            total_doc_len as f64 / documents.len() as f64
        };

        Self {
            documents,
            postings,
            doc_freqs,
            vocabulary,
            avg_doc_len,
        }
    }

    fn search(&self, query: &str, limit: usize) -> Vec<(FactId, f32)> {
        if limit == 0 {
            return Vec::new();
        }

        let query_terms = tokenize(query);
        if query_terms.is_empty() {
            return Vec::new();
        }

        let exact = self.search_terms(&query_terms, limit);
        if !exact.is_empty() {
            return exact;
        }

        let fuzzy_terms = self.fuzzy_terms(&query_terms);
        if fuzzy_terms.is_empty() {
            return Vec::new();
        }

        self.search_terms(&fuzzy_terms, limit)
    }

    fn search_terms(&self, query_terms: &[String], limit: usize) -> Vec<(FactId, f32)> {
        let mut scores: HashMap<usize, f64> = HashMap::new();
        let total_docs = self.documents.len() as f64;
        let avg_doc_len = self.avg_doc_len.max(1.0);

        for query_term in query_terms {
            let Some(postings) = self.postings.get(query_term) else {
                continue;
            };
            let df = *self.doc_freqs.get(query_term).unwrap_or(&0) as f64;
            if df == 0.0 {
                continue;
            }

            let idf = (1.0 + (total_docs - df + 0.5) / (df + 0.5)).ln();
            for &(doc_idx, tf) in postings {
                let doc_len = self.documents[doc_idx].length as f64;
                let tf = tf as f64;
                let norm = tf + BM25_K1 * (1.0 - BM25_B + BM25_B * (doc_len / avg_doc_len));
                let score = idf * (tf * (BM25_K1 + 1.0) / norm);
                *scores.entry(doc_idx).or_insert(0.0) += score;
            }
        }

        let mut hits: Vec<(FactId, f32)> = scores
            .into_iter()
            .filter(|(_, score)| *score > 0.0)
            .map(|(doc_idx, score)| (self.documents[doc_idx].id.clone(), score as f32))
            .collect();

        hits.sort_by(|(a_id, a_score), (b_id, b_score)| {
            b_score
                .partial_cmp(a_score)
                .unwrap_or(Ordering::Equal)
                .then_with(|| a_id.0.cmp(&b_id.0))
        });
        hits.truncate(limit);
        hits
    }

    fn fuzzy_terms(&self, query_terms: &[String]) -> Vec<String> {
        let mut seen = HashSet::new();
        let mut fuzzy_terms = Vec::new();

        for query_term in query_terms {
            for vocab_term in &self.vocabulary {
                if is_edit_distance_le_one(query_term, vocab_term)
                    && seen.insert(vocab_term.clone())
                {
                    fuzzy_terms.push(vocab_term.clone());
                }
            }
        }

        fuzzy_terms
    }
}

pub(crate) fn search_scored(
    docs: &[LexicalDocument],
    query: &str,
    limit: usize,
) -> Vec<(FactId, f32)> {
    if docs.is_empty() || limit == 0 || query.trim().is_empty() {
        return Vec::new();
    }
    LexicalIndex::build(docs).search(query, limit)
}

pub(crate) fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();

    for ch in text.chars() {
        if ch.is_alphanumeric() {
            current.extend(ch.to_lowercase());
        } else if !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

fn is_edit_distance_le_one(left: &str, right: &str) -> bool {
    if left == right {
        return true;
    }

    let left_chars: Vec<char> = left.chars().collect();
    let right_chars: Vec<char> = right.chars().collect();
    let left_len = left_chars.len();
    let right_len = right_chars.len();

    if left_len.abs_diff(right_len) > 1 {
        return false;
    }

    if left_len == right_len {
        let mismatches: Vec<usize> = left_chars
            .iter()
            .zip(right_chars.iter())
            .enumerate()
            .filter_map(|(idx, (l, r))| if l != r { Some(idx) } else { None })
            .collect();

        return match mismatches.as_slice() {
            [_] => true,
            [first, second]
                if *second == *first + 1
                    && left_chars[*first] == right_chars[*second]
                    && left_chars[*second] == right_chars[*first] =>
            {
                true
            }
            _ => false,
        };
    }

    let (shorter, longer) = if left_len < right_len {
        (&left_chars, &right_chars)
    } else {
        (&right_chars, &left_chars)
    };

    let mut i = 0usize;
    let mut j = 0usize;
    let mut skipped = false;
    while i < shorter.len() && j < longer.len() {
        if shorter[i] == longer[j] {
            i += 1;
            j += 1;
            continue;
        }
        if skipped {
            return false;
        }
        skipped = true;
        j += 1;
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "fulltext")]
    use tantivy::collector::TopDocs;
    #[cfg(feature = "fulltext")]
    use tantivy::query::{BooleanQuery, FuzzyTermQuery, Occur, QueryParser};
    #[cfg(feature = "fulltext")]
    use tantivy::schema::{Field, Schema, Value as TantivyValueTrait, STORED, STRING, TEXT};
    #[cfg(feature = "fulltext")]
    use tantivy::{doc, Index, Term};

    fn doc_with(id: &str, content: &str) -> LexicalDocument {
        LexicalDocument::new(FactId(id.to_string()), content.to_string())
    }

    #[test]
    fn tokenize_normalizes_non_alphanumeric_text() {
        let tokens = tokenize("Alice works_at Acme-Corp!");
        assert_eq!(tokens, vec!["alice", "works", "at", "acme", "corp"]);
    }

    #[test]
    fn fuzzy_fallback_only_runs_when_exact_search_is_empty() {
        let docs = vec![doc_with("a", "alice"), doc_with("b", "alcie")];
        let hits = search_scored(&docs, "alice", 10);
        let ids: Vec<&str> = hits.iter().map(|(id, _)| id.0.as_str()).collect();
        assert_eq!(ids, vec!["a"]);
    }

    #[test]
    fn search_orders_ties_by_fact_id() {
        let docs = vec![doc_with("b", "rust"), doc_with("a", "rust")];
        let hits = search_scored(&docs, "rust", 10);
        let ids: Vec<&str> = hits.iter().map(|(id, _)| id.0.as_str()).collect();
        assert_eq!(ids, vec!["a", "b"]);
    }

    #[cfg(feature = "fulltext")]
    fn tantivy_shadow_search_scored(
        docs: &[LexicalDocument],
        query: &str,
        limit: usize,
    ) -> Vec<(FactId, f32)> {
        if docs.is_empty() || limit == 0 || query.trim().is_empty() {
            return Vec::new();
        }

        let mut schema_builder = Schema::builder();
        let id_field = schema_builder.add_text_field("id", STRING | STORED);
        let content_field = schema_builder.add_text_field("content", TEXT);
        let schema = schema_builder.build();
        let index = Index::create_in_ram(schema);
        let mut writer = index.writer(50_000_000).unwrap();

        for doc_row in docs {
            writer
                .add_document(doc!(
                    id_field => doc_row.id.0.clone(),
                    content_field => doc_row.content.clone(),
                ))
                .unwrap();
        }
        writer.commit().unwrap();

        let reader = index.reader().unwrap();
        let searcher = reader.searcher();
        let parser = QueryParser::for_index(&index, vec![content_field]);
        let parsed = parser.parse_query(query).unwrap();
        let mut top_docs = searcher
            .search(&parsed, &TopDocs::with_limit(limit))
            .unwrap();

        if top_docs.is_empty() {
            let fuzzy = tantivy_fuzzy_query(query, content_field);
            top_docs = searcher
                .search(&fuzzy, &TopDocs::with_limit(limit))
                .unwrap();
        }

        let mut hits = Vec::new();
        for (score, addr) in top_docs {
            let retrieved = searcher
                .doc::<tantivy::schema::TantivyDocument>(addr)
                .unwrap();
            if let Some(id_val) = retrieved.get_first(id_field).and_then(|v| v.as_str()) {
                hits.push((FactId(id_val.to_string()), score));
            }
        }
        hits
    }

    #[cfg(feature = "fulltext")]
    fn tantivy_fuzzy_query(query: &str, content_field: Field) -> BooleanQuery {
        let terms: Vec<(Occur, Box<dyn tantivy::query::Query>)> = query
            .split_whitespace()
            .filter(|token| !token.is_empty())
            .map(|token| {
                let term = Term::from_field_text(content_field, token);
                (
                    Occur::Should,
                    Box::new(FuzzyTermQuery::new(term, 1, true)) as Box<dyn tantivy::query::Query>,
                )
            })
            .collect();
        BooleanQuery::new(terms)
    }

    #[test]
    #[cfg(feature = "fulltext")]
    fn lexical_engine_matches_tantivy_for_documented_queries() {
        let docs = vec![
            doc_with("fact-a", "alice works_at Acme ally works at"),
            doc_with("fact-b", "bob works_at Acme Industries"),
            doc_with("fact-c", "carol lives_in London"),
        ];

        for query in ["Acme", "alice works at", "ally", "alcie"] {
            let lexical_ids: Vec<String> = search_scored(&docs, query, 10)
                .into_iter()
                .map(|(id, _)| id.0)
                .collect();
            let tantivy_ids: Vec<String> = tantivy_shadow_search_scored(&docs, query, 10)
                .into_iter()
                .map(|(id, _)| id.0)
                .collect();
            assert_eq!(
                lexical_ids, tantivy_ids,
                "lexical engine should preserve Tantivy result IDs for query {query:?}"
            );
        }
    }
}
