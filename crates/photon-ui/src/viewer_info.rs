//! What the viewer's info panel says, as text: `exif.ts`, with its tests' cases, and the
//! few lines of `faces.ts` and `copies.ts` the panel shows. The engine stores the raw
//! numbers and derives nothing, so every spelling is pinned here.

use crate::{grid::labels::grouped, viewer_caption};
use jiff::tz::TimeZone;
use photon_core::library::{AlbumSummary, ItemFace};
use photon_engine::commands::{CopyKind, ItemCopy, ItemDates};

/// One line of the panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub label: String,
    pub value: String,
    /// The search this row links to, for the rows that are worth filtering by.
    pub search: Option<String>,
}

fn row(label: &str, value: String, search: Option<String>) -> Row {
    Row {
        label: label.to_owned(),
        value,
        search,
    }
}

/// A search confined to one field, in the grammar of `photon_core::search::Query`. The
/// value is quoted because it holds spaces, and a quote inside it is dropped because the
/// grammar has no escape; a quoted field value is matched word by word, so the lost
/// character costs nothing.
pub fn field_query(field: &str, value: &str) -> String {
    format!("{field}:\"{}\"", value.replace('"', " ").trim())
}

/// A position as people write one: "48.13740° N, 11.57550° E". Five decimals is about a
/// metre, as fine as a camera's fix. The hemisphere's letter carries the sign, and is
/// chosen after the rounding, so that a hair south of the equator is not "0.00000° S".
pub fn format_coordinates(lat: f64, lon: f64) -> String {
    let part = |value: f64, positive: &str, negative: &str| {
        let rounded = format!("{:.5}", value.abs());
        let nought = rounded.parse::<f64>().is_ok_and(|rounded| rounded == 0.0);
        let letter = if value < 0.0 && !nought {
            negative
        } else {
            positive
        };
        format!("{rounded}° {letter}")
    };
    format!("{}, {}", part(lat, "N", "S"), part(lon, "E", "W"))
}

/// The search for photos taken near a point, within the grammar's own distance. Four
/// decimals, about ten metres, keep the query short enough to read in the box.
pub fn near_query(lat: f64, lon: f64) -> String {
    format!("near:{lat:.4},{lon:.4}")
}

/// "Canon EOS 5D Mark IV", not "Canon Canon EOS 5D Mark IV": most makers repeat the make
/// in the model, and Nikon writes "NIKON CORPORATION" as the make and "NIKON D750" as the
/// model. The model wins whenever it names the maker's first word already.
pub fn camera_name(make: Option<&str>, model: Option<&str>) -> Option<String> {
    let make = make.unwrap_or("").trim();
    let model = model.unwrap_or("").trim();
    if model.is_empty() {
        return (!make.is_empty()).then(|| make.to_owned());
    }
    if make.is_empty() {
        return Some(model.to_owned());
    }
    let first = make.split_whitespace().next().unwrap_or("").to_lowercase();
    if !first.is_empty() && model.to_lowercase().contains(&first) {
        return Some(model.to_owned());
    }
    Some(format!("{make} {model}"))
}

/// A number to one decimal only where it has one: "50", "18.5".
fn one_decimal(value: f64) -> String {
    let rounded = (value * 10.0).round() / 10.0;
    if rounded.fract() == 0.0 {
        format!("{rounded:.0}")
    } else {
        format!("{rounded:.1}")
    }
}

/// Millimetres: "50 mm", "18.5 mm".
pub fn format_focal(mm: f64) -> String {
    format!("{} mm", one_decimal(mm))
}

/// "f/1.8", "f/11".
pub fn format_aperture(f: f64) -> String {
    format!("f/{}", one_decimal(f))
}

/// A shutter speed the way the camera shows it: "1/250 s" below a second, "2 s" or
/// "2.5 s" above, and "0.5 s", not "1/2 s", for the slow fractions whose reciprocal is
/// not what anyone reads off a dial. Nothing for a time of none.
pub fn format_exposure(seconds: f64) -> String {
    if seconds <= 0.0 {
        return String::new();
    }
    if seconds >= 1.0 {
        return format!("{} s", one_decimal(seconds));
    }
    let reciprocal = 1.0 / seconds;
    if reciprocal >= 4.0 {
        format!("1/{} s", reciprocal.round())
    } else {
        format!("{:.1} s", (seconds * 10.0).round() / 10.0)
    }
}

pub fn format_iso(iso: i64) -> String {
    format!("ISO {iso}")
}

