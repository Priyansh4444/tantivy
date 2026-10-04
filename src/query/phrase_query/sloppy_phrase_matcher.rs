use std::cmp::Reverse;
use std::collections::BinaryHeap;

/// A positional frontier following Lucene 10.4's SloppyPhraseMatcher traversal.
/// It emits minimal windows encountered by advancing the least clause, rather
/// than enumerating every positional assignment.
pub(super) struct SloppyPhraseMatcher {
    clauses: Vec<ClausePositions>,
    repeat_groups: Vec<Vec<usize>>,
    queue: BinaryHeap<Reverse<FrontierKey>>,
    end: i64,
    positioned: bool,
    slop: u32,
}

struct ClausePositions {
    query_offset: u32,
    positions: Vec<u32>,
    cursor: usize,
    repeat_group: Option<usize>,
    queued: bool,
}

/// The ordinal survives cost-based document intersection ordering.
#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
struct FrontierKey {
    position: i64,
    query_offset: u32,
    ordinal: usize,
}

impl SloppyPhraseMatcher {
    pub fn new(offsets: Vec<u32>, mut repeat_groups: Vec<Vec<usize>>, slop: u32) -> Self {
        let mut clauses: Vec<_> = offsets
            .into_iter()
            .map(|query_offset| ClausePositions {
                query_offset,
                positions: Vec::new(),
                cursor: 0,
                repeat_group: None,
                queued: false,
            })
            .collect();
        for (group_id, group) in repeat_groups.iter_mut().enumerate() {
            group.sort_by_key(|&ordinal| (clauses[ordinal].query_offset, ordinal));
            for &ordinal in group.iter() {
                clauses[ordinal].repeat_group = Some(group_id);
            }
        }
        Self {
            clauses,
            repeat_groups,
            queue: BinaryHeap::new(),
            end: i64::MIN,
            positioned: false,
            slop,
        }
    }

    pub fn positions_mut(&mut self, ordinal: usize) -> &mut Vec<u32> {
        &mut self.clauses[ordinal].positions
    }

    fn key(&self, ordinal: usize) -> FrontierKey {
        let clause = &self.clauses[ordinal];
        FrontierKey {
            position: i64::from(clause.positions[clause.cursor]) - i64::from(clause.query_offset),
            query_offset: clause.query_offset,
            ordinal,
        }
    }

    fn rebuild_queue(&mut self) {
        self.queue.clear();
        for ordinal in 0..self.clauses.len() {
            if self.clauses[ordinal].queued {
                self.queue.push(Reverse(self.key(ordinal)));
            }
        }
    }

    pub fn reset_document(&mut self) -> bool {
        self.positioned = false;
        self.queue.clear();
        self.end = i64::MIN;
        for clause in &mut self.clauses {
            clause.cursor = 0;
            clause.queued = false;
            if clause.positions.is_empty() {
                return false;
            }
        }
        // Each repeated query clause must start on a different occurrence.
        for group in &self.repeat_groups {
            for (cursor, &ordinal) in group.iter().enumerate() {
                if cursor >= self.clauses[ordinal].positions.len() {
                    return false;
                }
                self.clauses[ordinal].cursor = cursor;
            }
        }
        for ordinal in 0..self.clauses.len() {
            self.end = self.end.max(self.key(ordinal).position);
            self.clauses[ordinal].queued = true;
        }
        self.rebuild_queue();
        self.positioned = true;
        true
    }

    fn pop(&mut self) -> usize {
        let ordinal = self.queue.pop().unwrap().0.ordinal;
        self.clauses[ordinal].queued = false;
        ordinal
    }

    fn push(&mut self, ordinal: usize) {
        self.clauses[ordinal].queued = true;
        self.queue.push(Reverse(self.key(ordinal)));
    }

