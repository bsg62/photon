//! The one rule for "the faces on this photo": Picasa's, plus every detection that is not
//! one of Picasa's. The viewer and search both go through it, so they cannot disagree.
//!
//! It is also the one place the two frames meet. A `faces` row is fractions of the
//! unedited picture; a `detected_faces` row is fractions of the picture as shown.

use super::Rect;
use crate::edit::Edit;

/// A Picasa rectangle in the picture as shown, or `None` when the edit crops its centre
/// away - `viewer_item`'s rule, which this now holds for it.
pub fn shown(edit: Edit, picasa: Rect) -> Option<Rect> {
    let (left, top, right, bottom) =
        edit.map_rect((picasa.left, picasa.top, picasa.right, picasa.bottom))?;
    Some(Rect {
        left,
        top,
        right,
        bottom,
    })
}

fn centre(r: &Rect) -> (f64, f64) {
    ((r.left + r.right) / 2.0, (r.top + r.bottom) / 2.0)
}

fn holds(r: &Rect, (x, y): (f64, f64)) -> bool {
    (r.left..=r.right).contains(&x) && (r.top..=r.bottom).contains(&y)
}

/// Whether two rectangles are the same face: either one's centre lies inside the other.
///
/// Not an overlap ratio. Picasa draws a face with its hair and chin and the detector draws
/// it tight, so one face's two boxes can share a fifth of their union - less than two
/// neighbours' boxes do. A centre is where the face is, whatever the box's generosity.
pub fn same_face(a: &Rect, b: &Rect) -> bool {
    holds(a, centre(b)) || holds(b, centre(a))
}

/// The detections that are none of Picasa's faces. Both in the picture as shown.
pub fn unmatched(picasa_shown: &[Rect], detected: &[Rect]) -> Vec<Rect> {
    let tagged: Vec<((), Rect)> = detected.iter().map(|d| ((), *d)).collect();
    unmatched_by(picasa_shown, &tagged)
        .into_iter()
        .map(|(_, d)| d)
        .collect()
}

/// `unmatched` for detections that carry something to keep - the viewer's ids, which say
/// which face an outline is - so the rule has this one body however it is asked.
pub fn unmatched_by<T: Copy>(picasa_shown: &[Rect], detected: &[(T, Rect)]) -> Vec<(T, Rect)> {
    detected
        .iter()
        .filter(|(_, d)| !picasa_shown.iter().any(|p| same_face(p, d)))
        .copied()
        .collect()
}

/// How many faces a photo has: Picasa's that the edit still shows, and the detections
/// that are none of them. `picasa` is in the unedited picture, as `faces` stores it.
pub fn count(edit: Edit, picasa: &[Rect], detected: &[Rect]) -> u32 {
    let picasa: Vec<Rect> = picasa.iter().filter_map(|p| shown(edit, *p)).collect();
    (picasa.len() + unmatched(&picasa, detected).len()) as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::{CROP_UNIT, Crop, Edit};

    fn r(left: f64, top: f64, right: f64, bottom: f64) -> Rect {
        Rect {
            left,
            top,
            right,
            bottom,
        }
    }

    #[test]
    fn a_detection_over_a_picasa_face_is_the_same_face() {
        let picasa = [r(0.30, 0.20, 0.50, 0.50)];
        let detected = [r(0.32, 0.22, 0.48, 0.47)];
        assert!(unmatched(&picasa, &detected).is_empty());
    }

    #[test]
    fn the_unmatched_detections_keep_what_they_carry() {
        let picasa = [r(0.30, 0.20, 0.50, 0.50)];
        let under = r(0.32, 0.22, 0.48, 0.47);
        let left = r(0.05, 0.20, 0.20, 0.50);
        let right = r(0.60, 0.20, 0.80, 0.50);
        assert_eq!(
            unmatched_by(&picasa, &[(7, left), (8, under), (9, right)]),
            vec![(7, left), (9, right)]
        );
    }

    #[test]
    fn a_detection_beside_a_picasa_face_is_another_face() {
        let picasa = [r(0.10, 0.20, 0.30, 0.50)];
        let detected = [r(0.60, 0.20, 0.80, 0.50)];
        assert_eq!(unmatched(&picasa, &detected), detected);
    }

    /// Picasa draws a face generously, hair and chin; the detector draws it tight. The
    /// tight box is a fifth of the loose one's area, which an overlap ratio of 0.3 would
    /// call two faces. Either centre inside the other box calls it one.
    #[test]
    fn boxes_of_different_tightness_around_one_face_match() {
        let loose = r(0.20, 0.10, 0.60, 0.70);
        let tight = r(0.34, 0.30, 0.50, 0.56);
        assert!(same_face(&loose, &tight));
        assert!(same_face(&tight, &loose));
    }

    /// A face off-centre in Picasa's generous box: the tight box's centre is inside the
    /// loose one, but the loose one's centre is not inside the tight one. The match must
    /// hold whichever way round the two are asked.
    #[test]
    fn a_face_off_centre_in_a_loose_box_matches_both_ways() {
        let loose = r(0.20, 0.10, 0.60, 0.70);
        let tight = r(0.22, 0.12, 0.34, 0.30);
        assert!(same_face(&loose, &tight));
        assert!(same_face(&tight, &loose));
    }

    /// Two people cheek to cheek: the boxes overlap, but neither centre is in the other.
    #[test]
    fn overlapping_boxes_of_two_people_do_not_match() {
        let a = r(0.20, 0.20, 0.50, 0.60);
        let b = r(0.45, 0.20, 0.75, 0.60);
        assert!(!same_face(&a, &b));
    }

    /// A face Picasa recorded in the half that a crop removed is not in the picture as
    /// shown: it is not counted, and it must not swallow a detection either.
    #[test]
    fn a_picasa_face_cropped_away_is_gone() {
        // Keep the left half.
        let edit = Edit {
            turns: 0,
            crop: Some(Crop {
                left: 0,
                top: 0,
                right: (CROP_UNIT / 2) as u16,
                bottom: CROP_UNIT as u16,
            }),
        };
        let cropped_away = r(0.70, 0.20, 0.90, 0.50);
        assert_eq!(shown(edit, cropped_away), None);
        // Something detected elsewhere, in the picture as shown.
        let detected = [r(0.10, 0.20, 0.30, 0.50)];
        assert_eq!(count(edit, &[cropped_away], &detected), 1);
    }

    #[test]
    fn a_picasa_face_is_mapped_through_a_turn() {
        let edit = Edit {
            turns: 1,
            crop: None,
        };
        // A quarter turn clockwise sends (l, t, r, b) to (1 - b, l, 1 - t, r).
        assert_eq!(
            shown(edit, r(0.1, 0.2, 0.3, 0.6)),
            Some(r(0.4, 0.1, 0.8, 0.3))
        );
    }

    #[test]
    fn the_count_is_picasas_faces_and_the_detections_that_are_not_theirs() {
        let edit = Edit::default();
        let picasa = [r(0.10, 0.20, 0.30, 0.50)];
        let detected = [r(0.12, 0.22, 0.28, 0.48), r(0.60, 0.20, 0.80, 0.50)];
        assert_eq!(count(edit, &picasa, &detected), 2);
        assert_eq!(count(edit, &picasa, &[]), 1);
        assert_eq!(count(edit, &[], &detected), 2);
        assert_eq!(count(edit, &[], &[]), 0);
    }
}
