//! Rank graph labels against a spoken or typed name, taking identifier
//! structure into account.
//!
//! Spoken names come out of normalisation as words ("graph service"), while
//! labels are identifiers (`GraphServiceActor`, `graph_service.rs`). Both are
//! split into lowercase word tokens and joined with spaces, then compared by
//! character trigram Jaccard similarity, plus a bonus for a prefix match and
//! a bonus for a whole-word match.

use std::collections::HashSet;

/// Bonus when the label's joined tokens start with the query's joined tokens.
pub const PREFIX_BONUS: f32 = 0.3;
/// Bonus, given once, when any query token equals a label token.
pub const EXACT_TOKEN_BONUS: f32 = 0.2;

/// Split an identifier or phrase into lowercase word tokens.
///
/// Boundaries are any character that is not alphanumeric (`_`, `-`, `.`, `/`,
/// `:`, whitespace…) and camelCase transitions. A run of capitals keeps its
/// last capital for the next word (`HTTPServer` splits as `http`, `server`).
/// Digits stay attached to the word they follow.
///
/// ```
/// use codescape_collab::split_identifier;
/// assert_eq!(split_identifier("GraphServiceActor"), ["graph", "service", "actor"]);
/// assert_eq!(split_identifier("src/graph_service.rs"), ["src", "graph", "service", "rs"]);
/// assert_eq!(split_identifier("HTTPServer::new"), ["http", "server", "new"]);
/// ```
pub fn split_identifier(s: &str) -> Vec<String> {
    let chars: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut cur = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if !c.is_alphanumeric() {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            continue;
        }
        if c.is_uppercase() && !cur.is_empty() {
            let prev = chars[i - 1];
            let next_lower = chars.get(i + 1).is_some_and(|n| n.is_lowercase());
            if prev.is_lowercase() || prev.is_numeric() || (prev.is_uppercase() && next_lower) {
                out.push(std::mem::take(&mut cur));
            }
        }
        cur.extend(c.to_lowercase());
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn trigrams(joined: &str) -> HashSet<[char; 3]> {
    let padded: Vec<char> = std::iter::once(' ')
        .chain(joined.chars())
        .chain([' '])
        .collect();
    padded.windows(3).map(|w| [w[0], w[1], w[2]]).collect()
}

/// Score each label against `query` and return the best `limit` as
/// `(id, score)`. Results are sorted by descending score, then ascending id;
/// only scores above zero are returned.
///
/// The score is the trigram Jaccard similarity of the joined tokens, plus
/// [`PREFIX_BONUS`] and [`EXACT_TOKEN_BONUS`] where they apply. A query with
/// no tokens matches nothing.
///
/// ```
/// use codescape_collab::fuzzy_rank;
/// let labels = [(1, "GraphServiceActor"), (2, "graph_service.rs"), (3, "GraphData"), (4, "Service")];
/// let ranked = fuzzy_rank("graph service", labels.into_iter(), 2);
/// assert_eq!(ranked.iter().map(|r| r.0).collect::<Vec<_>>(), [2, 1]);
/// ```
pub fn fuzzy_rank<'a>(
    query: &str,
    labels: impl Iterator<Item = (u32, &'a str)>,
    limit: usize,
) -> Vec<(u32, f32)> {
    let q_tokens = split_identifier(query);
    if q_tokens.is_empty() || limit == 0 {
        return Vec::new();
    }
    let q_joined = q_tokens.join(" ");
    let q_tri = trigrams(&q_joined);
    let q_set: HashSet<&str> = q_tokens.iter().map(String::as_str).collect();

    let mut scored: Vec<(u32, f32)> = labels
        .filter_map(|(id, label)| {
            let l_tokens = split_identifier(label);
            if l_tokens.is_empty() {
                return None;
            }
            let l_joined = l_tokens.join(" ");
            let l_tri = trigrams(&l_joined);
            let inter = q_tri.intersection(&l_tri).count();
            let union = q_tri.len() + l_tri.len() - inter;
            let mut score = if union == 0 {
                0.0
            } else {
                inter as f32 / union as f32
            };
            if l_joined.starts_with(&q_joined) {
                score += PREFIX_BONUS;
            }
            if l_tokens.iter().any(|t| q_set.contains(t.as_str())) {
                score += EXACT_TOKEN_BONUS;
            }
            (score > 0.0).then_some((id, score))
        })
        .collect();
    scored.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    scored.truncate(limit);
    scored
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graph_service_ranking_order() {
        let labels = [
            (1, "GraphServiceActor"),
            (2, "graph_service.rs"),
            (3, "GraphData"),
            (4, "Service"),
        ];
        let ranked = fuzzy_rank("graph service", labels.into_iter(), 10);
        let ids: Vec<u32> = ranked.iter().map(|r| r.0).collect();
        assert_eq!(ids, vec![2, 1, 4, 3]);
        // graph_service.rs: 13 shared trigrams of 16 + prefix + exact token.
        assert!(
            (ranked[0].1 - (13.0 / 16.0 + 0.5)).abs() < 1e-6,
            "{ranked:?}"
        );
        assert!(
            (ranked[1].1 - (13.0 / 19.0 + 0.5)).abs() < 1e-6,
            "{ranked:?}"
        );
    }

    #[test]
    fn spoken_form_matches_identifier() {
        let labels = [(7, "parseCommand"), (8, "parse_target"), (9, "Command")];
        let ranked = fuzzy_rank("parse command", labels.into_iter(), 3);
        assert_eq!(ranked[0].0, 7);
    }

    #[test]
    fn edge_cases() {
        assert!(fuzzy_rank("", [(1, "x")].into_iter(), 5).is_empty());
        assert!(fuzzy_rank("::", [(1, "x")].into_iter(), 5).is_empty());
        assert!(fuzzy_rank("abc", [(1, "abc")].into_iter(), 0).is_empty());
        assert!(fuzzy_rank("abc", [(1, "zzz"), (2, "")].into_iter(), 5).is_empty());
        let ties = fuzzy_rank("a", [(5, "a"), (3, "a")].into_iter(), 5);
        assert_eq!(ties.iter().map(|t| t.0).collect::<Vec<_>>(), vec![3, 5]);
        assert_eq!(split_identifier("v3Frame"), ["v3", "frame"]);
        assert_eq!(split_identifier("İSTANBUL"), ["i̇stanbul"]);
        assert!(split_identifier("__").is_empty());
    }
}
