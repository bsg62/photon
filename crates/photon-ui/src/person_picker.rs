//! The dialog that names a face, or adds photos to a person: what it acts on, what is
//! typed, which of the people photon knows that narrows to, and what is said afterwards.
//! `person-picker.svelte.ts` and the lines of `people.ts` it uses, with their tests' cases.
//!
//! What it acts on is taken as it opens, as every dialog's subject is: the face the menu
//! was opened on, or the photos that were selected then.

use crate::grid::labels::grouped;
use photon_core::library::{NamedItems, NamedPerson, Skipped};

/// The field's name and its placeholder, and the button that writes what is typed.
pub const FIELD: &str = "Name";
pub const BUTTON: &str = "Name";
/// Under the field, while nothing is typed.
pub const HINT: &str = "Type a name, or choose someone below.";
/// In the list's place.
pub const NO_ONE: &str = "No one is named yet.";
pub const NO_MATCH: &str = "No one matches what you typed.";
/// What the list of people says it is, to whoever reads the window without seeing it.
pub const LIST: &str = "People";

/// What a name is written to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// One face, by its `detected_faces.id`: "Name this face…" in the viewer.
    Face(i64),
    /// Photos, each to be named by its one unnamed face: "Add to a person…" in the grid.
    Items(Vec<i64>),
}

/// A name to write, and what to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Submit {
    pub target: Target,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PersonPicker {
    target: Target,
    draft: String,
}

/// What a typed name will do, by the library's own rule (`people.rs`, `clean` and
/// `same_name`): trimmed, and compared without case.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NameChoice {
    Empty,
    New(String),
    /// A name someone already has, in any case, is that person and not a second one.
    Merge {
        id: i64,
        name: String,
    },
}

pub fn name_choice(typed: &str, people: &[NamedPerson]) -> NameChoice {
    let name = typed.trim();
    if name.is_empty() {
        return NameChoice::Empty;
    }
    let key = name.to_lowercase();
    match people
        .iter()
        .find(|person| person.name.to_lowercase() == key)
    {
        Some(person) => NameChoice::Merge {
            id: person.id,
            name: person.name.clone(),
        },
        None => NameChoice::New(name.to_owned()),
    }
}

impl PersonPicker {
    pub fn new(target: Target) -> Self {
        Self {
            target,
            draft: String::new(),
        }
    }

    pub fn target(&self) -> &Target {
        &self.target
    }

    pub fn draft(&self) -> &str {
        &self.draft
    }

    pub fn type_in(&mut self, text: String) {
        self.draft = text;
    }

    /// What the write will touch: the grid behind the dialog cannot be counted by eye.
    pub fn title(&self) -> String {
        match &self.target {
            Target::Face(_) => "Who is this?".to_owned(),
            Target::Items(items) => format!("Add {} to…", photos(items.len())),
        }
    }

    /// The people what is typed narrows to, the best match first: the very name, then
    /// those that begin with it, then the rest that hold it - each in the order given.
    /// All of them while nothing is typed.
    pub fn suggestions<'a>(&self, people: &'a [NamedPerson]) -> Vec<&'a NamedPerson> {
        let typed = self.draft.trim().to_lowercase();
        if typed.is_empty() {
            return people.iter().collect();
        }
        let rank = |name: &str| {
            let name = name.to_lowercase();
            if name == typed {
                Some(0)
            } else if name.starts_with(&typed) {
                Some(1)
            } else {
                name.contains(&typed).then_some(2)
            }
        };
        let mut ranked: Vec<(u8, &NamedPerson)> = (people.iter())
            .filter_map(|person| Some((rank(&person.name)?, person)))
            .collect();
        ranked.sort_by_key(|(rank, _)| *rank);
        ranked.into_iter().map(|(_, person)| person).collect()
    }

    /// What Enter will do, from `name_choice`, so that the line and the library agree.
    pub fn hint(&self, people: &[NamedPerson]) -> String {
        match name_choice(&self.draft, people) {
            NameChoice::Empty => String::new(),
            NameChoice::New(name) => format!("New person “{name}”"),
            NameChoice::Merge { name, .. } => format!("Add to {name}"),
        }
    }

    /// The name to write: `row`, a person of the list, or - none - what is typed. None
    /// for a blank name, which writes nothing and leaves the dialog as it is.
    pub fn submit(&self, row: Option<&str>) -> Option<Submit> {
        let name = row.unwrap_or(&self.draft).trim();
        (!name.is_empty()).then(|| Submit {
            target: self.target.clone(),
            name: name.to_owned(),
        })
    }
}

