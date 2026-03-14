//! LCS-based alignment of hash sequences for line regeneration.

/// Result of aligning two hash sequences.
#[derive(Debug, PartialEq, Eq)]
pub enum Alignment {
    /// Line exists in both old and new with the same hash.
    Matched { old_idx: usize, new_idx: usize },
    /// Line exists only in the new sequence (insertion).
    Inserted { new_idx: usize },
    /// Line exists only in the old sequence (deletion).
    Removed { old_idx: usize },
}

/// Align two hash sequences using longest common subsequence.
///
/// Returns alignment entries ordered by position, interleaving old and new
/// indices as they appear.
pub fn align_hashes(old: &[&str], new: &[&str]) -> Vec<Alignment> {
    let m = old.len();
    let n = new.len();

    // Build DP table for LCS lengths.
    // dp[i][j] = length of LCS of old[0..i] and new[0..j].
    let mut dp = vec![vec![0u32; n + 1]; m + 1];
    for i in 1..=m {
        for j in 1..=n {
            if old[i - 1] == new[j - 1] {
                dp[i][j] = dp[i - 1][j - 1] + 1;
            } else if dp[i - 1][j] >= dp[i][j - 1] {
                dp[i][j] = dp[i - 1][j];
            } else {
                dp[i][j] = dp[i][j - 1];
            }
        }
    }

    // Walk the DP table to produce alignment entries.
    let mut result = Vec::new();
    let mut i = m;
    let mut j = n;

    // Collect in reverse, then reverse at the end.
    while i > 0 || j > 0 {
        if i > 0 && j > 0 && old[i - 1] == new[j - 1] {
            result.push(Alignment::Matched {
                old_idx: i - 1,
                new_idx: j - 1,
            });
            i -= 1;
            j -= 1;
        } else if j > 0 && (i == 0 || dp[i][j - 1] >= dp[i - 1][j]) {
            result.push(Alignment::Inserted { new_idx: j - 1 });
            j -= 1;
        } else {
            result.push(Alignment::Removed { old_idx: i - 1 });
            i -= 1;
        }
    }

    result.reverse();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_sequences() {
        let old = vec!["a", "b", "c"];
        let new = vec!["a", "b", "c"];
        let result = align_hashes(&old, &new);
        assert_eq!(
            result,
            vec![
                Alignment::Matched {
                    old_idx: 0,
                    new_idx: 0
                },
                Alignment::Matched {
                    old_idx: 1,
                    new_idx: 1
                },
                Alignment::Matched {
                    old_idx: 2,
                    new_idx: 2
                },
            ]
        );
    }

    #[test]
    fn insertion_in_middle() {
        let old = vec!["a", "c"];
        let new = vec!["a", "b", "c"];
        let result = align_hashes(&old, &new);
        assert_eq!(
            result,
            vec![
                Alignment::Matched {
                    old_idx: 0,
                    new_idx: 0
                },
                Alignment::Inserted { new_idx: 1 },
                Alignment::Matched {
                    old_idx: 1,
                    new_idx: 2
                },
            ]
        );
    }

    #[test]
    fn deletion_from_middle() {
        let old = vec!["a", "b", "c"];
        let new = vec!["a", "c"];
        let result = align_hashes(&old, &new);
        assert_eq!(
            result,
            vec![
                Alignment::Matched {
                    old_idx: 0,
                    new_idx: 0
                },
                Alignment::Removed { old_idx: 1 },
                Alignment::Matched {
                    old_idx: 2,
                    new_idx: 1
                },
            ]
        );
    }

    #[test]
    fn edit_one_hash_changes() {
        let old = vec!["a", "b", "c"];
        let new = vec!["a", "x", "c"];
        let result = align_hashes(&old, &new);
        assert_eq!(
            result,
            vec![
                Alignment::Matched {
                    old_idx: 0,
                    new_idx: 0
                },
                Alignment::Removed { old_idx: 1 },
                Alignment::Inserted { new_idx: 1 },
                Alignment::Matched {
                    old_idx: 2,
                    new_idx: 2
                },
            ]
        );
    }

    #[test]
    fn insertion_at_start() {
        let old = vec!["b", "c"];
        let new = vec!["a", "b", "c"];
        let result = align_hashes(&old, &new);
        assert_eq!(
            result,
            vec![
                Alignment::Inserted { new_idx: 0 },
                Alignment::Matched {
                    old_idx: 0,
                    new_idx: 1
                },
                Alignment::Matched {
                    old_idx: 1,
                    new_idx: 2
                },
            ]
        );
    }

    #[test]
    fn insertion_at_end() {
        let old = vec!["a", "b"];
        let new = vec!["a", "b", "c"];
        let result = align_hashes(&old, &new);
        assert_eq!(
            result,
            vec![
                Alignment::Matched {
                    old_idx: 0,
                    new_idx: 0
                },
                Alignment::Matched {
                    old_idx: 1,
                    new_idx: 1
                },
                Alignment::Inserted { new_idx: 2 },
            ]
        );
    }

    #[test]
    fn deletion_at_start() {
        let old = vec!["a", "b", "c"];
        let new = vec!["b", "c"];
        let result = align_hashes(&old, &new);
        assert_eq!(
            result,
            vec![
                Alignment::Removed { old_idx: 0 },
                Alignment::Matched {
                    old_idx: 1,
                    new_idx: 0
                },
                Alignment::Matched {
                    old_idx: 2,
                    new_idx: 1
                },
            ]
        );
    }

    #[test]
    fn deletion_at_end() {
        let old = vec!["a", "b", "c"];
        let new = vec!["a", "b"];
        let result = align_hashes(&old, &new);
        assert_eq!(
            result,
            vec![
                Alignment::Matched {
                    old_idx: 0,
                    new_idx: 0
                },
                Alignment::Matched {
                    old_idx: 1,
                    new_idx: 1
                },
                Alignment::Removed { old_idx: 2 },
            ]
        );
    }

    #[test]
    fn completely_different() {
        let old = vec!["a", "b"];
        let new = vec!["x", "y"];
        let result = align_hashes(&old, &new);
        assert_eq!(
            result,
            vec![
                Alignment::Removed { old_idx: 0 },
                Alignment::Removed { old_idx: 1 },
                Alignment::Inserted { new_idx: 0 },
                Alignment::Inserted { new_idx: 1 },
            ]
        );
    }

    #[test]
    fn empty_old() {
        let old: Vec<&str> = vec![];
        let new = vec!["a", "b"];
        let result = align_hashes(&old, &new);
        assert_eq!(
            result,
            vec![
                Alignment::Inserted { new_idx: 0 },
                Alignment::Inserted { new_idx: 1 },
            ]
        );
    }

    #[test]
    fn empty_new() {
        let old = vec!["a", "b"];
        let new: Vec<&str> = vec![];
        let result = align_hashes(&old, &new);
        assert_eq!(
            result,
            vec![
                Alignment::Removed { old_idx: 0 },
                Alignment::Removed { old_idx: 1 },
            ]
        );
    }

    #[test]
    fn duplicate_hashes_positional() {
        // Both sequences have duplicates — alignment should preserve positions.
        let old = vec!["a", "a", "b"];
        let new = vec!["a", "b", "a"];
        let result = align_hashes(&old, &new);
        // LCS: "a", "b" (length 2). Tie-breaking favors removing from old first,
        // so old[0] is removed, old[1] matches new[0], old[2] matches new[1],
        // and new[2] is inserted.
        assert_eq!(
            result,
            vec![
                Alignment::Removed { old_idx: 0 },
                Alignment::Matched {
                    old_idx: 1,
                    new_idx: 0
                },
                Alignment::Matched {
                    old_idx: 2,
                    new_idx: 1
                },
                Alignment::Inserted { new_idx: 2 },
            ]
        );
    }
}
