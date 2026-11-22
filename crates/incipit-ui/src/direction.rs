//! Which way text is written. A paragraph runs right to left when its first letter is of a script
//! written that way (Hebrew, Arabic, Syriac, Thaana, N'Ko), as the Unicode bidirectional algorithm
//! decides a paragraph's way; letters of other scripts, digits and marks run left to right or take
//! the way of what is around them.

/// Whether `c` is a letter of a script written right to left.
pub fn is_right_to_left(c: char) -> bool {
    matches!(c, '\u{0590}'..='\u{08FF}' | '\u{FB1D}'..='\u{FDFF}' | '\u{FE70}'..='\u{FEFF}')
}

/// Whether a paragraph runs right to left: its first letter is of a script written that way.
pub fn right_to_left(text: &str) -> bool {
    text.chars()
        .find(|c| c.is_alphabetic())
        .is_some_and(is_right_to_left)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_paragraph_runs_the_way_of_its_first_letter() {
        assert!(right_to_left("«תורת היחסות» of Einstein"));
        assert!(right_to_left("1905: النظرية النسبية"));
        assert!(!right_to_left("Einstein's תורת היחסות"));
        assert!(!right_to_left("— 42 —"));
    }
}