/// What of a photo the camera's rows are made of.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Camera<'a> {
    pub make: Option<&'a str>,
    pub model: Option<&'a str>,
    pub lens: Option<&'a str>,
    pub focal_mm: Option<f64>,
    pub aperture: Option<f64>,
    pub exposure_s: Option<f64>,
    pub iso: Option<i64>,
}

/// The camera's rows, in the order a photographer reads them. A field the camera did not
/// write is simply absent, so a scan with no EXIF has no rows at all.
pub fn camera_rows(of: Camera<'_>) -> Vec<Row> {
    let mut rows = Vec::new();
    if let Some(camera) = camera_name(of.make, of.model) {
        let search = field_query("camera", &camera);
        rows.push(row("Camera", camera, Some(search)));
    }
    if let Some(lens) = of.lens.filter(|lens| !lens.is_empty()) {
        rows.push(row(
            "Lens",
            lens.to_owned(),
            Some(field_query("lens", lens)),
        ));
    }
    let exposure: Vec<String> = [
        of.focal_mm.filter(|mm| *mm != 0.0).map(format_focal),
        of.aperture.filter(|f| *f != 0.0).map(format_aperture),
        (of.exposure_s.filter(|s| *s != 0.0)).map(format_exposure),
        of.iso.filter(|iso| *iso != 0).map(format_iso),
    ]
    .into_iter()
    .flatten()
    .collect();
    if !exposure.is_empty() {
        rows.push(row("Exposure", exposure.join(" · "), None));
    }
    rows
}

/// The panel's dates: the camera's, then the file's, each left out when it is absent. A
/// date shown twice in a group is shown once, labelled with every tag that carries it
/// ("Taken, edited") - a camera writes all three of its dates alike, and three rows the
/// same read as a fault. Compared as spelled, so two times the panel cannot tell apart
/// are one row, and never across the groups: the camera's 12:30 and the file's 12:30 are
/// different times. The camera's are its own clock; the file's are instants, read in
/// `zone`, the machine's.
pub fn date_rows(dates: &ItemDates, zone: &TimeZone) -> Vec<Row> {
    let camera = |seconds: Option<i64>| seconds.map(viewer_caption::format_taken);
    let file = |ms: Option<i64>| ms.map(|ms| viewer_caption::format_instant(ms, zone));
    let groups = [
        (
            "",
            vec![
                ("taken", camera(dates.taken)),
                ("digitized", camera(dates.digitized)),
                ("edited", camera(dates.edited)),
            ],
        ),
        (
            "file ",
            vec![
                ("created", file(dates.file_created_ms)),
                ("modified", file(Some(dates.file_modified_ms))),
            ],
        ),
    ];
    let mut rows = Vec::new();
    for (prefix, dated) in groups {
        let mut merged: Vec<(String, Vec<&str>)> = Vec::new();
        for (name, value) in dated {
            let Some(value) = value else { continue };
            match merged.iter_mut().find(|(spelled, _)| *spelled == value) {
                Some((_, names)) => names.push(name),
                None => merged.push((value, vec![name])),
            }
        }
        for (value, names) in merged {
            let label = format!("{prefix}{}", names.join(", "));
            let mut letters = label.chars();
            let label = match letters.next() {
                Some(first) => first.to_uppercase().chain(letters).collect(),
                None => label,
            };
            rows.push(Row {
                label,
                value,
                search: None,
            });
        }
    }
    rows
}

/// "4000 × 3000".
pub fn format_dimensions(width: u32, height: u32) -> String {
    format!("{width} × {height}")
}

/// One section of the panel's list of copies.
#[derive(Debug, PartialEq, Eq)]
pub struct CopyGroup<'a> {
    pub kind: CopyKind,
    pub label: &'static str,
    pub copies: Vec<&'a ItemCopy>,
}

/// The copies in the two sections the panel shows, the identical ones first, each in the
/// order the engine lists them. A section with nothing in it is left out, as the whole
/// list is for a photo with no copy at all.
pub fn copy_groups(copies: &[ItemCopy]) -> Vec<CopyGroup<'_>> {
    [
        (CopyKind::Identical, "Identical"),
        (CopyKind::Similar, "Looks the same"),
    ]
    .into_iter()
    .map(|(kind, label)| CopyGroup {
        kind,
        label,
        copies: copies.iter().filter(|copy| copy.kind == kind).collect(),
    })
    .filter(|group| !group.copies.is_empty())
    .collect()
}

/// The people named on a photo, once each, as their key and their name, in the order
/// their faces come: Picasa and photon can both name one person on a photo, and what the
/// panel shows is a link to their view, not a count.
pub fn people(faces: &[ItemFace]) -> Vec<(&str, &str)> {
    let mut once: Vec<(&str, &str)> = Vec::new();
    for face in faces {
        if !once.iter().any(|(key, _)| *key == face.key) {
            once.push((&face.key, &face.name));
        }
    }
    once
}