fn photos(count: usize) -> String {
    if count == 1 {
        "1 photo".to_owned()
    } else {
        format!("{} photos", grouped(count))
    }
}

fn count(number: i64) -> usize {
    usize::try_from(number).unwrap_or(0)
}

/// What is said after a face was named from the viewer.
pub fn named_face_message(name: &str) -> String {
    format!("This is {name}.")
}

/// What is said when the face a name was typed for is no longer there.
pub const FACE_GONE: &str = "That face is no longer there.";

/// What is said after photos were named: what was done, then why the rest were not, each
/// reason on its own, so that someone who selected forty photos can tell which to look
/// at.
pub fn named_items_message(result: &NamedItems) -> String {
    let name = &result.name;
    let mut parts = vec![match count(result.named) {
        0 => format!("Nothing was added to {name}."),
        named => format!("Added {} to {name}.", photos(named)),
    }];
    // A kind the user settles in the viewer: how many, why, which, and what to do there.
    let mut to_open = |skipped: &Skipped, why: &str| {
        let total = count(skipped.count);
        if total == 0 {
            return;
        }
        let (have, them) = if total == 1 {
            ("has", "it")
        } else {
            ("have", "them")
        };
        let mut names: Vec<String> = (skipped.items.iter())
            .map(|item| item.file_name.clone())
            .collect();
        let more = total.saturating_sub(names.len());
        if more > 0 && !names.is_empty() {
            names.push(format!("and {} more", grouped(more)));
        }
        let listed = if names.is_empty() {
            String::new()
        } else {
            format!(": {}", names.join(", "))
        };
        parts.push(format!(
            "{} {have} {why}{listed} \u{2014} open {them} to choose the face.",
            grouped(total)
        ));
    };
    to_open(&result.several, "more than one unnamed face");
    to_open(&result.rejected, &format!("a face you said is not {name}"));
    let already = count(result.already.count);
    if already > 0 {
        let are = if already == 1 { "is" } else { "are" };
        parts.push(format!("{} {are} already {name}'s.", grouped(already)));
    }
    // "Unnamed", not "no face photon found": a photo whose faces are all someone's
    // already, or all ignored, or all Picasa's under a name, has faces and none to name.
    let none = count(result.none.count);
    if none > 0 {
        let have = if none == 1 { "has" } else { "have" };
        parts.push(format!("{} {have} no unnamed face.", grouped(none)));
    }
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use photon_core::library::SkippedItem;

    fn person(id: i64, name: &str) -> NamedPerson {
        NamedPerson {
            id,
            name: name.to_owned(),
        }
    }

    fn typing(text: &str) -> PersonPicker {
        let mut picker = PersonPicker::new(Target::Face(1));
        picker.type_in(text.to_owned());
        picker
    }

    #[test]
    fn it_opens_on_what_it_was_given_with_nothing_typed() {
        let picker = PersonPicker::new(Target::Items(vec![1, 2]));
        assert_eq!(picker.target(), &Target::Items(vec![1, 2]));
        assert_eq!(picker.draft(), "");
    }

    #[test]
    fn the_title_says_what_it_acts_on() {
        let title = |target| PersonPicker::new(target).title();
        assert_eq!(title(Target::Face(7)), "Who is this?");
        assert_eq!(title(Target::Items(vec![4])), "Add 1 photo to…");
        assert_eq!(title(Target::Items(vec![1, 2, 3])), "Add 3 photos to…");
        assert_eq!(title(Target::Items(vec![0; 1200])), "Add 1,200 photos to…");
    }

    #[test]
    fn what_is_typed_narrows_the_people_and_puts_the_best_match_first() {
        let people = [
            person(1, "Hannah"),
            person(2, "Anna"),
            person(3, "Annabel"),
            person(4, "Ben"),
            person(5, "Émile"),
        ];
        let ids = |text: &str| -> Vec<i64> {
            (typing(text).suggestions(&people).iter())
                .map(|person| person.id)
                .collect()
        };
        assert_eq!(ids("ANNA"), [2, 3, 1]);
        assert_eq!(ids("émile"), [5]);
        assert_eq!(ids("ÉMILE"), [5], "without case, beyond ASCII too");
        assert_eq!(
            ids(" anna "),
            [2, 3, 1],
            "the spaces around it are not part of it"
        );
        assert_eq!(ids(""), [1, 2, 3, 4, 5]);
        assert_eq!(ids("   "), [1, 2, 3, 4, 5]);
        assert_eq!(ids("zed"), Vec::<i64>::new());
        // Of two that match as well, the one that comes first in the list.
        let twins = [person(1, "Anna B"), person(2, "Anna A")];
        let first: Vec<i64> = (typing("anna").suggestions(&twins).iter())
            .map(|person| person.id)
            .collect();
        assert_eq!(first, [1, 2]);
    }

    #[test]
    fn the_hint_says_whether_enter_adds_or_makes_someone() {
        let people = [person(3, "Anna")];
        assert_eq!(typing("anna").hint(&people), "Add to Anna");
        assert_eq!(typing(" Ben ").hint(&people), "New person “Ben”");
        assert_eq!(typing("  ").hint(&people), "");
    }

    #[test]
    fn a_name_someone_has_in_any_case_is_that_person() {
        let people = [person(3, "Anna"), person(4, "Émile")];
        let merge = |id, name: &str| NameChoice::Merge {
            id,
            name: name.to_owned(),
        };
        assert_eq!(name_choice("ANNA", &people), merge(3, "Anna"));
        assert_eq!(name_choice("  émile ", &people), merge(4, "Émile"));
        assert_eq!(name_choice("ÉMILE", &people), merge(4, "Émile"));
        assert_eq!(
            name_choice("Ann", &people),
            NameChoice::New("Ann".to_owned())
        );
        assert_eq!(name_choice("", &people), NameChoice::Empty);
        assert_eq!(name_choice(" \t", &people), NameChoice::Empty);
    }

    #[test]
    fn what_is_written_is_the_row_or_what_is_typed_and_never_a_blank_name() {
        let face = |name: &str| {
            Some(Submit {
                target: Target::Face(1),
                name: name.to_owned(),
            })
        };
        assert_eq!(typing(" Ben ").submit(None), face("Ben"));
        assert_eq!(typing("an").submit(Some("Anna")), face("Anna"));
        assert_eq!(typing("").submit(Some("Anna")), face("Anna"));
        assert_eq!(typing("  ").submit(None), None);
        assert_eq!(typing("").submit(None), None);
        assert_eq!(typing("Ben").submit(Some("  ")), None);
        // To the photos it was opened for.
        let mut photos = PersonPicker::new(Target::Items(vec![1, 2, 3]));
        photos.type_in("Anna".to_owned());
        assert_eq!(
            photos.submit(None),
            Some(Submit {
                target: Target::Items(vec![1, 2, 3]),
                name: "Anna".to_owned(),
            })
        );
    }

    fn files(names: &[&str]) -> Vec<SkippedItem> {
        (names.iter().enumerate())
            .map(|(id, name)| SkippedItem {
                id: id as i64,
                file_name: (*name).to_owned(),
            })
            .collect()
    }

    fn skipped(names: &[&str], count: i64) -> Skipped {
        Skipped {
            items: files(names),
            count,
        }
    }

    fn result() -> NamedItems {
        NamedItems {
            person: Some(1),
            name: "Anna".to_owned(),
            named: 5,
            already: Skipped::default(),
            several: Skipped::default(),
            rejected: Skipped::default(),
            none: Skipped::default(),
        }
    }

    #[test]
    fn a_face_named_is_said_by_the_name() {
        assert_eq!(named_face_message("Anna"), "This is Anna.");
    }

    #[test]
    fn it_says_what_was_added() {
        assert_eq!(named_items_message(&result()), "Added 5 photos to Anna.");
        let one = NamedItems {
            named: 1,
            ..result()
        };
        assert_eq!(named_items_message(&one), "Added 1 photo to Anna.");
    }

    #[test]
    fn the_photos_with_several_unnamed_faces_are_listed() {
        let several = NamedItems {
            several: skipped(&["IMG_1.jpg", "IMG_2.jpg"], 2),
            ..result()
        };
        assert_eq!(
            named_items_message(&several),
            "Added 5 photos to Anna. 2 have more than one unnamed face: IMG_1.jpg, \
             IMG_2.jpg \u{2014} open them to choose the face."
        );
        // More than were listed by name.
        let many = NamedItems {
            several: skipped(&["a.jpg", "b.jpg"], 5),
            ..result()
        };
        assert!(
            named_items_message(&many)
                .contains("5 have more than one unnamed face: a.jpg, b.jpg, and 3 more \u{2014}")
        );
        // And with no name kept at all.
        let unlisted = NamedItems {
            several: skipped(&[], 5),
            ..result()
        };
        assert_eq!(
            named_items_message(&unlisted),
            "Added 5 photos to Anna. 5 have more than one unnamed face \u{2014} open them \
             to choose the face."
        );
    }

    #[test]
    fn the_ones_that_are_theirs_already_and_the_ones_with_no_face_to_name_are_counted() {
        let already = NamedItems {
            already: skipped(&["a"], 1),
            ..result()
        };
        assert_eq!(
            named_items_message(&already),
            "Added 5 photos to Anna. 1 is already Anna's."
        );
        let two = NamedItems {
            already: skipped(&["a", "b"], 2),
            ..result()
        };
        assert_eq!(
            named_items_message(&two),
            "Added 5 photos to Anna. 2 are already Anna's."
        );
        let none = NamedItems {
            none: skipped(&["a"], 1),
            ..result()
        };
        assert_eq!(
            named_items_message(&none),
            "Added 5 photos to Anna. 1 has no unnamed face."
        );
        let none = NamedItems {
            none: skipped(&["a", "b"], 2),
            ..result()
        };
        assert_eq!(
            named_items_message(&none),
            "Added 5 photos to Anna. 2 have no unnamed face."
        );
    }

    #[test]
    fn a_face_the_user_said_is_not_theirs_is_left_for_the_viewer() {
        let one = NamedItems {
            named: 0,
            rejected: skipped(&["IMG_1.jpg"], 1),
            ..result()
        };
        assert_eq!(
            named_items_message(&one),
            "Nothing was added to Anna. 1 has a face you said is not Anna: IMG_1.jpg \
             \u{2014} open it to choose the face."
        );
        let two = NamedItems {
            rejected: skipped(&["a.jpg", "b.jpg"], 2),
            ..result()
        };
        assert_eq!(
            named_items_message(&two),
            "Added 5 photos to Anna. 2 have a face you said is not Anna: a.jpg, b.jpg \
             \u{2014} open them to choose the face."
        );
    }

    #[test]
    fn nothing_added_is_said_so_with_every_reason_in_its_order() {
        let none = NamedItems {
            named: 0,
            none: skipped(&["a"], 1),
            ..result()
        };
        assert_eq!(
            named_items_message(&none),
            "Nothing was added to Anna. 1 has no unnamed face."
        );
        let all = NamedItems {
            named: 2,
            several: skipped(&["s.jpg"], 1),
            rejected: skipped(&["r.jpg"], 1),
            already: skipped(&[], 1200),
            none: skipped(&[], 3),
            ..result()
        };
        assert_eq!(
            named_items_message(&all),
            "Added 2 photos to Anna. 1 has more than one unnamed face: s.jpg \u{2014} open \
             it to choose the face. 1 has a face you said is not Anna: r.jpg \u{2014} open \
             it to choose the face. 1,200 are already Anna's. 3 have no unnamed face."
        );
    }
}
