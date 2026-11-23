//! Find in the graph: which nodes' titles have the words typed into the graph's field, best
//! first, and which one Enter goes to. A title matches when it has every word, in any case and
//! anywhere in it, so the field narrows as one types. The best match is the title that is the
//! words themselves, then one that starts with them, then one where each word starts a word of
//! the title, then the rest; among equals, the node with more links, then the shorter title.
//! Pure, so it is tested without a window.

/// How closely a title matches the words: lower is better.
fn closeness(title: &str, query: &str, words: &[String]) -> u8 {
    if title == query {
        0
    } else if title.starts_with(query) {
        1
    } else if words.iter().all(|w| {
        title
            .split(|c: char| !c.is_alphanumeric())
            .any(|part| part.starts_with(w.as_str()))
    }) {
        2
    } else {
        3
    }
}

/// The nodes whose titles have every word of `query`, best first. `links[i]` is node `i`'s
/// number of links. An empty query matches nothing.
pub fn matches(titles: &[String], links: &[usize], query: &str) -> Vec<usize> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    if words.is_empty() {
        return Vec::new();
    }
    let query = words.join(" ");
    let mut found: Vec<(u8, usize)> = titles
        .iter()
        .enumerate()
        .filter_map(|(node, title)| {
            let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
            let title = title.to_lowercase();
            words
                .iter()
                .all(|w| title.contains(w.as_str()))
                .then(|| (closeness(&title, &query, &words), node))
        })
        .collect();
    found.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(links[b.1].cmp(&links[a.1]))
            .then(titles[a.1].len().cmp(&titles[b.1].len()))
            .then(a.1.cmp(&b.1))
    });
    found.into_iter().map(|(_, node)| node).collect()
}

/// Where Enter goes: the best match, or, when a match is already selected, the one after it, back
/// to the first after the last.
pub fn next_match(matches: &[usize], selected: Option<usize>) -> Option<usize> {
    let at = selected.and_then(|s| matches.iter().position(|m| *m == s));
    match at {
        Some(i) => matches.get((i + 1) % matches.len()).copied(),
        None => matches.first().copied(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn titles(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    #[test]
    fn a_title_matches_when_it_has_every_word_in_any_case() {
        let t = titles(&["Maxwell's demon", "Entropy", "Black hole entropy", "Demons"]);
        let links = [1, 1, 1, 1];
        assert_eq!(matches(&t, &links, "DEMON"), vec![3, 0]);
        assert_eq!(matches(&t, &links, "hole  entropy"), vec![2]);
        assert_eq!(matches(&t, &links, "entropy demon"), Vec::<usize>::new());
    }

    #[test]
    fn the_title_itself_comes_before_one_that_starts_with_it_before_one_that_has_it() {
        let t = titles(&[
            "Black hole entropy",
            "Entropy and time",
            "Entropy",
            "Negentropy",
        ]);
        let links = [9, 9, 0, 9];
        assert_eq!(matches(&t, &links, "entropy"), vec![2, 1, 0, 3]);
    }

    #[test]
    fn among_equals_the_better_linked_then_the_shorter_comes_first() {
        let t = titles(&["Heat engine notes", "Heat engine", "Heat engines"]);
        assert_eq!(matches(&t, &[1, 1, 5], "heat eng"), vec![2, 1, 0]);
    }

    #[test]
    fn nothing_typed_matches_nothing() {
        let t = titles(&["Entropy"]);
        assert!(matches(&t, &[0], "").is_empty());
        assert!(matches(&t, &[0], "   ").is_empty());
    }

    #[test]
    fn enter_goes_to_the_best_match_then_to_each_next_one_and_round() {
        let found = [4, 1, 7];
        assert_eq!(next_match(&found, None), Some(4));
        assert_eq!(next_match(&found, Some(9)), Some(4));
        assert_eq!(next_match(&found, Some(4)), Some(1));
        assert_eq!(next_match(&found, Some(7)), Some(4));
        assert_eq!(next_match(&[], Some(4)), None);
    }
}
