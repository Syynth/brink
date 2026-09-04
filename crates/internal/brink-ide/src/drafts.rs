//! Draft files — `[project] drafts` globs against the compile closure.
//!
//! The rule (decision log 2026-08-27, "reachability wins"):
//!
//! ```text
//! draft(file) := matches(file, [project] drafts) && !reachable_from_entry(file)
//! ```
//!
//! Both halves come from different places — the config and the compile
//! closure — so this computes them together rather than letting a caller
//! reassemble them and be free to disagree. It lives here, at the session
//! layer, because every consumer needs the same answer: the wasm
//! `EditorSession` binds it for the web studio, and a native host asks
//! [`IdeSession`] for it directly.
//!
//! Before the first analysis the closure is empty, so nothing is known to be
//! unreachable yet and no file is a draft. Reporting every glob match during
//! that window would flash draft marks onto files that turn out to be part
//! of the story.

use std::collections::HashSet;

use crate::session::IdeSession;

/// What one authored glob is actually doing.
///
/// A bare list of glob strings hides two ordinary author mistakes: a glob
/// that matches **nothing** (a typo, or a folder since renamed) looks
/// identical to one that is working, and a glob matching a file the entry
/// still reaches did not make it a draft — "reachability wins" — which
/// without this split would read as though the glob had taken effect.
///
/// Attribution, not a partition: a file matching two globs is listed under
/// both.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftGlob {
    /// The glob as the author wrote it.
    pub glob: String,
    /// Matched, and outside the compile closure — actually drafts.
    pub drafts: Vec<String>,
    /// Matched, but the entry still reaches them, so they are not drafts.
    pub in_story: Vec<String>,
}

/// Per-glob attribution for the Drafts settings view (#3145).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftGlobReport {
    /// False before the first analysis, when the closure is empty and
    /// nothing is known to be unreachable. Every list is then empty too, and
    /// a caller should say "not known yet" rather than "matches nothing" —
    /// those look identical in the data and mean opposite things.
    pub compiled: bool,
    /// In the order the author wrote them.
    pub globs: Vec<DraftGlob>,
}

impl IdeSession {
    /// The project's draft files, sorted.
    ///
    /// Sorted so a caller that memoizes on the list is not churned by an
    /// unstable order across calls that changed nothing.
    #[must_use]
    pub fn draft_paths(&self) -> Vec<String> {
        if self.draft_globs().is_empty() {
            return Vec::new();
        }
        let closure = self.compilation_closure_paths();
        if closure.is_empty() {
            return Vec::new();
        }
        let closure: HashSet<&str> = closure.iter().map(String::as_str).collect();
        let mut drafts: Vec<String> = self
            .author_file_paths()
            .into_iter()
            .filter(|path| !closure.contains(path.as_str()))
            .filter(|path| brink_project_config::globs::matches_any(path, self.draft_globs()))
            .collect();
        drafts.sort();
        drafts
    }

    /// Per-glob attribution — see [`DraftGlobReport`].
    ///
    /// [`Self::draft_paths`] answers "which files are drafts"; this answers
    /// "what is each glob I wrote actually doing", which the settings list
    /// has to show and cannot derive from the first.
    #[must_use]
    pub fn draft_glob_report(&self) -> DraftGlobReport {
        let closure_paths = self.compilation_closure_paths();
        let compiled = !closure_paths.is_empty();
        // A set, not the `Vec`: every glob tests every file against it, so a
        // linear scan here would be cubic in the project.
        let closure: HashSet<&str> = closure_paths.iter().map(String::as_str).collect();

        // The author's own files, gathered once — every glob is tested
        // against the same set.
        let paths = if compiled {
            self.author_file_paths()
        } else {
            Vec::new()
        };

        let globs = self
            .draft_globs()
            .iter()
            .map(|glob| {
                let mut drafts = Vec::new();
                let mut in_story = Vec::new();
                for path in &paths {
                    if !brink_project_config::globs::matches(path, glob) {
                        continue;
                    }
                    if closure.contains(path.as_str()) {
                        in_story.push(path.clone());
                    } else {
                        drafts.push(path.clone());
                    }
                }
                drafts.sort();
                in_story.sort();
                DraftGlob {
                    glob: glob.clone(),
                    drafts,
                    in_story,
                }
            })
            .collect();

        DraftGlobReport { compiled, globs }
    }

    /// Every tracked file that is the author's own — the mounted stdlib is
    /// never the author's draft, however the globs happen to be spelled.
    fn author_file_paths(&self) -> Vec<String> {
        self.db()
            .file_ids()
            .filter(|id| !self.is_mounted_std(*id))
            .filter_map(|id| self.db().file_path(id).map(str::to_owned))
            .collect()
    }
}
