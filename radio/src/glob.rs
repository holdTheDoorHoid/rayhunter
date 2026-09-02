//! Bounded glob matching.
//!
//! The only pattern language anywhere in this crate. Regular expressions are
//! deliberately absent: a rule can arrive from a browser, and a regex engine
//! is a denial-of-service surface that has to be reasoned about carefully.
//! A glob does what users actually want from a network-name rule and cannot
//! blow up.

/// Match `input` against a glob pattern, where `*` matches any run of
/// characters (including none) and `?` matches exactly one.
///
/// Implemented as the standard two-pointer scan: on a mismatch it rewinds to
/// just after the most recent `*` and advances the input by one character.
/// There is no recursion and no backtracking stack, so the worst case is
/// O(pattern x input) rather than exponential. Callers additionally cap both
/// lengths, so a crafted pattern cannot become a denial of service.
///
/// Matching is over `char`s, so a multi-byte character counts as one `?` and
/// a pattern can never split one.
pub fn glob_match(pattern: &str, input: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let s: Vec<char> = input.chars().collect();
    let (mut pi, mut si) = (0usize, 0usize);
    let mut star: Option<usize> = None;
    let mut resume = 0usize;

    while si < s.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == s[si]) {
            pi += 1;
            si += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            resume = si;
            pi += 1;
        } else if let Some(sp) = star {
            // Let the star absorb one more character and try again.
            pi = sp + 1;
            resume += 1;
            si = resume;
        } else {
            return false;
        }
    }
    // Trailing stars may match nothing.
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_patterns_match_exactly() {
        assert!(glob_match("exact", "exact"));
        assert!(!glob_match("exact", "exactly"));
        assert!(!glob_match("exact", "exac"));
    }

    #[test]
    fn star_matches_any_run_including_empty() {
        assert!(glob_match("Flock*", "FlockSafety-1234"));
        assert!(glob_match("Flock*", "Flock"));
        assert!(glob_match("*Safety*", "FlockSafety-1234"));
        assert!(glob_match("*", "anything"));
        assert!(glob_match("*", ""));
        assert!(!glob_match("Flock*", "HomeWiFi"));
    }

    #[test]
    fn question_mark_matches_exactly_one_character() {
        assert!(glob_match("Flock?afety*", "FlockSafety-1234"));
        assert!(!glob_match("Flock?", "Flock"));
        assert!(glob_match("Flock?", "Flocks"));
        assert!(!glob_match("Flock?", "Flocks1"));
    }

    #[test]
    fn empty_cases() {
        assert!(glob_match("", ""));
        assert!(!glob_match("", "x"));
        assert!(glob_match("*", ""));
    }

    #[test]
    fn multiple_stars_work() {
        assert!(glob_match("a*b*c", "axxbyyc"));
        assert!(!glob_match("a*b*c", "axxbyy"));
        assert!(glob_match("*cam*", "backyard-cam-02"));
    }

    #[test]
    fn matching_is_over_characters_not_bytes() {
        // Three characters, more than three bytes.
        assert!(glob_match(
            "???",
            "héllo".chars().take(3).collect::<String>().as_str()
        ));
        assert!(glob_match("h?llo", "héllo"));
    }

    /// The input that makes a naive recursive matcher hang. The two-pointer
    /// scan resolves it promptly; the length caps in `userrules` mean it can
    /// never get this large in practice anyway.
    #[test]
    fn adversarial_input_does_not_blow_up() {
        let pattern = "a*a*a*a*b";
        let input = "a".repeat(4000);
        let start = std::time::Instant::now();
        assert!(!glob_match(pattern, &input));
        assert!(
            start.elapsed().as_millis() < 500,
            "took {:?}, which suggests backtracking blow-up",
            start.elapsed()
        );
    }
}
