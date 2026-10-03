//! Which group a face belongs in. Pure: vectors and group sums in, a choice out. The
//! library applies it (`library/people.rs`), one face at a time in id order.

use crate::face_embed::{DIM, dot, norm};
use std::collections::HashSet;

/// How alike a face and a group's average must be for the face to join it. Measured
/// 2026-10-03 on 2,674 LFW faces (500 people with several photos, 700 with one): at 0.50
/// this rule misplaced 6-11 faces and kept 446-448 of the 500 people in one group; at
/// 0.45, 27-40 and 468-469; at 0.55, 2 and 411-414. Below 0.50 mixed groups rise quickly,
/// above it people split for little gain - and a split is one merge to fix, where a
/// mixed group is faces removed one at a time.
pub const GROUP_SIMILARITY: f32 = 0.50;

/// A group as the rule sees it: the sum of the vectors that count towards it, and that
/// sum's length, kept beside it. The rule compares a face with every group, inside the
/// library's write; the length changes only when a face is added, so it is worked out then
/// rather than in each comparison. The fields are private so nothing can change the sum
/// without the length.
#[derive(Clone, Debug)]
pub struct Group {
    pub id: i64,
    sum: Vec<f32>,
    norm: f32,
}

impl Group {
    /// A group nothing counts towards yet.
    pub fn new(id: i64) -> Self {
        Self {
            id,
            sum: vec![0f32; DIM],
            norm: 0.0,
        }
    }

    pub fn add(&mut self, face: &[f32]) {
        for (s, x) in self.sum.iter_mut().zip(face) {
            *s += x;
        }
        self.norm = norm(&self.sum);
    }
}

#[derive(Debug, PartialEq)]
pub enum Choice {
    Join(i64),
    New,
}

/// The group `face` joins: the most alike, if at least [`GROUP_SIMILARITY`], passing over
/// groups the face was rejected from and groups nothing counts towards.
///
/// The cosine with each group is its dot product over the two lengths, as
/// [`similarity`](crate::face_embed::similarity) has it, with the face's length taken once
/// and the group's kept in the group. A group whose sum has no length - nothing counted
/// towards it - is passed over, where `similarity` would have answered 0, below any
/// threshold: the same choice.
pub fn choose(face: &[f32], groups: &[Group], rejected: &HashSet<i64>) -> Choice {
    let face_norm = norm(face);
    if face_norm == 0.0 {
        return Choice::New;
    }
    groups
        .iter()
        .filter(|g| g.norm > 0.0 && !rejected.contains(&g.id))
        .map(|g| (g.id, dot(&g.sum, face) / (g.norm * face_norm)))
        .filter(|(_, s)| *s >= GROUP_SIMILARITY)
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map_or(Choice::New, |(id, _)| Choice::Join(id))
}

/// Whether a face counts towards its group's average: every face of an unnamed or ignored
/// group, and only the confirmed faces of a named person, so that suggestions cannot pull
/// a person towards themselves.
pub fn counts_toward_centroid(named: bool, confirmed: bool) -> bool {
    !named || confirmed
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unit vector at `deg` degrees in the first two dimensions: two of them have a
    /// similarity of cos(difference).
    fn at(deg: f32) -> Vec<f32> {
        let mut v = vec![0f32; 128];
        v[0] = deg.to_radians().cos();
        v[1] = deg.to_radians().sin();
        v
    }

    fn group(id: i64, deg: f32) -> Group {
        let mut g = Group::new(id);
        g.add(&at(deg));
        g
    }

    const NONE: &[i64] = &[];

    fn rejected(ids: &[i64]) -> HashSet<i64> {
        ids.iter().copied().collect()
    }

    /// cos(59°) = 0.515 joins; cos(61°) = 0.485 does not.
    #[test]
    fn a_face_joins_at_the_threshold_and_not_below() {
        assert_eq!(
            choose(&at(59.0), &[group(1, 0.0)], &rejected(NONE)),
            Choice::Join(1)
        );
        assert_eq!(
            choose(&at(61.0), &[group(1, 0.0)], &rejected(NONE)),
            Choice::New
        );
    }

    #[test]
    fn the_nearest_group_wins() {
        let groups = [group(1, 0.0), group(2, 30.0)];
        assert_eq!(choose(&at(20.0), &groups, &rejected(NONE)), Choice::Join(2));
    }

    #[test]
    fn a_rejected_group_is_passed_over() {
        let groups = [group(1, 0.0), group(2, 40.0)];
        assert_eq!(choose(&at(5.0), &groups, &rejected(&[1])), Choice::Join(2));
        assert_eq!(
            choose(&at(5.0), &[group(1, 0.0)], &rejected(&[1])),
            Choice::New
        );
    }

    #[test]
    fn a_group_with_no_centroid_is_passed_over() {
        assert_eq!(
            choose(&at(0.0), &[Group::new(1)], &rejected(NONE)),
            Choice::New
        );
    }

    /// The comparison is with the group's average direction, not its first face: a group
    /// built from faces at 0° and 40° sits at 20°.
    #[test]
    fn a_group_is_compared_by_its_average() {
        let mut g = group(1, 0.0);
        g.add(&at(40.0));
        // 75° is 0.26 from the first face alone and 0.57 from the average at 20°.
        assert_eq!(choose(&at(75.0), &[g], &rejected(NONE)), Choice::Join(1));
        assert_eq!(
            choose(&at(75.0), &[group(1, 0.0)], &rejected(NONE)),
            Choice::New
        );
    }

    /// The choice made with each group's kept length is the one the plain cosine makes,
    /// for groups that grew face by face. A length not brought up to date by `add` stays
    /// that of the first face: a group of three faces at 0°, 10° and 20° has a sum about
    /// three long, and divided by one instead the face at 80° - 0.34 from the average at
    /// 10° - would score 0.99 and join it.
    #[test]
    fn the_kept_length_chooses_as_the_cosine_does() {
        use crate::face_embed::similarity;
        let mut a = group(1, 0.0);
        a.add(&at(10.0));
        a.add(&at(20.0));
        let mut b = group(2, 150.0);
        for deg in [160.0, 170.0, 180.0, 190.0] {
            b.add(&at(deg));
        }
        let groups = [a, b];
        let plain = |face: &[f32]| {
            groups
                .iter()
                .map(|g| (g.id, similarity(&g.sum, face)))
                .filter(|(_, s)| *s >= GROUP_SIMILARITY)
                .max_by(|x, y| x.1.total_cmp(&y.1))
                .map_or(Choice::New, |(id, _)| Choice::Join(id))
        };
        assert_eq!(choose(&at(80.0), &groups, &rejected(NONE)), Choice::New);
        // Never exactly 60° from either average (10° and 170°), where the cosine is the
        // threshold itself and the two sums may round to either side of it.
        for deg in (-178..180).step_by(5) {
            let face = at(deg as f32);
            assert_eq!(
                choose(&face, &groups, &rejected(NONE)),
                plain(&face),
                "a face at {deg}°"
            );
        }
    }

    #[test]
    fn only_confirmed_faces_move_a_named_person() {
        assert!(counts_toward_centroid(false, false));
        assert!(counts_toward_centroid(false, true));
        assert!(counts_toward_centroid(true, true));
        assert!(!counts_toward_centroid(true, false));
    }
}
