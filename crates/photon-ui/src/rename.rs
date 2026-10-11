//! The one field the sidebar has room for: a name being typed in the place of an album's
//! row, a saved search's, a folder's, or of "New album…". `createAlbumEditor` in Rust.
//!
//! The Svelte sidebar has three of these, because its lists key their rows by ids that
//! collide - album 3, search 3 and folder 3 - and one editor shared between them would
//! open a field in each. Here the field is keyed by what it stands in for (`Of`), so it
//! is one: a rename begun while another name is half typed takes its place by being the
//! only field there is.
//!
//! Storing a name is a write on the application's queue, and its answer comes frames
//! later. Meanwhile the field is still there, takes no typing and keeps the keyboard:
//! a field that gave the keyboard up would send the next name typed to the grid, where
//! `h` hides a photo. And the answer closes the field only if it is still the field that
//! was sent - a folder's write waits for a rebuild, long enough for another row's rename
//! to have begun.

/// What a name is being typed for.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Of {
    /// An album that does not exist yet: "New album…".
    NewAlbum,
    Album(i64),
    Search(i64),
    /// A folder's name in photon. The directory keeps its own.
    Folder(i64),
}

/// A name to store, and the number its answer is to come back under.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Commit {
    pub of: Of,
    /// Trimmed. Empty only for a folder, where it asks for the directory's own name
    /// back.
    pub name: String,
    pub sent: u64,
}

#[derive(Debug)]
struct Field {
    of: Of,
    text: String,
    /// The number its write was sent under, while that write runs.
    sent: Option<u64>,
}

#[derive(Debug, Default)]
pub struct Rename {
    field: Option<Field>,
    /// How many names have been sent: the next one's number.
    sends: u64,
}

impl Rename {
    /// The field that is open: what it stands in for, and what it holds.
    pub fn field(&self) -> Option<(&Of, &str)> {
        (self.field.as_ref()).map(|field| (&field.of, field.text.as_str()))
    }

    /// Whether the field is open in this entry's place.
    pub fn editing(&self, of: &Of) -> bool {
        (self.field.as_ref()).is_some_and(|field| &field.of == of)
    }

    /// Whether the open field's name is being stored.
    pub fn busy(&self) -> bool {
        (self.field.as_ref()).is_some_and(|field| field.sent.is_some())
    }

    /// Opens the field in `of`'s place with `name` in it, so that a small correction is
    /// a few keys and not the whole name again; for a new album, with nothing. A field
    /// that was open elsewhere is gone, with what was typed in it: the sidebar has room
    /// for one.
    pub fn start(&mut self, of: Of, name: &str) {
        self.field = Some(Field {
            of,
            text: name.to_owned(),
            sent: None,
        });
    }

    /// What the field holds now. Not while its name is being stored: what is typed
    /// then is dropped, the name on its way being the one that was sent.
    pub fn type_in(&mut self, text: String) {
        if let Some(field) = (self.field.as_mut()).filter(|field| field.sent.is_none()) {
            field.text = text;
        }
    }

    /// Escape: the field closes, and what was typed is not stored. A name already on
    /// its way is stored all the same - a write cannot be taken back - and its answer
    /// finds no field to close.
    pub fn cancel(&mut self) {
        self.field = None;
    }

    /// Enter, or a press somewhere else: the name is to be stored. None when there is
    /// nothing to send - no field, a name already on its way, or a blank name, which is
    /// a cancel: Escape and an empty Enter should feel the same. But a folder's blank
    /// name is sent, as the empty name: it is how the user asks for the directory's own
    /// name back.
    pub fn commit(&mut self) -> Option<Commit> {
        let field = (self.field.as_mut()).filter(|field| field.sent.is_none())?;
        let name = field.text.trim();
        if name.is_empty() && !matches!(field.of, Of::Folder(_)) {
            self.field = None;
            return None;
        }
        self.sends += 1;
        field.sent = Some(self.sends);
        Some(Commit {
            of: field.of.clone(),
            name: name.to_owned(),
            sent: self.sends,
        })
    }

