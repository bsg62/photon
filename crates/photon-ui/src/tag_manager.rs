//! Settings' Tags: the keywords listed and filtered, a keyword renamed in its row, and the
//! changes made so far - `tags.ts` and `createTagRenamer`, with their tests' cases.
//!
//! A keyword's name is read from the photos and never written to them: a rename and a
//! removal are rules in `library.db` (`library/tags.rs`), applied as the keywords are read,
//! and each one is undone under Changes.

use photon_core::library::{TagCount, TagRule};

pub const HEADING: &str = "Tags";
pub const NO_TAGS: &str = "No tags. Keywords saved in your photos appear here.";
pub const FILTER: &str = "Filter tags";
pub const RENAME: &str = "Rename";
pub const REMOVE: &str = "Remove…";
pub const CHANGES: &str = "Changes";
pub const RESTORE: &str = "Restore";
pub const BLANK: &str = "A tag needs a name.";
/// The buttons of the questions asked in a row: what each does, beside Cancel.
pub const REMOVE_YES: &str = "Remove";
pub const MERGE_YES: &str = "Merge";
pub const RENAME_YES: &str = "Rename";

/// The words under the filter when no keyword holds what it holds.
pub fn no_match(filter: &str) -> String {
    format!("No tag matches “{filter}”.")
}

/// The field's name, for whoever reads the window without seeing it.
pub fn rename_label(tag: &str) -> String {
    format!("New name for {tag}")
}

/// The keywords the filter holds: a substring, without case. Here and not in the
/// library because the whole list is on hand.
pub fn filter_tags<'a>(tags: &'a [TagCount], query: &str) -> Vec<&'a TagCount> {
    let needle = query.trim().to_lowercase();
    (tags.iter())
        .filter(|tag| needle.is_empty() || tag.tag.to_lowercase().contains(&needle))
        .collect()
}

/// A change as Changes lists it.
pub fn rule_label(rule: &TagRule) -> String {
    match &rule.target {
        None => format!("{} — removed", rule.tag),
        Some(target) => format!("{} → {target}", rule.tag),
    }
}

/// "1 photo", "1,234 photos": `photoCountLabel`.
pub fn photos(count: i64) -> String {
    if count == 1 {
        "1 photo".to_owned()
    } else {
        format!(
            "{} photos",
            crate::grid::labels::grouped(usize::try_from(count).unwrap_or(0))
        )
    }
}

/// The question asked before a keyword is removed.
pub fn remove_question(tag: &str) -> String {
    format!(
        "Remove “{tag}” from photon? The keyword stays in your photo files, and you can restore it under Changes."
    )
}

/// What storing a rename would do. A merge asks first, because two keywords become one
/// and only restoring the rule separates them again; so does a name that has a rule of its
/// own (removed, or renamed away), which the library drops, bringing that keyword's photos
/// back under it. Names compare exactly, as the library does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenameCheck {
    Blank,
    Same,
    Merge,
    Revive,
    Ok,
}

pub fn rename_check(from: &str, to: &str, existing: &[TagCount], rules: &[TagRule]) -> RenameCheck {
    let name = to.trim();
    if name.is_empty() {
        RenameCheck::Blank
    } else if name == from {
        RenameCheck::Same
    } else if rules.iter().any(|rule| rule.tag == name) {
        RenameCheck::Revive
    } else if existing.iter().any(|tag| tag.tag == name) {
        RenameCheck::Merge
    } else {
        RenameCheck::Ok
    }
}

/// The question a merge or a revive asks.
pub fn rename_question(check: RenameCheck, from: &str, to: &str) -> Option<String> {
    match check {
        RenameCheck::Merge => Some(format!(
            "Merge “{from}” into “{to}”? Photos tagged with either will show under “{to}”."
        )),
        RenameCheck::Revive => Some(format!(
            "“{to}” is listed under Changes. Renaming onto it undoes that change and shows its photos and those tagged “{from}” under “{to}”."
        )),
        RenameCheck::Blank | RenameCheck::Same | RenameCheck::Ok => None,
    }
}