/// photon's own albums: the ones a photo can be put into and taken out of. Picasa's are
/// mirrored from its INI, and the engine refuses every such write on them.
pub fn own_albums(albums: &[AlbumSummary]) -> Vec<&AlbumSummary> {
    albums.iter().filter(|album| !album.picasa).collect()
}

/// The Picasa albums a photo is in, in the list's order: the panel's rows that cannot
/// be ticked.
pub fn picasa_albums_of<'a>(
    albums: &'a [AlbumSummary],
    member_of: &[i64],
) -> Vec<&'a AlbumSummary> {
    (albums.iter())
        .filter(|album| album.picasa && member_of.contains(&album.id))
        .collect()
}

/// "Show 3 duplicates": the words the tile's menu has for the same thing.
pub fn show_copies_label(count: usize) -> String {
    if count == 1 {
        "Show 1 duplicate".to_owned()
    } else {
        format!("Show {} duplicates", grouped(count))
    }
}

/// The panel's line for the faces that have no name; none when there are none.
pub fn unnamed_faces_label(count: usize) -> Option<String> {
    match count {
        0 => None,
        1 => Some("1 face not named".to_owned()),
        count => Some(format!("{} faces not named", grouped(count))),
    }
}

/// The panel's line for a photo with no face at all, named or not. Which faces there
/// could have been depends on who looks: with photon's own detection on (`finds`), it
/// looked and found none - or has not got to this photo, while a pass is still running
/// (`looking`); with it off there are only the ones Picasa recorded.
pub fn no_faces_label(finds: bool, looking: bool) -> &'static str {
    match (finds, looking) {
        (false, _) => "No faces named in Picasa.",
        (true, true) => "No faces found yet.",
        (true, false) => "No faces found.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_make_is_not_repeated_when_the_model_names_it() {
        let name = |make, model| camera_name(make, model);
        assert_eq!(
            name(Some("Canon"), Some("Canon EOS 5D Mark IV")).as_deref(),
            Some("Canon EOS 5D Mark IV")
        );
        assert_eq!(
            name(Some("NIKON CORPORATION"), Some("NIKON D750")).as_deref(),
            Some("NIKON D750")
        );
        assert_eq!(
            name(Some("Apple"), Some("iPhone 15 Pro")).as_deref(),
            Some("Apple iPhone 15 Pro")
        );
        // Either half missing.
        assert_eq!(name(Some("Canon"), None).as_deref(), Some("Canon"));
        assert_eq!(name(None, Some("EOS 5D")).as_deref(), Some("EOS 5D"));
        assert_eq!(name(None, None), None);
        assert_eq!(name(Some("  "), Some("")), None);
    }

    #[test]
    fn focal_length_aperture_and_iso_are_spelled_as_a_photographer_reads_them() {
        assert_eq!(format_focal(50.0), "50 mm");
        assert_eq!(format_focal(18.5), "18.5 mm");
        assert_eq!(format_focal(4.25), "4.3 mm");
        assert_eq!(format_aperture(1.8), "f/1.8");
        assert_eq!(format_aperture(11.0), "f/11");
        assert_eq!(format_iso(400), "ISO 400");
    }

    #[test]
    fn a_shutter_speed_is_a_fraction_below_a_second_and_seconds_above() {
        assert_eq!(format_exposure(1.0 / 250.0), "1/250 s");
        assert_eq!(format_exposure(0.004), "1/250 s");
        // As a camera writes a sixtieth: to the whole number it is read as.
        assert_eq!(format_exposure(0.016_666_7), "1/60 s");
        assert_eq!(format_exposure(0.25), "1/4 s");
        assert_eq!(format_exposure(0.5), "0.5 s");
        assert_eq!(format_exposure(0.3), "0.3 s");
        assert_eq!(format_exposure(1.0), "1 s");
        assert_eq!(format_exposure(2.5), "2.5 s");
        assert_eq!(format_exposure(30.0), "30 s");
        assert_eq!(format_exposure(0.0), "");
    }

    #[test]
    fn the_cameras_rows_are_camera_lens_and_one_line_of_exposure() {
        let whole = Camera {
            make: Some("Canon"),
            model: Some("Canon EOS 5D"),
            lens: Some("EF50mm f/1.8 STM"),
            focal_mm: Some(50.0),
            aperture: Some(1.8),
            exposure_s: Some(0.004),
            iso: Some(400),
        };
        assert_eq!(
            camera_rows(whole),
            [
                row(
                    "Camera",
                    "Canon EOS 5D".into(),
                    Some("camera:\"Canon EOS 5D\"".into())
                ),
                row(
                    "Lens",
                    "EF50mm f/1.8 STM".into(),
                    Some("lens:\"EF50mm f/1.8 STM\"".into())
                ),
                row("Exposure", "50 mm · f/1.8 · 1/250 s · ISO 400".into(), None),
            ]
        );
        // None for a photo with no camera data, and what there is for one with some.
        assert_eq!(camera_rows(Camera::default()), []);
        let iso = Camera {
            iso: Some(100),
            ..Camera::default()
        };
        assert_eq!(camera_rows(iso), [row("Exposure", "ISO 100".into(), None)]);
        // A nought the camera wrote for "not known" is no value, and an empty lens no
        // lens.
        let noughts = Camera {
            lens: Some(""),
            focal_mm: Some(0.0),
            aperture: Some(0.0),
            exposure_s: Some(0.0),
            iso: Some(0),
            ..Camera::default()
        };
        assert_eq!(camera_rows(noughts), []);
    }

    #[test]
    fn a_field_query_quotes_its_value_and_drops_a_quote_it_could_not_escape() {
        assert_eq!(field_query("camera", "NIKON D750"), "camera:\"NIKON D750\"");
        assert_eq!(field_query("lens", " 7\" tele "), "lens:\"7  tele\"");
    }

    #[test]
    fn dimensions_are_spelled_with_the_multiplication_sign() {
        assert_eq!(format_dimensions(4000, 3000), "4000 × 3000");
    }

    fn copy(id: i64, kind: CopyKind) -> ItemCopy {
        ItemCopy {
            id,
            path: format!("/photos/{id}.jpg"),
            kind,
            width: 4000,
            height: 3000,
        }
    }

    #[test]
    fn copies_are_split_into_identical_and_look_alike_in_their_order() {
        let copies = [
            copy(2, CopyKind::Identical),
            copy(3, CopyKind::Similar),
            copy(4, CopyKind::Similar),
        ];
        assert_eq!(
            copy_groups(&copies),
            [
                CopyGroup {
                    kind: CopyKind::Identical,
                    label: "Identical",
                    copies: vec![&copies[0]],
                },
                CopyGroup {
                    kind: CopyKind::Similar,
                    label: "Looks the same",
                    copies: vec![&copies[1], &copies[2]],
                },
            ]
        );
        // A group with none is left out, and a photo with no copy has nothing to show.
        let kinds: Vec<CopyKind> = (copy_groups(&copies[1..]).iter())
            .map(|group| group.kind)
            .collect();
        assert_eq!(kinds, [CopyKind::Similar]);
        assert_eq!(copy_groups(&[]), []);
    }

    // 2024-06-15 12:30 as the camera's own clock, in seconds, and as an instant.
    const NOON: i64 = 1_718_454_645;

    fn dates() -> ItemDates {
        ItemDates {
            taken: None,
            digitized: None,
            edited: None,
            file_created_ms: None,
            file_modified_ms: NOON * 1000,
        }
    }

    fn rows(dates: &ItemDates, zone: &TimeZone) -> Vec<(String, String)> {
        (date_rows(dates, zone).into_iter())
            .map(|row| (row.label, row.value))
            .collect()
    }

    fn pairs(rows: &[(&str, &str)]) -> Vec<(String, String)> {
        (rows.iter())
            .map(|(label, value)| ((*label).to_owned(), (*value).to_owned()))
            .collect()
    }

    #[test]
    fn every_date_the_photo_has_is_listed_the_cameras_first() {
        let all = ItemDates {
            taken: Some(NOON),
            edited: Some(NOON + 86_400),
            file_created_ms: Some((NOON + 3_600) * 1000),
            ..dates()
        };
        assert_eq!(
            rows(&all, &TimeZone::UTC),
            pairs(&[
                ("Taken", "Jun 15, 2024, 12:30 PM"),
                ("Edited", "Jun 16, 2024, 12:30 PM"),
                ("File created", "Jun 15, 2024, 1:30 PM"),
                ("File modified", "Jun 15, 2024, 12:30 PM"),
            ])
        );
        assert_eq!(
            rows(&dates(), &TimeZone::UTC),
            pairs(&[("File modified", "Jun 15, 2024, 12:30 PM")])
        );
    }

    #[test]
    fn a_date_is_shown_once_under_every_tag_that_carries_it() {
        let alike = ItemDates {
            taken: Some(NOON),
            digitized: Some(NOON + 60),
            edited: Some(NOON),
            file_created_ms: Some(NOON * 1000),
            ..dates()
        };
        assert_eq!(
            rows(&alike, &TimeZone::UTC),
            pairs(&[
                ("Taken, edited", "Jun 15, 2024, 12:30 PM"),
                ("Digitized", "Jun 15, 2024, 12:31 PM"),
                ("File created, modified", "Jun 15, 2024, 12:30 PM"),
            ])
        );
    }

    // The camera's 12:30 is 12:30 wherever the photo is looked at; the file's instant is
    // 14:30 in Berlin in June. Equal as numbers, they are different times, and are never
    // one row.
    #[test]
    fn the_cameras_dates_are_its_own_clock_and_the_files_the_machines() {
        let berlin = TimeZone::get("Europe/Berlin").unwrap();
        let taken = ItemDates {
            taken: Some(NOON),
            ..dates()
        };
        assert_eq!(
            rows(&taken, &berlin),
            pairs(&[
                ("Taken", "Jun 15, 2024, 12:30 PM"),
                ("File modified", "Jun 15, 2024, 2:30 PM"),
            ])
        );
    }

    #[test]
    fn a_position_has_its_hemisphere_as_a_letter_not_a_sign() {
        assert_eq!(
            format_coordinates(48.1374, 11.5755),
            "48.13740° N, 11.57550° E"
        );
        assert_eq!(
            format_coordinates(-33.86882, -151.2093),
            "33.86882° S, 151.20930° W"
        );
        // A rounded nought is neither south nor west.
        assert_eq!(
            format_coordinates(-0.000001, -0.000001),
            "0.00000° N, 0.00000° E"
        );
        assert_eq!(format_coordinates(-0.00001, 0.0), "0.00001° S, 0.00000° E");
    }

    #[test]
    fn photos_nearby_is_a_term_the_search_reads_with_its_signs() {
        assert_eq!(near_query(48.13742, 11.57549), "near:48.1374,11.5755");
        assert_eq!(near_query(-33.86882, 151.2093), "near:-33.8688,151.2093");
    }

    #[test]
    fn a_person_named_twice_on_a_photo_is_listed_once() {
        let face = |key: &str, name: &str| ItemFace {
            key: key.to_owned(),
            name: name.to_owned(),
            left: 0.0,
            top: 0.0,
            right: 0.1,
            bottom: 0.1,
            face_id: None,
        };
        let faces = [face("p:1", "Anna"), face("c:9", "Ben"), face("p:1", "Anna")];
        assert_eq!(people(&faces), [("p:1", "Anna"), ("c:9", "Ben")]);
        // By who they are, not by what they are called: two people of one name are two.
        let namesakes = [face("p:1", "Anna"), face("p:2", "Anna")];
        assert_eq!(people(&namesakes), [("p:1", "Anna"), ("p:2", "Anna")]);
        assert!(people(&[]).is_empty());
    }

    #[test]
    fn photons_own_albums_are_ticked_and_picasas_only_listed_where_the_photo_is_in_them() {
        let album = |id: i64, picasa: bool| AlbumSummary {
            id,
            name: format!("Album {id}"),
            count: 1,
            picasa,
        };
        let albums = [
            album(1, false),
            album(2, true),
            album(3, false),
            album(4, true),
        ];
        let ids = |list: Vec<&AlbumSummary>| list.iter().map(|album| album.id).collect::<Vec<_>>();
        assert_eq!(ids(own_albums(&albums)), [1, 3]);
        // In 1, 2 and 4: Picasa's two, in the list's order; its own is not among them.
        assert_eq!(ids(picasa_albums_of(&albums, &[4, 1, 2])), [2, 4]);
        assert_eq!(ids(picasa_albums_of(&albums, &[1])), Vec::<i64>::new());
    }

    #[test]
    fn the_lines_about_faces_and_copies_count_in_words() {
        assert_eq!(unnamed_faces_label(0), None);
        assert_eq!(unnamed_faces_label(1).as_deref(), Some("1 face not named"));
        assert_eq!(
            unnamed_faces_label(1200).as_deref(),
            Some("1,200 faces not named")
        );
        assert_eq!(no_faces_label(false, false), "No faces named in Picasa.");
        assert_eq!(no_faces_label(false, true), "No faces named in Picasa.");
        assert_eq!(no_faces_label(true, true), "No faces found yet.");
        assert_eq!(no_faces_label(true, false), "No faces found.");
        assert_eq!(show_copies_label(1), "Show 1 duplicate");
        assert_eq!(show_copies_label(3), "Show 3 duplicates");
        assert_eq!(show_copies_label(1500), "Show 1,500 duplicates");
    }
}
