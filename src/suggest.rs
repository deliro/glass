pub fn levenshtein(a: &str, b: &str) -> usize {
    let b_chars: Vec<char> = b.chars().collect();
    let b_len = b_chars.len();
    let mut prev: Vec<usize> = (0..=b_len).collect();
    let mut curr = vec![0; b_len + 1];

    for (i, ca) in a.chars().enumerate() {
        if let Some(slot) = curr.get_mut(0) {
            *slot = i + 1;
        }
        for (j, &cb) in b_chars.iter().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            let replace = prev
                .get(j)
                .copied()
                .unwrap_or(usize::MAX)
                .saturating_add(cost);
            let delete = prev
                .get(j + 1)
                .copied()
                .unwrap_or(usize::MAX)
                .saturating_add(1);
            let insert = curr.get(j).copied().unwrap_or(usize::MAX).saturating_add(1);
            if let Some(slot) = curr.get_mut(j + 1) {
                *slot = replace.min(delete).min(insert);
            }
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev.get(b_len).copied().unwrap_or(usize::MAX)
}

pub fn closest_match<'a>(name: &str, candidates: impl Iterator<Item = &'a str>) -> Option<String> {
    let mut best: Option<(usize, &str)> = None;
    for candidate in candidates {
        let dist = levenshtein(name, candidate);
        if dist > 0 && dist <= 2 {
            match best {
                Some((d, _)) if dist < d => best = Some((dist, candidate)),
                None => best = Some((dist, candidate)),
                _ => {}
            }
        }
    }
    best.map(|(_, s)| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_strings() {
        assert_eq!(levenshtein("abc", "abc"), 0);
    }

    #[test]
    fn single_insertion() {
        assert_eq!(levenshtein("abc", "abcd"), 1);
    }

    #[test]
    fn single_deletion() {
        assert_eq!(levenshtein("abcd", "abc"), 1);
    }

    #[test]
    fn single_substitution() {
        assert_eq!(levenshtein("abc", "aXc"), 1);
    }

    #[test]
    fn transposition_counts_as_two() {
        assert_eq!(levenshtein("ab", "ba"), 2);
    }

    #[test]
    fn empty_strings() {
        assert_eq!(levenshtein("", ""), 0);
        assert_eq!(levenshtein("abc", ""), 3);
        assert_eq!(levenshtein("", "abc"), 3);
    }

    #[test]
    fn closest_match_found() {
        let candidates = vec!["length", "width", "height"];
        let result = closest_match("lenght", candidates.into_iter());
        assert_eq!(result, Some("length".to_string()));
    }

    #[test]
    fn closest_match_none_when_too_far() {
        let candidates = vec!["foo", "bar", "baz"];
        let result = closest_match("completely_different", candidates.into_iter());
        assert_eq!(result, None);
    }

    #[test]
    fn closest_match_exact_not_suggested() {
        let candidates = vec!["abc"];
        let result = closest_match("abc", candidates.into_iter());
        assert_eq!(result, None);
    }
}