    fn advance(&mut self, ordinal: usize) -> bool {
        let clause = &mut self.clauses[ordinal];
        if clause.cursor + 1 >= clause.positions.len() {
            return false;
        }
        clause.cursor += 1;
        self.end = self.end.max(self.key(ordinal).position);
        true
    }

    fn collision(&self, ordinal: usize, group: usize) -> Option<usize> {
        let clause = &self.clauses[ordinal];
        let position = clause.positions[clause.cursor];
        self.repeat_groups[group].iter().copied().find(|&other| {
            other != ordinal
                && self.clauses[other].positions[self.clauses[other].cursor] == position
        })
    }

    fn resolve_repeats(&mut self, mut ordinal: usize) -> bool {
        let Some(group) = self.clauses[ordinal].repeat_group else {
            return true;
        };
        let mut changed_queue = false;
        while let Some(other) = self.collision(ordinal, group) {
            // Lucene's collision comparison intentionally does not use ordinal.
            let left = self.key(ordinal);
            let right = self.key(other);
            if (left.position, left.query_offset) >= (right.position, right.query_offset) {
                ordinal = other;
            }
            changed_queue |= self.clauses[ordinal].queued;
            if !self.advance(ordinal) {
                return false;
            }
        }
        if changed_queue {
            self.rebuild_queue();
        }
        true
    }

    pub fn next_match_distance(&mut self) -> Option<u32> {
        if !self.positioned {
            return None;
        }
        let mut ordinal = self.pop();
        let mut distance = self.end - self.key(ordinal).position;
        let mut next = self.queue.peek().unwrap().0.position;
        while self.advance(ordinal) {
            if !self.resolve_repeats(ordinal) {
                break;
            }
            if self.key(ordinal).position > next {
                self.push(ordinal);
                if distance <= i64::from(self.slop) {
                    return Some(distance as u32);
                }
                ordinal = self.pop();
                next = self.queue.peek().unwrap().0.position;
                distance = self.end - self.key(ordinal).position;
            } else {
                distance = distance.min(self.end - self.key(ordinal).position);
            }
        }
        self.positioned = false;
        (distance <= i64::from(self.slop)).then_some(distance as u32)
    }

    pub fn frequency(&mut self) -> (u32, f32) {
        let mut count = 0;
        let mut frequency = 0.0;
        while let Some(distance) = self.next_match_distance() {
            count += 1;
            frequency += 1.0 / (1.0 + distance as f32);
        }
        (count, frequency)
    }
}

#[cfg(test)]
mod tests {
    use super::SloppyPhraseMatcher;

    fn assignment_exists(
        positions: &[Vec<u32>],
        offsets: &[u32],
        repeats: bool,
        slop: u32,
    ) -> bool {
        // Independent bounded Cartesian matching reference, not a frequency oracle.
        for &left in &positions[0] {
            for &right in &positions[1] {
                if repeats && left == right {
                    continue;
                }
                let left = i64::from(left) - i64::from(offsets[0]);
                let right = i64::from(right) - i64::from(offsets[1]);
                if left.abs_diff(right) <= u64::from(slop) {
                    return true;
                }
            }
        }
        false
    }