    /// The name sent under `sent` was stored: its field closes - if it is still the
    /// field that is open, and not one begun since.
    pub fn landed(&mut self, sent: u64) {
        if (self.field.as_ref()).is_some_and(|field| field.sent == Some(sent)) {
            self.field = None;
        }
    }

    /// The library refused the name sent under `sent`: its field stays open with the
    /// name in it, to be put right - if it is still the field that is open.
    pub fn refused(&mut self, sent: u64) {
        if let Some(field) = (self.field.as_mut()).filter(|field| field.sent == Some(sent)) {
            field.sent = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(of: Of, name: &str, sent: u64) -> Option<Commit> {
        Some(Commit {
            of,
            name: name.to_owned(),
            sent,
        })
    }

    // album-editor.svelte.test.ts, case for case.

    #[test]
    fn a_new_album_is_made_from_a_trimmed_name_and_the_field_closes_when_it_is_stored() {
        let mut rename = Rename::default();
        assert_eq!(rename.field(), None);
        rename.start(Of::NewAlbum, "");
        assert!(rename.editing(&Of::NewAlbum));
        assert_eq!(rename.field(), Some((&Of::NewAlbum, "")));
        rename.type_in("  Lisbon ".to_owned());
        assert_eq!(rename.commit(), commit(Of::NewAlbum, "Lisbon", 1));
        // Still there while it is stored.
        assert!(rename.editing(&Of::NewAlbum));
        assert!(rename.busy());
        rename.landed(1);
        assert_eq!(rename.field(), None);
        assert!(!rename.busy());
    }

    #[test]
    fn a_rename_opens_with_the_name_in_it() {
        let mut rename = Rename::default();
        rename.start(Of::Album(4), "Lisbon");
        assert_eq!(rename.field(), Some((&Of::Album(4), "Lisbon")));
        assert!(rename.editing(&Of::Album(4)));
        assert!(!rename.editing(&Of::NewAlbum));
        assert!(!rename.editing(&Of::Album(5)));
        rename.type_in("Lisboa".to_owned());
        assert_eq!(rename.commit(), commit(Of::Album(4), "Lisboa", 1));
    }

    #[test]
    fn a_blank_name_is_a_cancel_and_sends_nothing() {
        for of in [Of::NewAlbum, Of::Album(4), Of::Search(4)] {
            for blank in ["", "   "] {
                let mut rename = Rename::default();
                rename.start(of.clone(), "Lisbon");
                rename.type_in(blank.to_owned());
                assert_eq!(rename.commit(), None, "{of:?} {blank:?}");
                assert_eq!(rename.field(), None, "{of:?} {blank:?}");
            }
        }
        // With no field open there is nothing to send either.
        assert_eq!(Rename::default().commit(), None);
    }

    // A folder's name in photon can be taken away: the directory's own stands in.
    #[test]
    fn a_folders_blank_name_is_sent_as_the_empty_name() {
        let mut rename = Rename::default();
        rename.start(Of::Folder(7), "Summer");
        rename.type_in("  ".to_owned());
        assert_eq!(rename.commit(), commit(Of::Folder(7), "", 1));
        assert!(rename.busy());
    }

    // A folder's write waits for a rebuild, long enough for the press that sent it to
    // be followed by a rename of another row.
    #[test]
    fn a_field_begun_while_a_name_is_stored_stays_open_when_that_name_lands() {
        let mut rename = Rename::default();
        rename.start(Of::Folder(7), "Summer");
        let first = rename.commit().unwrap();
        rename.start(Of::Folder(8), "Winter");
        assert!(!rename.busy());
        rename.landed(first.sent);
        assert_eq!(rename.field(), Some((&Of::Folder(8), "Winter")));
        // And is not closed by the first one's refusal either, nor made to look sent.
        rename.type_in("Winter 2024".to_owned());
        let second = rename.commit().unwrap();
        assert_eq!(second.sent, 2);
        rename.refused(first.sent);
        assert!(rename.busy());
        rename.landed(second.sent);
        assert_eq!(rename.field(), None);
    }

    #[test]
    fn a_name_the_library_refuses_leaves_the_field_open_to_be_put_right() {
        let mut rename = Rename::default();
        rename.start(Of::Album(4), "Lisbon");
        rename.type_in("Porto".to_owned());
        let sent = rename.commit().unwrap().sent;
        rename.refused(sent);
        assert_eq!(rename.field(), Some((&Of::Album(4), "Porto")));
        assert!(!rename.busy());
        // Put right and sent again, under another number.
        rename.type_in("Porto 2024".to_owned());
        assert_eq!(rename.commit(), commit(Of::Album(4), "Porto 2024", 2));
    }

    #[test]
    fn a_rename_takes_the_place_of_a_new_album_half_typed() {
        let mut rename = Rename::default();
        rename.start(Of::NewAlbum, "");
        rename.type_in("Lis".to_owned());
        rename.start(Of::Album(4), "Porto");
        assert!(!rename.editing(&Of::NewAlbum));
        assert_eq!(rename.field(), Some((&Of::Album(4), "Porto")));
    }

    #[test]
    fn escape_closes_the_field_and_stores_nothing() {
        let mut rename = Rename::default();
        rename.start(Of::Album(4), "Lisbon");
        rename.type_in("Porto".to_owned());
        rename.cancel();
        assert_eq!(rename.field(), None);
        assert_eq!(rename.commit(), None);
    }

    // What the Svelte editor's one field per list could not get wrong, and this one
    // could.

    #[test]
    fn the_same_number_in_two_lists_is_two_things() {
        let mut rename = Rename::default();
        rename.start(Of::Album(3), "Lisbon");
        assert!(rename.editing(&Of::Album(3)));
        assert!(!rename.editing(&Of::Search(3)));
        assert!(!rename.editing(&Of::Folder(3)));
    }

    #[test]
    fn while_a_name_is_stored_nothing_is_typed_and_nothing_sent() {
        let mut rename = Rename::default();
        rename.start(Of::Album(4), "Lisbon");
        rename.type_in("Porto".to_owned());
        let sent = rename.commit().unwrap().sent;
        rename.type_in("Porto and more".to_owned());
        assert_eq!(rename.field(), Some((&Of::Album(4), "Porto")));
        // A second Enter, or the press elsewhere that follows the first.
        assert_eq!(rename.commit(), None);
        assert!(rename.editing(&Of::Album(4)));
        assert!(rename.busy());
        rename.landed(sent);
        assert_eq!(rename.field(), None);
    }

    // The write is on its way and cannot be taken back; the field can go.
    #[test]
    fn escape_closes_a_field_whose_name_is_being_stored_and_its_answer_finds_none() {
        let mut rename = Rename::default();
        rename.start(Of::Album(4), "Lisbon");
        let sent = rename.commit().unwrap().sent;
        rename.cancel();
        assert_eq!(rename.field(), None);
        assert!(!rename.busy());
        rename.refused(sent);
        rename.landed(sent);
        assert_eq!(rename.field(), None);
    }

    #[test]
    fn an_answer_for_no_field_or_another_number_changes_nothing() {
        let mut rename = Rename::default();
        rename.landed(1);
        rename.refused(1);
        assert_eq!(rename.field(), None);
        rename.start(Of::Album(4), "Lisbon");
        // Not sent yet: no number is its own.
        rename.landed(0);
        rename.landed(1);
        assert!(rename.editing(&Of::Album(4)));
        let sent = rename.commit().unwrap().sent;
        rename.landed(sent + 1);
        rename.refused(sent + 1);
        assert!(rename.busy());
    }
}
