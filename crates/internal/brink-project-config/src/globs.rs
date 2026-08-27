//! Path-glob matching for `brink.toml` path lists (`[project] drafts`).
//!
//! This crate is deliberately dependency-free beyond the `brink-source-tree`
//! L0 leaf (see the `Cargo.toml` header), so it carries its own matcher
//! rather than pulling in `globset`/`glob`. That constraint is what decides
//! the dialect: a SMALL, fully specified subset, not a re-implementation of
//! gitignore.
//!
//! # The dialect
//!
//! A pattern is matched against the whole **project-relative, `/`-separated**
//! path. There is no leading-`./`, no `..`, and no absolute form — callers
//! normalize before asking (see [`matches_any`]).
//!
//! | Token | Matches |
//! |-------|---------|
//! | `?`   | exactly one character, never `/` |
//! | `*`   | any run of characters (including none), never `/` |
//! | `**`  | any run of characters, `/` included |
//! | any other character | itself, literally |
//!
//! A trailing `/` is sugar for `/**`, so `scratch/` and `scratch/**` mean the
//! same thing. Everything else is literal — in particular a bare directory
//! name does **not** match what is under it. `scratch` matches a file
//! *called* `scratch` and nothing else; to cover its contents write
//! `scratch/**`. That is the one place this deliberately departs from
//! gitignore, whose bare-name rule is a frequent source of "why did that
//! match?" — in a short, hand-written list of paths, being told to say what
//! you mean is cheaper than a silent over-match.
//!
//! Matching is case-sensitive.

/// Whether `path` matches any of `patterns`.
///
/// `path` is taken as already project-relative; a leading `./` is tolerated
/// and stripped, since that spelling reaches config lists routinely.
#[must_use]
pub fn matches_any(path: &str, patterns: &[String]) -> bool {
    let path = path.strip_prefix("./").unwrap_or(path);
    patterns.iter().any(|p| matches(path, p))
}

/// Whether `path` matches the single pattern `pattern`.
#[must_use]
pub fn matches(path: &str, pattern: &str) -> bool {
    // A pattern of `""` would otherwise match only the empty path; treat it
    // as matching nothing at all, so an accidental empty string in the list
    // is inert rather than mysterious.
    if pattern.is_empty() {
        return false;
    }
    let expanded;
    let pattern = if let Some(dir) = pattern.strip_suffix('/') {
        expanded = format!("{dir}/**");
        &expanded
    } else {
        pattern
    };
    match_here(path.as_bytes(), pattern.as_bytes())
}

/// Backtracking matcher over bytes.
///
/// Recursion is bounded by the pattern's wildcard count and each `*`/`**`
/// arm shrinks the remaining path, so this terminates on every input; the
/// patterns here are hand-written config lines, not adversarial ones.
fn match_here(path: &[u8], pat: &[u8]) -> bool {
    let (Some(&p0), Some(rest)) = (pat.first(), pat.get(1..)) else {
        return path.is_empty();
    };
    match p0 {
        b'*' => {
            if rest.first() == Some(&b'*') {
                // `**` — may cross `/`. Try every split, shortest first.
                let tail = &rest[1..];
                // `a/**/b` should also match `a/b`: a `**` between separators
                // can stand for nothing at all, which means consuming the
                // following `/` as well as the zero-length run before it.
                if let Some(after_slash) = tail.strip_prefix(b"/")
                    && match_here(path, after_slash)
                {
                    return true;
                }
                (0..=path.len()).any(|i| match_here(&path[i..], tail))
            } else {
                // `*` — stops at the first `/`.
                let limit = path.iter().position(|&c| c == b'/').unwrap_or(path.len());
                (0..=limit).any(|i| match_here(&path[i..], rest))
            }
        }
        b'?' => match path.first() {
            Some(&c) if c != b'/' => match_here(&path[1..], rest),
            _ => false,
        },
        lit => match path.first() {
            Some(&c) if c == lit => match_here(&path[1..], rest),
            _ => false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{matches, matches_any};

    #[test]
    fn a_literal_pattern_matches_only_itself() {
        assert!(matches("scratch/cut.ink", "scratch/cut.ink"));
        assert!(!matches("scratch/cut.ink", "scratch/cut.brink"));
        assert!(!matches("scratch/cut.ink", "scratch/cut"));
        assert!(!matches("a/scratch/cut.ink", "scratch/cut.ink"));
    }

    #[test]
    fn a_star_stops_at_a_separator() {
        assert!(matches("scratch/cut.ink", "scratch/*.ink"));
        assert!(matches("scratch/cut.ink", "*/cut.ink"));
        // The whole point of the `*`/`**` distinction: one `*` must not
        // swallow a directory boundary, or `*.ink` would cover the project.
        assert!(!matches("scratch/deep/cut.ink", "scratch/*.ink"));
        assert!(!matches("scratch/cut.ink", "*.ink"));
    }

    #[test]
    fn a_double_star_crosses_separators() {
        assert!(matches("scratch/deep/cut.ink", "scratch/**"));
        assert!(matches("scratch/deep/cut.ink", "**/cut.ink"));
        assert!(matches("scratch/deep/cut.ink", "**.ink"));
        assert!(matches("scratch/cut.ink", "scratch/**/cut.ink"));
        // `**` standing for nothing at all, separator included.
        assert!(matches("scratch/cut.ink", "**/scratch/cut.ink"));
        assert!(!matches("notes/cut.ink", "scratch/**"));
    }

    #[test]
    fn a_trailing_slash_is_sugar_for_everything_under_it() {
        assert!(matches("scratch/cut.ink", "scratch/"));
        assert!(matches("scratch/deep/cut.ink", "scratch/"));
        assert!(!matches("scratch", "scratch/"));
        assert!(!matches("scratchpad/cut.ink", "scratch/"));
    }

    #[test]
    fn a_bare_directory_name_does_not_cover_its_contents() {
        // The documented departure from gitignore. If this ever starts
        // passing, the doc comment on this module is a lie.
        assert!(!matches("scratch/cut.ink", "scratch"));
        assert!(matches("scratch", "scratch"));
    }

    #[test]
    fn a_question_mark_is_one_non_separator_character() {
        assert!(matches("act1.ink", "act?.ink"));
        assert!(!matches("act10.ink", "act?.ink"));
        assert!(!matches("a/b", "a?b"));
    }

    #[test]
    fn matching_is_case_sensitive() {
        assert!(!matches("Scratch/cut.ink", "scratch/**"));
    }

    #[test]
    fn an_empty_pattern_matches_nothing() {
        // Not even the empty path — an accidental "" in the list is inert.
        assert!(!matches("", ""));
        assert!(!matches("cut.ink", ""));
    }

    #[test]
    fn matches_any_is_the_or_of_its_patterns_and_tolerates_a_dot_slash() {
        let pats = vec!["scratch/**".to_owned(), "*.draft.ink".to_owned()];
        assert!(matches_any("scratch/cut.ink", &pats));
        assert!(matches_any("./scratch/cut.ink", &pats));
        assert!(matches_any("aside.draft.ink", &pats));
        assert!(!matches_any("main.ink", &pats));
        assert!(!matches_any("main.ink", &[]));
    }
}
