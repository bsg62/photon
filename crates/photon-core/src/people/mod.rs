//! Which group a face belongs in. Pure: vectors and group sums in, a choice out. The
//! library applies it (`library/people.rs`), one face at a time in id order.

use crate::face_embed::similarity;
use std::collections::HashSet;

/// How alike a face and a group's average must be for the face to join it. Measured
/// 2026-10-03 on 2,674 LFW faces (500 people with several photos, 700 with one): at 0.50
/// this rule misplaced 6-11 faces and kept 446-448 of the 500 people in one group; at
/// 0.45, 27-40 and 468-469; at 0.55, 2 and 411-414. Below 0.50 mixed groups rise quickly,
/// above it people split for little gain - and a split is one merge to fix, where a
/// mixed group is faces removed one at a time.
pub const GROUP_SIMILARITY: f32 = 0.50;

/// A group as the rule sees it: the sum of the vectors that count towards it.
#[derive(Clone, Debug)]
pub struct Group {
    pub id: i64,
    pub sum: Vec<f32>,
    pub count: usize,
}

impl Group {
    pub fn add(&mut self, face: &[f32]) {
        for (s, x) in self.sum.iter_mut().zip(face) {
            *s += x;
        }
        self.count += 1;
    }
}

#[derive(Debug, PartialEq)]
pub enum Choice {
    Join(i64),
    New,
}

/// The group `face` joins: the most alike, if at least [`GROUP_SIMILARITY`], passing over
/// groups the face was rejected from and groups nothing counts towards.
pub fn choose(face: &[f32], groups: &[Group], rejected: &HashSet<i64>) -> Choice {
    groups
        .iter()
        // `similarity` already answers 0 for an all-zero sum, so for every group the library
        // builds today this check changes nothing. It is kept for a group with a nonzero
        // sum and nothing counted towards it, which only a future caller that seeds a sum
        // before counting could produce.
        .filter(|g| g.count > 0 && !rejected.contains(&g.id))
        .map(|g| (g.id, similarity(&g.sum, face)))
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
        Group {
            id,
            sum: at(deg),
            count: 1,
        }
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
        let empty = Group {
            id: 1,
            sum: vec![0f32; 128],
            count: 0,
        };
        assert_eq!(choose(&at(0.0), &[empty], &rejected(NONE)), Choice::New);
    }

    /// A group with a nonzero sum and nothing counted towards it. The library does not
    /// produce one today (a named person with no confirmed face has a zero sum); this pins
    /// the `count` check for a future caller that seeds a sum before counting.
    #[test]
    fn a_group_that_counts_nothing_is_passed_over_whatever_its_sum() {
        let seeded = Group {
            id: 1,
            sum: at(0.0),
            count: 0,
        };
        assert_eq!(choose(&at(0.0), &[seeded], &rejected(NONE)), Choice::New);
    }

    /// The comparison is with the group's average direction, not its first face: a group
    /// built from faces at 0° and 40° sits at 20°.
    #[test]
    fn a_group_is_compared_by_its_average() {
        let mut g = group(1, 0.0);
        g.add(&at(40.0));
        assert_eq!(g.count, 2);
        // 75° is 0.26 from the first face alone and 0.57 from the average at 20°.
        assert_eq!(choose(&at(75.0), &[g], &rejected(NONE)), Choice::Join(1));
        assert_eq!(
            choose(&at(75.0), &[group(1, 0.0)], &rejected(NONE)),
            Choice::New
        );
    }

    #[test]
    fn only_confirmed_faces_move_a_named_person() {
        assert!(counts_toward_centroid(false, false));
        assert!(counts_toward_centroid(false, true));
        assert!(counts_toward_centroid(true, true));
        assert!(!counts_toward_centroid(true, false));
    }
}