    #[test]
    fn test_frontier_exists_matches_bounded_assignment_reference() {
        let positions = |mask: u32| {
            (0..5)
                .filter(|position| mask & (1 << position) != 0)
                .collect::<Vec<u32>>()
        };
        for left in 0..32 {
            for right in 0..32 {
                for slop in [1, 2, 4, 256, u32::MAX] {
                    for repeats in [false, true] {
                        if repeats && left != right {
                            continue;
                        }
                        let arrays = vec![positions(left), positions(right)];
                        let mut matcher = SloppyPhraseMatcher::new(
                            vec![0, 1],
                            if repeats { vec![vec![0, 1]] } else { vec![] },
                            slop,
                        );
                        for (ordinal, array) in arrays.iter().enumerate() {
                            *matcher.positions_mut(ordinal) = array.clone();
                        }
                        let observed =
                            matcher.reset_document() && matcher.next_match_distance().is_some();
                        assert_eq!(
                            observed,
                            assignment_exists(&arrays, &[0, 1], repeats, slop),
                            "{arrays:?}, repeats={repeats}, slop={slop}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn test_frontier_weighted_windows_and_repeat_collisions() {
        for (positions, groups, slop, distances) in [
            (vec![vec![0, 2], vec![1]], vec![], 2, vec![0, 2]),
            (vec![vec![1], vec![0]], vec![], 2, vec![2]),
            (
                vec![vec![0, 1, 2], vec![0, 1, 2]],
                vec![vec![0, 1]],
                1,
                vec![0, 0],
            ),
        ] {
            let mut matcher = SloppyPhraseMatcher::new(vec![0, 1], groups, slop);
            for (ordinal, array) in positions.into_iter().enumerate() {
                *matcher.positions_mut(ordinal) = array;
            }
            assert!(matcher.reset_document());
            let mut observed = Vec::new();
            while let Some(distance) = matcher.next_match_distance() {
                observed.push(distance);
            }
            assert_eq!(observed, distances);
        }
    }

    #[test]
    fn test_frozen_lucene_high_repeat_frontier_frequency() {
        // Lucene 10.4 explanations for 97 consecutive alpha tokens and
        // eight alpha-beta pairs report phraseFreq 95 and 9 respectively.
        for (positions, groups, offsets, slop, frequency) in [
            (
                vec![(0..97).collect(); 3],
                vec![vec![0, 1, 2]],
                vec![0, 1, 2],
                1,
                95.0,
            ),
            (
                vec![
                    (0..16).step_by(2).collect(),
                    (1..16).step_by(2).collect(),
                    (0..16).step_by(2).collect(),
                    (1..16).step_by(2).collect(),
                ],
                vec![vec![0, 2], vec![1, 3]],
                vec![0, 1, 2, 3],
                4,
                9.0,
            ),
        ] {
            let mut matcher = SloppyPhraseMatcher::new(offsets, groups, slop);
            for (ordinal, positions) in positions.into_iter().enumerate() {
                *matcher.positions_mut(ordinal) = positions;
            }
            assert!(matcher.reset_document());
            assert_eq!(matcher.frequency().1, frequency);
        }
    }

    #[test]
    fn test_three_term_frontier_exists_matches_assignment_reference() {
        let positions = |mask: u32| {
            (0..4)
                .filter(|p| mask & (1 << p) != 0)
                .collect::<Vec<u32>>()
        };
        for first in 0..16 {
            for second in 0..16 {
                for third in 0..16 {
                    for repeated in [false, true] {
                        if repeated && first != third {
                            continue;
                        }
                        let arrays = [positions(first), positions(second), positions(third)];
                        for slop in [1, 2, 4, 256, u32::MAX] {
                            let expected = arrays[0].iter().any(|&a| {
                                arrays[1].iter().any(|&b| {
                                    arrays[2].iter().any(|&c| {
                                        if repeated && a == c {
                                            return false;
                                        }
                                        let normalized =
                                            [i64::from(a), i64::from(b) - 1, i64::from(c) - 2];
                                        normalized.iter().max().unwrap()
                                            - normalized.iter().min().unwrap()
                                            <= i64::from(slop)
                                    })
                                })
                            });
                            let mut matcher = SloppyPhraseMatcher::new(
                                vec![0, 1, 2],
                                if repeated { vec![vec![0, 2]] } else { vec![] },
                                slop,
                            );
                            for (ordinal, array) in arrays.iter().enumerate() {
                                *matcher.positions_mut(ordinal) = array.clone();
                            }
                            let observed =
                                matcher.reset_document() && matcher.next_match_distance().is_some();
                            assert_eq!(
                                observed, expected,
                                "{arrays:?} repeated={repeated} slop={slop}"
                            );
                        }
                    }
                }
            }
        }
    }
}