/// A rename to store: from, to, and the number its field was sent under.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rename {
    pub from: String,
    pub to: String,
    pub sent: u64,
}

/// What a commit of the field came to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Committed {
    /// Nothing to do: the field closed (the name unchanged), or stays with its error.
    Nothing,
    /// The row asks first: a merge, or a name listed under Changes.
    Asks(String),
    Store(Rename),
}

/// The row a keyword is renamed in: one at a time. The field stays open until the rename
/// has been stored - a declined merge and a refusal leave the typed name in place, the
/// library's words under it for the second - and while a rename is on its way it takes no
/// typing, no second commit and is not closed by being left.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Renamer {
    editing: Option<String>,
    text: String,
    error: String,
    /// The question the row asks, while it asks.
    asking: Option<String>,
    /// The number of the rename on its way, if one is.
    sent: Option<u64>,
    sends: u64,
}

impl Renamer {
    /// The keyword whose field is open.
    pub fn editing(&self) -> Option<&str> {
        self.editing.as_deref()
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn error(&self) -> &str {
        &self.error
    }

    pub fn asking(&self) -> Option<&str> {
        self.asking.as_deref()
    }

    /// Whether a rename is on its way.
    pub fn busy(&self) -> bool {
        self.sent.is_some()
    }

    /// Opens the field, seeded with the keyword's name: a small correction is a few
    /// keystrokes. Not while a rename is on its way.
    pub fn start(&mut self, tag: &str) {
        if self.busy() {
            return;
        }
        self.editing = Some(tag.to_owned());
        self.text = tag.to_owned();
        self.error.clear();
        self.asking = None;
    }

    /// Typing clears what was said: it was about the name as it was.
    pub fn type_in(&mut self, text: &str) {
        if self.busy() || self.asking.is_some() {
            return;
        }
        self.text = text.to_owned();
        self.error.clear();
    }

    /// Escape, or the field left. Not while a rename is on its way, nor under the
    /// question, which is what took the keyboard from the field.
    pub fn cancel(&mut self) {
        if self.busy() || self.asking.is_some() {
            return;
        }
        self.editing = None;
        self.error.clear();
    }

    /// Enter: what the name typed comes to. `existing` and `rules` are what the user can
    /// see, which is what decides whether to ask first.
    pub fn commit(&mut self, existing: &[TagCount], rules: &[TagRule]) -> Committed {
        let Some(from) = self.editing.clone() else {
            return Committed::Nothing;
        };
        if self.busy() || self.asking.is_some() {
            return Committed::Nothing;
        }
        let check = rename_check(&from, &self.text, existing, rules);
        match check {
            RenameCheck::Blank => {
                self.error = BLANK.to_owned();
                Committed::Nothing
            }
            RenameCheck::Same => {
                self.editing = None;
                self.error.clear();
                Committed::Nothing
            }
            RenameCheck::Merge | RenameCheck::Revive => {
                let question = rename_question(check, &from, self.text.trim())
                    .expect("a merge and a revive ask");
                self.asking = Some(question.clone());
                Committed::Asks(question)
            }
            RenameCheck::Ok => Committed::Store(self.send(from)),
        }
    }

    /// The question answered with yes: the rename to store.
    pub fn agreed(&mut self) -> Option<Rename> {
        self.asking.take()?;
        let from = self.editing.clone()?;
        Some(self.send(from))
    }

    /// The question answered with no: the typed name stays, to be put right.
    pub fn declined(&mut self) {
        self.asking = None;
    }

    fn send(&mut self, from: String) -> Rename {
        self.sends += 1;
        self.sent = Some(self.sends);
        Rename {
            from,
            to: self.text.trim().to_owned(),
            sent: self.sends,
        }
    }

    /// The library stored the rename sent under `sent`: its field closes.
    pub fn stored(&mut self, sent: u64) {
        if self.sent == Some(sent) {
            self.sent = None;
            self.editing = None;
            self.error.clear();
        }
    }

    /// The library refused it: the field stays, with its words.
    pub fn refused(&mut self, sent: u64, said: String) {
        if self.sent == Some(sent) {
            self.sent = None;
            self.error = said;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(name: &str, total: i64) -> TagCount {
        TagCount {
            tag: name.to_owned(),
            count: total,
            total,
        }
    }

    fn removed(name: &str) -> TagRule {
        TagRule {
            tag: name.to_owned(),
            target: None,
        }
    }

    fn tags() -> Vec<TagCount> {
        vec![tag("Lisbon", 3), tag("lake", 2), tag("Porto", 1)]
    }

    // The words are the Svelte ones: read out of the component and the module that say
    // them, their lines joined.
    #[test]
    fn the_words_are_the_svelte_ones() {
        let joined = |source: &str| {
            (source.lines())
                .map(str::trim)
                .collect::<Vec<_>>()
                .join(" ")
        };
        let settings = joined(include_str!("../../../ui/src/components/Settings.svelte"));
        for words in [
            NO_TAGS,
            "placeholder=\"Filter tags\"",
            ">Rename</button>",
            ">Remove…</button>",
            "<h2>Changes</h2>",
            ">Restore</button>",
            "No tag matches “{tagFilter}”.",
            "aria-label=\"New name for {tag.tag}\"",
            "`Remove “${tag.tag}” from photon? The keyword stays in your photo files, and you can restore it under Changes.`",
        ] {
            assert!(
                settings.contains(words),
                "Settings.svelte no longer says {words:?}"
            );
        }
        let renamer = joined(include_str!("../../../ui/src/lib/tag-renamer.svelte.ts"));
        for words in [
            "'A tag needs a name.'",
            "`Merge “${from}” into “${to}”? Photos tagged with either will show under “${to}”.`",
            "`“${to}” is listed under Changes. Renaming onto it undoes that change and shows its photos and those tagged “${from}” under “${to}”.`",
        ] {
            assert!(
                renamer.contains(words),
                "tag-renamer.svelte.ts no longer says {words:?}"
            );
        }
        let module = joined(include_str!("../../../ui/src/lib/tags.ts"));
        assert!(module.contains("`${rule.tag} — removed` : `${rule.tag} → ${rule.target}`"));
    }

    // `tags.test.ts`'s cases.
    #[test]
    fn the_filter_is_a_substring_without_case() {
        let all = tags();
        let names = |query| {
            (filter_tags(&all, query).iter())
                .map(|tag| tag.tag.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(names(""), ["Lisbon", "lake", "Porto"]);
        assert_eq!(names("  "), ["Lisbon", "lake", "Porto"]);
        assert_eq!(names("L"), ["Lisbon", "lake"]);
        assert_eq!(names(" por "), ["Porto"]);
        assert!(names("x").is_empty());
    }

    #[test]
    fn a_change_says_what_became_of_the_keyword() {
        assert_eq!(rule_label(&removed("Lisbon")), "Lisbon — removed");
        let renamed = TagRule {
            tag: "lisbon".to_owned(),
            target: Some("Lisbon".to_owned()),
        };
        assert_eq!(rule_label(&renamed), "lisbon → Lisbon");
        assert_eq!(photos(1), "1 photo");
        assert_eq!(photos(1234), "1,234 photos");
    }

    #[test]
    fn a_rename_is_checked_against_what_the_user_can_see() {
        let (all, rules) = (tags(), vec![removed("Faro")]);
        assert_eq!(
            rename_check("Lisbon", "  ", &all, &rules),
            RenameCheck::Blank
        );
        assert_eq!(
            rename_check("Lisbon", " Lisbon ", &all, &rules),
            RenameCheck::Same
        );
        assert_eq!(
            rename_check("Lisbon", "Porto", &all, &rules),
            RenameCheck::Merge
        );
        assert_eq!(
            rename_check("Lisbon", "Faro", &all, &rules),
            RenameCheck::Revive
        );
        assert_eq!(
            rename_check("Lisbon", "Lisboa", &all, &rules),
            RenameCheck::Ok
        );
        // Exactly, as the library compares: another case is another keyword.
        assert_eq!(
            rename_check("Lisbon", "porto", &all, &rules),
            RenameCheck::Ok
        );
    }

    // `tag-renamer.svelte.test.ts`'s cases.
    #[test]
    fn a_rename_is_sent_trimmed_and_closes_when_it_is_stored() {
        let mut renamer = Renamer::default();
        renamer.start("Lisbon");
        assert_eq!(
            (renamer.editing(), renamer.text()),
            (Some("Lisbon"), "Lisbon")
        );
        renamer.type_in("  Lisboa ");
        let Committed::Store(rename) = renamer.commit(&tags(), &[]) else {
            panic!("stored");
        };
        assert_eq!(
            (rename.from.as_str(), rename.to.as_str()),
            ("Lisbon", "Lisboa")
        );
        // On its way: no typing, no second commit, not closed by being left.
        renamer.type_in("x");
        assert_eq!(renamer.commit(&tags(), &[]), Committed::Nothing);
        renamer.cancel();
        assert_eq!(renamer.editing(), Some("Lisbon"));
        renamer.stored(rename.sent + 1);
        assert_eq!(renamer.editing(), Some("Lisbon"), "another rename's answer");
        renamer.stored(rename.sent);
        assert_eq!(renamer.editing(), None);
    }

    #[test]
    fn a_blank_name_is_refused_and_an_unchanged_one_closes() {
        let mut renamer = Renamer::default();
        renamer.start("Lisbon");
        renamer.type_in(" ");
        assert_eq!(renamer.commit(&tags(), &[]), Committed::Nothing);
        assert_eq!(
            (renamer.editing(), renamer.error()),
            (Some("Lisbon"), BLANK)
        );
        renamer.type_in("Lisbon");
        assert_eq!(renamer.error(), "", "typing clears what was said");
        assert_eq!(renamer.commit(&tags(), &[]), Committed::Nothing);
        assert_eq!(renamer.editing(), None);
    }

    #[test]
    fn a_merge_asks_and_declined_keeps_the_name_typed() {
        let mut renamer = Renamer::default();
        renamer.start("Lisbon");
        renamer.type_in("Porto");
        let asked = renamer.commit(&tags(), &[]);
        assert_eq!(
            asked,
            Committed::Asks(
                "Merge “Lisbon” into “Porto”? Photos tagged with either will show under “Porto”."
                    .to_owned()
            )
        );
        // Under the question the field is not left by losing the keyboard to it.
        renamer.cancel();
        assert_eq!(renamer.editing(), Some("Lisbon"));
        renamer.declined();
        assert_eq!(
            (renamer.editing(), renamer.text()),
            (Some("Lisbon"), "Porto")
        );
        assert_eq!(renamer.asking(), None);
        renamer.commit(&tags(), &[]);
        let rename = renamer.agreed().expect("agreed");
        assert_eq!(rename.to, "Porto");
    }

    #[test]
    fn a_refused_rename_keeps_the_name_and_says_why() {
        let mut renamer = Renamer::default();
        renamer.start("Lisbon");
        renamer.type_in("Lisboa");
        let Committed::Store(rename) = renamer.commit(&tags(), &[]) else {
            panic!("stored");
        };
        renamer.refused(rename.sent, "disk full".to_owned());
        assert_eq!(
            (
                renamer.editing(),
                renamer.text(),
                renamer.error(),
                renamer.busy()
            ),
            (Some("Lisbon"), "Lisboa", "disk full", false)
        );
    }
}
