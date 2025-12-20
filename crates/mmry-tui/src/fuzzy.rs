pub fn fuzzy_score(query: &str, candidate: &str) -> Option<i64> {
    let query = query.trim();
    if query.is_empty() {
        return Some(0);
    }

    let query = query.to_lowercase();
    let candidate = candidate.to_lowercase();

    let mut score: i64 = 0;
    let mut last_match_idx: Option<usize> = None;
    let mut search_from = 0usize;

    for qc in query.chars() {
        let mut found = None;
        for (i, cc) in candidate[search_from..].char_indices() {
            if cc == qc {
                found = Some(search_from + i);
                break;
            }
        }
        let idx = found?;

        // Base score per matched character.
        score += 10;

        // Bonus for matches at word boundaries / separators.
        if idx == 0 {
            score += 20;
        } else if let Some(prev) = candidate[..idx].chars().last() {
            if prev == ' ' || prev == '-' || prev == '_' || prev == '/' || prev == ':' {
                score += 15;
            }
        }

        // Bonus for consecutive matches.
        if let Some(prev_idx) = last_match_idx {
            if idx == prev_idx + 1 {
                score += 25;
            } else {
                // Small penalty for gaps.
                score -= (idx as i64 - prev_idx as i64).saturating_sub(1);
            }
        }

        last_match_idx = Some(idx);
        search_from = idx + 1;
    }

    Some(score)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_score_empty_query_matches() {
        assert_eq!(fuzzy_score("", "abc"), Some(0));
        assert_eq!(fuzzy_score("   ", "abc"), Some(0));
    }

    #[test]
    fn fuzzy_score_requires_subsequence_match() {
        assert!(fuzzy_score("abc", "axbyc").is_some());
        assert!(fuzzy_score("abc", "acb").is_none());
    }

    #[test]
    fn fuzzy_score_is_case_insensitive() {
        assert!(fuzzy_score("AbC", "aBc").is_some());
    }

    #[test]
    fn fuzzy_score_prefers_consecutive_matches() {
        let consecutive = fuzzy_score("abc", "abc").unwrap();
        let gapped = fuzzy_score("abc", "a__b__c").unwrap();
        assert!(consecutive > gapped);
    }
}
