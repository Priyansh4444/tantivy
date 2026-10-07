use std::ops::{Deref, DerefMut};

use super::block_wand_intersection;
use crate::query::score_combiner::{ScoreSumUpperBound, SumCombiner};
use crate::query::term_query::TermScorer;
use crate::query::weight::for_each_pruning_scorer;
use crate::query::{BufferedUnionScorer, Scorer};
use crate::{DocId, DocSet, Score, TERMINATED};

/// Score the first candidates without WAND bookkeeping. Once the threshold
/// rises, visit only documents containing the term that can still win alone,
/// or intersect when neither term can win alone.
pub(crate) fn two_term_or_maxscore(
    mut scorers: Vec<TermScorer>,
    max_doc: DocId,
    mut threshold: Score,
    callback: &mut dyn FnMut(DocId, Score) -> Score,
) {
    debug_assert_eq!(scorers.len(), 2);
    let max_scores = [scorers[0].max_score(), scorers[1].max_score()];
    // Establish the region kernel's numeric domain before any cursor moves.
    // Signed and exceptional leaves retain canonical SumCombiner arithmetic.
    if threshold.is_nan()
        || scorers.iter().zip(max_scores).any(|(scorer, maximum)| {
            !scorer.bm25_weight().has_safe_score_bounds() || !maximum.is_finite() || maximum <= 0.0
        })
    {
        let mut union = BufferedUnionScorer::build(scorers, SumCombiner::default, max_doc);
        for_each_pruning_scorer(&mut union, threshold, callback);
        return;
    }

    for _ in 0..128 {
        let doc = scorers[0].doc().min(scorers[1].doc());
        if doc == TERMINATED {
            return;
        }
        let mut score = 0.0;
        for scorer in &mut scorers {
            if scorer.doc() == doc {
                score += scorer.score();
                scorer.advance();
            }
        }
        if score > threshold {
            threshold = callback(doc, score);
        }
        if threshold >= max_scores[0] && threshold >= max_scores[1] {
            block_wand_intersection(scorers, threshold, callback);
            return;
        }
    }

    if threshold >= max_scores[0] || threshold >= max_scores[1] {
        // A document without the essential term cannot exceed the current
        // threshold. Drive its postings and only look up the optional term
        // for these candidates.
        let essential_idx = usize::from(threshold >= max_scores[0]);
        let (essential, optional) = if essential_idx == 0 {
            let (essential, optional) = scorers.split_at_mut(1);
            (&mut essential[0], &mut optional[0])
        } else {
            let (optional, essential) = scorers.split_at_mut(1);
            (&mut essential[0], &mut optional[0])
        };
        while essential.doc() < TERMINATED {
            let doc = essential.doc();
            let mut score = essential.score();
            if score + optional.max_score() > threshold && optional.doc() <= doc {
                if optional.seek(doc) == doc {
                    score += optional.score();
                }
            }
            if score > threshold {
                threshold = callback(doc, score);
            }
            essential.advance();
            if threshold >= max_scores[essential_idx] {
                block_wand_intersection(scorers, threshold, callback);
                return;
            }
        }
        return;
    }

    two_term_regions(scorers, max_doc, threshold, callback);
}

// Keep hot metadata separate from the large incoming-order TermScorers. Actual
// cursor mutations own the decoded-document mirror; shallow selection does not.
struct TwoTermState {
    doc: DocId,
    global_max: Score,
    cost: u32,
}

impl TwoTermState {
    #[inline]
    fn assert_coherent(&self, scorer: &TermScorer) {
        debug_assert_eq!(self.doc, scorer.doc());
    }

    #[inline]
    fn seek(&mut self, scorer: &mut TermScorer, target: DocId) {
        self.assert_coherent(scorer);
        self.doc = scorer.seek(target);
        self.assert_coherent(scorer);
    }

    #[inline]
    fn advance(&mut self, scorer: &mut TermScorer) {
        self.assert_coherent(scorer);
        self.doc = scorer.advance();
        self.assert_coherent(scorer);
    }
}

enum TwoTermMode {
    Union,
    Essential { ordinal: usize, required: bool },
}

#[cfg(test)]
type TwoTermRegionChoice = (DocId, DocId, u8);

#[cfg(test)]
thread_local! {
    // Opt-in evidence for mode-transition fixtures; absent from release builds.
    static TWO_TERM_REGIONS: std::cell::RefCell<Option<Vec<TwoTermRegionChoice>>> =
        const { std::cell::RefCell::new(None) };
}

/// The post-warm two-term fallback: establish one local certificate, then use
/// a fixed merge or one essential stream until its physical half-open end.
/// Local required/optional roles expire at that end; this remains an OR query.
fn two_term_regions(
    mut scorers: Vec<TermScorer>,
    max_doc: DocId,
    mut threshold: Score,
    callback: &mut dyn FnMut(DocId, Score) -> Score,
) {
    let mut states: [TwoTermState; 2] = std::array::from_fn(|ordinal| TwoTermState {
        doc: scorers[ordinal].doc(),
        global_max: scorers[ordinal].max_score(),
        cost: scorers[ordinal].size_hint(),
    });
    let upper_bound = ScoreSumUpperBound::new(2);
    let mut lo = states[0].doc.min(states[1].doc);
    while lo < max_doc {
        for ordinal in 0..2 {
            states[ordinal].assert_coherent(&scorers[ordinal]);
        }
        let globally_optional = [
            states[0].global_max <= threshold,
            states[1].global_max <= threshold,
        ];
        // Preserve the established globally single-essential policy: an
        // optional global certificate need not cap its sparse driver's block.
        // When both terms need each other, use both local block certificates.
        let ignore_optional_cap = globally_optional[0] != globally_optional[1];
        let mut maxima = [0.0; 2];
        let mut hi = max_doc;
        for ordinal in 0..2 {
            let state = &mut states[ordinal];
            if state.doc == TERMINATED {
                continue;
            }
            if ignore_optional_cap && globally_optional[ordinal] {
                maxima[ordinal] = state.global_max;
                continue;
            }
            state.assert_coherent(&scorers[ordinal]);
            scorers[ordinal].seek_block(lo.max(state.doc));
            state.assert_coherent(&scorers[ordinal]);
            let last_doc = scorers[ordinal].last_doc_in_block();
            // A selected tail can leave an old decoded document below lo.
            // Only an actual seek proves whether there is a remaining tail doc.
            if last_doc == TERMINATED && state.doc < lo {
                state.seek(&mut scorers[ordinal], lo);
            }
            if state.doc == TERMINATED {
                continue;
            }
            // Keep the originally selected cap across tail reconciliation.
            hi = hi.min(last_doc.min(max_doc - 1) + 1);
            let bound = scorers[ordinal].block_max_score();
            state.assert_coherent(&scorers[ordinal]);
            maxima[ordinal] = if bound.is_finite() && bound >= 0.0 {
                bound
            } else {
                state.global_max
            };
        }
        debug_assert!(hi > lo);
        for ordinal in 0..2 {
            if states[ordinal].doc >= hi {
                maxima[ordinal] = 0.0;
            }
        }
        if upper_bound.score(f64::from(maxima[0]) + f64::from(maxima[1])) <= threshold {
            lo = hi;
            continue;
        }
        let mode = match (maxima[0] > threshold, maxima[1] > threshold) {
            (true, true) => TwoTermMode::Union,
            (true, false) => TwoTermMode::Essential {
                ordinal: 0,
                required: false,
            },
            (false, true) => TwoTermMode::Essential {
                ordinal: 1,
                required: false,
            },
            (false, false) => TwoTermMode::Essential {
                ordinal: usize::from(states[1].cost < states[0].cost),
                required: true,
            },
        };
        #[cfg(test)]
        TWO_TERM_REGIONS.with(|regions| {
            if let Some(regions) = regions.borrow_mut().as_mut() {
                let kind = match &mode {
                    TwoTermMode::Union => 0,
                    TwoTermMode::Essential { ordinal, required } => {
                        1 + *ordinal as u8 + 2 * u8::from(*required)
                    }
                };
                regions.push((lo, hi, kind));
            }
        });
        match mode {
            TwoTermMode::Union => {
                for ordinal in 0..2 {
                    if states[ordinal].doc < lo {
                        states[ordinal].seek(&mut scorers[ordinal], lo);
                    }
                }
                loop {
                    let doc = states[0].doc.min(states[1].doc);
                    if doc >= hi {
                        break;
                    }
                    let mut leaves = [0.0; 2];
                    for ordinal in 0..2 {
                        if states[ordinal].doc == doc {
                            states[ordinal].assert_coherent(&scorers[ordinal]);
                            leaves[ordinal] = scorers[ordinal].score();
                            states[ordinal].assert_coherent(&scorers[ordinal]);
                        }
                    }
                    let score = (f64::from(leaves[0]) + f64::from(leaves[1])) as Score;
                    if score > threshold {
                        let next_threshold = callback(doc, score);
                        debug_assert!(next_threshold >= threshold);
                        threshold = next_threshold;
                    }
                    for ordinal in 0..2 {
                        if states[ordinal].doc == doc {
                            states[ordinal].advance(&mut scorers[ordinal]);
                        }
                    }
                }
            }
            TwoTermMode::Essential { ordinal, required } => {
                let optional = 1 - ordinal;
                if states[ordinal].doc < lo {
                    states[ordinal].seek(&mut scorers[ordinal], lo);
                }
                while states[ordinal].doc < hi {
                    let doc = states[ordinal].doc;
                    states[ordinal].assert_coherent(&scorers[ordinal]);
                    let leaf = scorers[ordinal].score();
                    states[ordinal].assert_coherent(&scorers[ordinal]);
                    // Forward rounded bounds preserve winners at float midpoints.
                    if upper_bound.score(f64::from(leaf) + f64::from(maxima[optional])) > threshold
                    {
                        if states[optional].doc < doc {
                            states[optional].seek(&mut scorers[optional], doc);
                        }
                        let matched = states[optional].doc == doc;
                        if matched || !required {
                            let mut leaves = [0.0; 2];
                            leaves[ordinal] = leaf;
                            if matched {
                                states[optional].assert_coherent(&scorers[optional]);
                                leaves[optional] = scorers[optional].score();
                                states[optional].assert_coherent(&scorers[optional]);
                            }
                            let score = (f64::from(leaves[0]) + f64::from(leaves[1])) as Score;
                            if score > threshold {
                                let next_threshold = callback(doc, score);
                                debug_assert!(next_threshold >= threshold);
                                threshold = next_threshold;
                            }
                        }
                    }
                    states[ordinal].advance(&mut scorers[ordinal]);
                }
            }
        }
        // Rising thresholds only make the old local mode more conservative.
        // Do not hand stale optional cursors into the intersection batch path.
        lo = hi;
    }
}

/// Takes a term_scorers sorted by their current doc() and a threshold and returns
/// Returns (pivot_len, pivot_ord) defined as follows:
/// - `pivot_doc` lowest document that has a chance of exceeding (>) the threshold score.
/// - `before_pivot_len` number of term_scorers such that term_scorer.doc() < pivot.
/// - `pivot_len` number of term_scorers such that term_scorer.doc() <= pivot.
///
/// We always have `before_pivot_len` < `pivot_len`.
///
/// `None` is returned if we establish that no document can exceed the threshold.
fn find_pivot_doc(
    term_scorers: &[TermScorerWithMaxScore],
    threshold: Score,
    upper_bound: &ScoreSumUpperBound,
) -> Option<(usize, usize, DocId)> {
    let mut max_score = 0.0;
    let mut before_pivot_len = 0;
    let mut pivot_doc = TERMINATED;
    while before_pivot_len < term_scorers.len() {
        let term_scorer = &term_scorers[before_pivot_len];
        max_score += f64::from(term_scorer.max_score);
        if upper_bound.score(max_score) > threshold {
            pivot_doc = term_scorer.doc();
            break;
        }
        before_pivot_len += 1;
    }
    if pivot_doc == TERMINATED {
        return None;
    }
    // Right now i is an ordinal, we want a len.
    let mut pivot_len = before_pivot_len + 1;
    // Some other term_scorer may be positioned on the same document.
    pivot_len += term_scorers[pivot_len..]
        .iter()
        .take_while(|term_scorer| term_scorer.doc() == pivot_doc)
        .count();
    Some((before_pivot_len, pivot_len, pivot_doc))
}

/// Advance the scorer with best score among the scorers[..pivot_len] to
/// the next doc candidate defined by the min of `last_doc_in_block + 1` for
/// scorer in scorers[..pivot_len] and `scorer.doc()` for scorer in scorers[pivot_len..].
/// Note: before and after calling this method, scorers need to be sorted by their `.doc()`.
fn block_max_was_too_low_advance_one_scorer(
    scorers: &mut [TermScorerWithMaxScore],
    pivot_len: usize,
) {
    debug_assert!(scorers.iter().map(|scorer| scorer.doc()).is_sorted());
    let mut scorer_to_seek = pivot_len - 1;
    let mut global_max_score = scorers[scorer_to_seek].max_score;
    let mut doc_to_seek_after = scorers[scorer_to_seek].last_doc_in_block();
    for scorer_ord in (0..pivot_len - 1).rev() {
        let scorer = &scorers[scorer_ord];
        if scorer.last_doc_in_block() <= doc_to_seek_after {
            doc_to_seek_after = scorer.last_doc_in_block();
        }
        if scorers[scorer_ord].max_score > global_max_score {
            global_max_score = scorers[scorer_ord].max_score;
            scorer_to_seek = scorer_ord;
        }
    }
    // Add +1 to go to the next block unless we are already at the end.
    if doc_to_seek_after != TERMINATED {
        doc_to_seek_after += 1;
    }
    for scorer in &scorers[pivot_len..] {
        if scorer.doc() <= doc_to_seek_after {
            doc_to_seek_after = scorer.doc();
        }
    }
    scorers[scorer_to_seek].seek(doc_to_seek_after);

    restore_ordering(scorers, scorer_to_seek);
    debug_assert!(scorers.iter().map(|scorer| scorer.doc()).is_sorted());
}

// Given a list of term_scorers and a `ord` and assuming that `term_scorers[ord]` is sorted
// except term_scorers[ord] that might be in advance compared to its ranks,
// bubble up term_scorers[ord] in order to restore the ordering.
fn restore_ordering(term_scorers: &mut [TermScorerWithMaxScore], ord: usize) {
    let doc = term_scorers[ord].doc();
    for i in ord + 1..term_scorers.len() {
        if term_scorers[i].doc() >= doc {
            break;
        }
        term_scorers.swap(i, i - 1);
    }
    debug_assert!(term_scorers.iter().map(|scorer| scorer.doc()).is_sorted());
}

// Attempts to advance all term_scorers between `&term_scorers[0..before_len]` to the pivot.
// If this works, return true.
// If this fails (ie: one of the term_scorer does not contain `pivot_doc` and seek goes past the
// pivot), reorder the term_scorers to ensure the list is still sorted and returns `false`.
// If a term_scorer reach TERMINATED in the process return false remove the term_scorer and return.
fn align_scorers(
    term_scorers: &mut Vec<TermScorerWithMaxScore>,
    pivot_doc: DocId,
    before_pivot_len: usize,
) -> bool {
    debug_assert_ne!(pivot_doc, TERMINATED);
    for i in (0..before_pivot_len).rev() {
        let new_doc = term_scorers[i].seek(pivot_doc);
        if new_doc != pivot_doc {
            if new_doc == TERMINATED {
                term_scorers.swap_remove(i);
            }
            // We went past the pivot.
            // We just go through the outer loop mechanic (Note that pivot is
            // still a possible candidate).
            //
            // Termination is still guaranteed since we can only consider the same
            // pivot at most term_scorers.len() - 1 times.
            restore_ordering(term_scorers, i);
            return false;
        }
    }
    true
}

// Assumes terms_scorers[..pivot_len] are positioned on the same doc (pivot_doc).
// Advance term_scorers[..pivot_len] and out of these removes the terminated scores.
// Restores the ordering of term_scorers.
fn advance_all_scorers_on_pivot(term_scorers: &mut Vec<TermScorerWithMaxScore>, pivot_len: usize) {
    for term_scorer in &mut term_scorers[..pivot_len] {
        term_scorer.advance();
    }
    // TODO use drain_filter when available.
    let mut i = 0;
    while i != term_scorers.len() {
        if term_scorers[i].doc() == TERMINATED {
            term_scorers.swap_remove(i);
        } else {
            i += 1;
        }
    }
    term_scorers.sort_by_key(|scorer| scorer.doc());
}

/// Implements the WAND (Weak AND) algorithm for dynamic pruning
/// described in the paper "Faster Top-k Document Retrieval Using Block-Max Indexes".
/// Link: <http://engineering.nyu.edu/~suel/papers/bmw.pdf>
pub fn block_wand(
    mut scorers: Vec<TermScorer>,
    mut threshold: Score,
    callback: &mut dyn FnMut(u32, Score) -> Score,
) {
    scorers.retain(|scorer| scorer.doc() < TERMINATED);
    let upper_bound = ScoreSumUpperBound::new(scorers.len());
    if scorers.len() == 1 {
        let scorer = scorers.pop().unwrap();
        return block_wand_single_scorer(scorer, threshold, callback);
    }
    let mut scorers: Vec<TermScorerWithMaxScore> = scorers
        .iter_mut()
        .map(TermScorerWithMaxScore::from)
        .collect();
    // At this point we need to ensure that the scorers are sorted!
    scorers.sort_by_key(|scorer| scorer.doc());
    while let Some((before_pivot_len, pivot_len, pivot_doc)) =
        find_pivot_doc(&scorers[..], threshold, &upper_bound)
    {
        debug_assert!(scorers.iter().map(|scorer| scorer.doc()).is_sorted());
        debug_assert_ne!(pivot_doc, TERMINATED);
        debug_assert!(before_pivot_len < pivot_len);

        let block_max_score_sum: f64 = scorers[..pivot_len]
            .iter_mut()
            .map(|scorer| {
                scorer.seek_block(pivot_doc);
                f64::from(scorer.block_max_score())
            })
            .sum();

        // Beware after shallow advance, skip readers can be in advance compared to
        // the segment posting lists.
        //
        // `block_segment_postings.load_block()` need to be called separately.
        if upper_bound.score(block_max_score_sum) <= threshold {
            // Block max condition was not reached
            // We could get away by simply advancing the scorers to DocId + 1 but it would
            // be inefficient. The optimization requires proper explanation and was
            // isolated in a different function.
            block_max_was_too_low_advance_one_scorer(&mut scorers, pivot_len);
            continue;
        }

        // Block max condition is observed.
        //
        // Let's try and advance all scorers before the pivot to the pivot.
        if !align_scorers(&mut scorers, pivot_doc, before_pivot_len) {
            // At least of the scorer does not contain the pivot.
            //
            // Let's stop scoring this pivot and go through the pivot selection again.
            // Note that the current pivot is not necessarily a bad candidate and it
            // may be picked again.
            continue;
        }

        // At this point, all scorers are positioned on the doc.
        let score = scorers[..pivot_len]
            .iter_mut()
            .map(|scorer| f64::from(scorer.score()))
            .sum::<f64>() as Score;

        if score > threshold {
            threshold = callback(pivot_doc, score);
        }
        // let's advance all of the scorers that are currently positioned on the pivot.
        advance_all_scorers_on_pivot(&mut scorers, pivot_len);
    }
}

/// Specialized version of [`block_wand`] for a single scorer.
/// In this case, the algorithm is simple, readable and faster (~ x3)
/// than the generic algorithm.
/// The algorithm behaves as follows:
/// - While we don't hit the end of the docset:
///   - While the block max score is under the `threshold`, go to the next block.
///   - On a block, advance until the end and execute `callback` when the doc score is greater or
///     equal to the `threshold`.
pub fn block_wand_single_scorer(
    mut scorer: TermScorer,
    mut threshold: Score,
    callback: &mut dyn FnMut(u32, Score) -> Score,
) {
    let mut doc = scorer.doc();
    loop {
        // We position the scorer on a block that can reach
        // the threshold.
        while scorer.block_max_score() <= threshold {
            let last_doc_in_block = scorer.last_doc_in_block();
            if last_doc_in_block == TERMINATED {
                return;
            }
            doc = last_doc_in_block + 1;
            scorer.seek_block(doc);
        }
        // Seek will effectively load that block.
        doc = scorer.seek(doc);
        if doc == TERMINATED {
            break;
        }
        loop {
            let score = scorer.score();
            if score > threshold {
                threshold = callback(doc, score);
            }
            debug_assert!(doc <= scorer.last_doc_in_block());
            if doc == scorer.last_doc_in_block() {
                break;
            }
            doc = scorer.advance();
            if doc == TERMINATED {
                return;
            }
        }
        doc += 1;
        scorer.seek_block(doc);
    }
}

struct TermScorerWithMaxScore<'a> {
    scorer: &'a mut TermScorer,
    max_score: Score,
}

impl<'a> From<&'a mut TermScorer> for TermScorerWithMaxScore<'a> {
    fn from(scorer: &'a mut TermScorer) -> Self {
        let max_score = scorer.max_score();
        TermScorerWithMaxScore { scorer, max_score }
    }
}

impl Deref for TermScorerWithMaxScore<'_> {
    type Target = TermScorer;

    fn deref(&self) -> &Self::Target {
        self.scorer
    }
}

impl DerefMut for TermScorerWithMaxScore<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.scorer
    }
}

#[cfg(test)]
mod tests {
    use std::cmp::Ordering;
    use std::collections::BinaryHeap;

    use proptest::prelude::*;

    use crate::query::score_combiner::SumCombiner;
    use crate::query::term_query::TermScorer;
    use crate::query::{Bm25Weight, BufferedUnionScorer, Scorer};
    use crate::{DocId, DocSet, Score, TERMINATED};

    struct Float(Score);

    impl Eq for Float {}

    impl PartialEq for Float {
        fn eq(&self, other: &Self) -> bool {
            self.cmp(other) == Ordering::Equal
        }
    }

    impl PartialOrd for Float {
        fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
            Some(self.cmp(other))
        }
    }

    impl Ord for Float {
        fn cmp(&self, other: &Self) -> Ordering {
            other.0.partial_cmp(&self.0).unwrap_or(Ordering::Equal)
        }
    }

    fn nearly_equals(left: Score, right: Score) -> bool {
        (left - right).abs() < 0.0001 * (left + right).abs()
    }

    fn compute_checkpoints_for_each_pruning(
        mut term_scorers: Vec<TermScorer>,
        n: usize,
        max_doc: DocId,
    ) -> Vec<(DocId, Score)> {
        let mut heap: BinaryHeap<Float> = BinaryHeap::with_capacity(n);
        let mut checkpoints: Vec<(DocId, Score)> = Vec::new();
        let mut limit: Score = 0.0;

        let callback = &mut |doc, score| {
            heap.push(Float(score));
            if heap.len() > n {
                heap.pop().unwrap();
            }
            if heap.len() == n {
                limit = heap.peek().unwrap().0;
            }
            if !nearly_equals(score, limit) {
                checkpoints.push((doc, score));
            }
            limit
        };

        if term_scorers.len() == 1 {
            let scorer = term_scorers.pop().unwrap();
            super::block_wand_single_scorer(scorer, Score::MIN, callback);
        } else if term_scorers.len() == 2 {
            super::two_term_or_maxscore(term_scorers, max_doc, Score::MIN, callback);
        } else {
            super::block_wand(term_scorers, Score::MIN, callback);
        }
        checkpoints
    }

    fn compute_checkpoints_manual(
        term_scorers: Vec<TermScorer>,
        n: usize,
        max_doc: u32,
    ) -> Vec<(DocId, Score)> {
        let mut heap: BinaryHeap<Float> = BinaryHeap::with_capacity(n);
        let mut checkpoints: Vec<(DocId, Score)> = Vec::new();
        let mut scorer = BufferedUnionScorer::build(term_scorers, SumCombiner::default, max_doc);

        let mut limit = Score::MIN;
        loop {
            if scorer.doc() == TERMINATED {
                break;
            }
            let doc = scorer.doc();
            let score = scorer.score();
            if score > limit {
                heap.push(Float(score));
                if heap.len() > n {
                    heap.pop().unwrap();
                }
                if heap.len() == n {
                    limit = heap.peek().unwrap().0;
                }
                if !nearly_equals(score, limit) {
                    checkpoints.push((doc, score));
                }
            }
            scorer.advance();
        }
        checkpoints
    }

    const MAX_TERM_FREQ: u32 = 100u32;

    fn two_term_fixture(postings: &[(DocId, u32)], norms: &[u32], boost: Score) -> TermScorer {
        let average =
            norms.iter().map(|&norm| u64::from(norm)).sum::<u64>() as Score / norms.len() as Score;
        let weight = Bm25Weight::new_without_explain(1.0, average).boost_by(boost);
        TermScorer::create_for_test(postings, norms, weight)
    }

    // Unlike the older near-equality checks, retain every callback and its raw
    // score bits. Collector rejection leaves the competitive threshold intact.
    fn exact_two_trace(
        initial: Score,
        limit: usize,
        deleted: &[DocId],
        run: impl FnOnce(&mut dyn FnMut(DocId, Score) -> Score),
    ) -> Vec<(DocId, u32)> {
        let mut accepted = Vec::new();
        let mut best = Vec::new();
        let mut threshold = initial;
        run(&mut |doc, score| {
            accepted.push((doc, score.to_bits()));
            if limit == 0 || deleted.contains(&doc) {
                return threshold;
            }
            best.push((score, doc));
            best.sort_unstable_by(|left, right| {
                right
                    .0
                    .total_cmp(&left.0)
                    .then_with(|| left.1.cmp(&right.1))
            });
            best.truncate(limit);
            if best.len() == limit {
                threshold = initial.max(best.last().unwrap().0);
            }
            threshold
        });
        assert!(accepted.windows(2).all(|pair| pair[0].0 < pair[1].0));
        accepted
    }

    fn check_two_exact(
        scorers: Vec<TermScorer>,
        max_doc: DocId,
        threshold: Score,
        limit: usize,
        direct_regions: bool,
        deleted: &[DocId],
    ) -> Vec<(DocId, u32)> {
        let expected = exact_two_trace(threshold, limit, deleted, |callback| {
            let mut union =
                BufferedUnionScorer::build(scorers.clone(), SumCombiner::default, max_doc);
            crate::query::weight::for_each_pruning_scorer(&mut union, threshold, callback);
        });
        let actual = exact_two_trace(threshold, limit, deleted, |callback| {
            if direct_regions {
                super::two_term_regions(scorers, max_doc, threshold, callback);
            } else {
                super::two_term_or_maxscore(scorers, max_doc, threshold, callback);
            }
        });
        assert_eq!(actual, expected, "threshold={threshold:?} limit={limit}");
        actual
    }

    #[test]
    fn two_term_exact_region_roles_reverse_after_shallow_skips() {
        let norms = vec![1; 800];
        let changing: Vec<_> = (0..512)
            .map(|doc| (doc, if doc < 256 { 1 } else { 1000 }))
            .collect();
        let sparse = [(0, 1), (700, 1)];
        let scorers = vec![
            two_term_fixture(&changing, &norms, 0.1),
            two_term_fixture(&sparse, &norms, 1.0),
        ];
        for reverse in [false, true] {
            let mut ordered = scorers.clone();
            if reverse {
                ordered.reverse();
            }
            super::TWO_TERM_REGIONS.with(|regions| *regions.borrow_mut() = Some(Vec::new()));
            let accepted = check_two_exact(ordered.clone(), 800, 0.15, 0, true, &[]);
            let choices =
                super::TWO_TERM_REGIONS.with(|regions| regions.borrow_mut().take().unwrap());
            let first_kind = if reverse { 1 } else { 2 };
            let later_kind = if reverse { 2 } else { 1 };
            let first = choices
                .iter()
                .position(|&(_, _, kind)| kind == first_kind)
                .unwrap();
            let later = choices
                .iter()
                .position(|&(_, _, kind)| kind == later_kind)
                .unwrap();
            assert!(first < later, "region choices: {choices:?}");
            assert_eq!(accepted.len(), 258);
            assert_eq!(accepted[0].0, 0);
            assert_eq!(accepted[1].0, 256);
            assert_eq!(accepted.last().unwrap().0, 700);
            for limit in [0, 1, 10] {
                check_two_exact(ordered.clone(), 800, 0.15, limit, false, &[]);
            }
        }
    }

    #[test]
    fn two_term_exact_local_required_mode_expires_before_single_term_winner() {
        let norms = vec![1; 800];
        let changing: Vec<_> = (0..512)
            .map(|doc| (doc, if doc < 256 { 1 } else { 1000 }))
            .collect();
        let early: Vec<_> = (0..256).map(|doc| (doc, 1)).collect();
        let scorers = vec![
            two_term_fixture(&changing, &norms, 0.1),
            two_term_fixture(&early, &norms, 0.1),
        ];
        for reverse in [false, true] {
            let mut ordered = scorers.clone();
            if reverse {
                ordered.reverse();
            }
            super::TWO_TERM_REGIONS.with(|regions| *regions.borrow_mut() = Some(Vec::new()));
            let accepted = check_two_exact(ordered.clone(), 800, 0.15, 0, true, &[]);
            let choices =
                super::TWO_TERM_REGIONS.with(|regions| regions.borrow_mut().take().unwrap());
            let required_kind = if reverse { 3 } else { 4 };
            let later_kind = if reverse { 2 } else { 1 };
            let required = choices
                .iter()
                .position(|&(_, _, kind)| kind == required_kind)
                .unwrap();
            let later = choices
                .iter()
                .position(|&(_, _, kind)| kind == later_kind)
                .unwrap();
            assert!(required < later, "region choices: {choices:?}");
            assert_eq!(accepted.len(), 512);
            assert_eq!(
                accepted[256].0, 256,
                "later hit does not contain the old required term"
            );
            for limit in [0, 1, 10] {
                for direct in [false, true] {
                    check_two_exact(ordered.clone(), 800, 0.15, limit, direct, &[]);
                }
            }
        }
    }

    #[test]
    fn two_term_exact_complete_blocks_stale_tails_and_exhaustion() {
        let norms: Vec<_> = (0..1024).map(|doc| 1 + doc % 13).collect();
        for length in [1, 127, 128, 129, 255, 256, 257, 600] {
            let dense: Vec<_> = (0..length).map(|doc| (doc, 1 + doc % 5)).collect();
            let gapped = [(0, 1), (128, 1), (256, 1), (900, 1)];
            let scorers = vec![
                two_term_fixture(&dense, &norms, 0.1),
                two_term_fixture(&gapped, &norms, 0.1),
            ];
            for reverse in [false, true] {
                let mut ordered = scorers.clone();
                if reverse {
                    ordered.reverse();
                }
                for threshold in [-Score::INFINITY, 0.0, 0.15, 0.4, Score::INFINITY] {
                    for limit in [0, 1, 10] {
                        for direct in [false, true] {
                            check_two_exact(ordered.clone(), 1024, threshold, limit, direct, &[]);
                        }
                    }
                }
            }
        }
        // Dense decoder remains stale as full blocks are skipped, then the
        // selected tail needs real reconciliation; the late sparse hit has no
        // dense counterpart and must fail the local required-membership test.
        let norms = vec![1; 1024];
        let dense: Vec<_> = (0..600).map(|doc| (doc, 1)).collect();
        let late = [(0, 1), (900, 1)];
        let scorers = vec![
            two_term_fixture(&dense, &norms, 0.1),
            two_term_fixture(&late, &norms, 0.1),
        ];
        for direct in [false, true] {
            let accepted = check_two_exact(scorers.clone(), 1024, 0.15, 0, direct, &[]);
            assert_eq!(
                accepted.iter().map(|&(doc, _)| doc).collect::<Vec<_>>(),
                [0]
            );
        }
    }

    #[test]
    fn two_term_exact_midpoints_ties_and_warm_boundary_handoffs() {
        let norms = vec![1; 260];
        let postings: Vec<_> = (0..260).map(|doc| (doc, 1)).collect();
        for tiny in [2.0f32.powi(-24), 2.0f32.powi(-24).next_up()] {
            let scorers = vec![
                two_term_fixture(&postings, &norms, 1.0),
                two_term_fixture(&postings, &norms, tiny),
            ];
            assert_eq!(scorers[0].clone().score().to_bits(), 1.0f32.to_bits());
            assert_eq!(scorers[1].clone().score().to_bits(), tiny.to_bits());
            let expected = (1.0f64 + f64::from(tiny)) as Score;
            for reverse in [false, true] {
                let mut ordered = scorers.clone();
                if reverse {
                    ordered.reverse();
                }
                for threshold in [expected.next_down(), expected, expected.next_up()] {
                    for limit in [0, 1, 10] {
                        for direct in [false, true] {
                            let accepted = check_two_exact(
                                ordered.clone(),
                                260,
                                threshold,
                                limit,
                                direct,
                                &[],
                            );
                            if threshold >= expected {
                                assert!(accepted.is_empty());
                            } else {
                                assert_eq!(accepted.len(), if limit == 0 { 260 } else { limit });
                                assert!(accepted
                                    .iter()
                                    .all(|&(_, bits)| bits == expected.to_bits()));
                            }
                        }
                    }
                }
            }
        }
        let norms = vec![1; 300];
        for winning_doc in [127, 128, 129] {
            let first: Vec<_> = (0..270)
                .map(|doc| (doc, if doc == winning_doc { 1000 } else { 1 }))
                .collect();
            let second: Vec<_> = (0..270).map(|doc| (doc, 1)).collect();
            let scorers = vec![
                two_term_fixture(&first, &norms, 0.1),
                two_term_fixture(&second, &norms, 0.1),
            ];
            for reverse in [false, true] {
                let mut ordered = scorers.clone();
                if reverse {
                    ordered.reverse();
                }
                for direct in [false, true] {
                    let accepted = check_two_exact(ordered.clone(), 300, 0.15, 1, direct, &[]);
                    assert_eq!(
                        accepted.iter().map(|&(doc, _)| doc).collect::<Vec<_>>(),
                        [0, winning_doc]
                    );
                }
            }
        }
    }

    #[test]
    fn two_term_exact_signed_and_exceptional_domains_use_canonical_fallback() {
        let norms = vec![1; 300];
        let first: Vec<_> = (0..260).map(|doc| (doc, 1)).collect();
        let second: Vec<_> = (0..260)
            .filter(|doc| doc % 2 == 0)
            .map(|doc| (doc, 1))
            .collect();
        for boost in [
            0.0,
            -0.0,
            -1.0,
            Score::INFINITY,
            -Score::INFINITY,
            Score::NAN,
        ] {
            let scorers = vec![
                two_term_fixture(&first, &norms, boost),
                two_term_fixture(&second, &norms, 1.0),
            ];
            for reverse in [false, true] {
                let mut ordered = scorers.clone();
                if reverse {
                    ordered.reverse();
                }
                for threshold in [Score::MIN, 0.0, Score::NAN, Score::INFINITY] {
                    for limit in [0, 1, 10] {
                        check_two_exact(ordered.clone(), 300, threshold, limit, false, &[]);
                    }
                }
            }
        }
    }

    #[test]
    fn two_term_exact_collector_rejection_preserves_physical_range() {
        let norms = vec![1; 1024];
        let first: Vec<_> = (0..600)
            .map(|doc| (doc, if doc == 128 || doc == 599 { 1000 } else { 1 }))
            .collect();
        let second: Vec<_> = (0..600).map(|doc| (doc, 1)).collect();
        let scorers = vec![
            two_term_fixture(&first, &norms, 0.1),
            two_term_fixture(&second, &norms, 0.1),
        ];
        // Reject a warm hit, the first post-warm winner, and the last indexed
        // physical doc; none of these may raise the collector's threshold.
        for reverse in [false, true] {
            let mut ordered = scorers.clone();
            if reverse {
                ordered.reverse();
            }
            for direct in [false, true] {
                let accepted =
                    check_two_exact(ordered.clone(), 1024, 0.15, 1, direct, &[0, 128, 599]);
                assert_eq!(
                    accepted.iter().map(|&(doc, _)| doc).collect::<Vec<_>>(),
                    [0, 1, 128, 599]
                );
            }
        }
        eprintln!(
            "TwoTermState bytes={}",
            std::mem::size_of::<super::TwoTermState>()
        );
    }

    #[test]
    fn test_two_term_or_preserves_single_term_winner_with_sparse_quantized_field(
    ) -> crate::Result<()> {
        use crate::collector::TopDocs;
        use crate::query::{BooleanQuery, Occur, Query, TermQuery};
        use crate::schema::{IndexRecordOption, Schema, TEXT};
        use crate::{Index, Term};

        let mut schema = Schema::builder();
        let field = schema.add_text_field("text", TEXT);
        let index = Index::create_in_ram(schema.build());
        let mut writer = index.writer_for_tests()?;
        writer.add_document(
            doc!(field => std::iter::repeat_n("a", 62).collect::<Vec<_>>().join(" ")),
        )?;
        writer.add_document(
            doc!(field => std::iter::repeat_n("a", 63).collect::<Vec<_>>().join(" ")),
        )?;
        for _ in 0..128 {
            writer.add_document(doc!(field => "b"))?;
        }
        for _ in 0..126 {
            writer.add_document(crate::TantivyDocument::default())?;
        }
        writer.commit()?;
        let searcher = index.reader()?.searcher();
        assert_eq!(searcher.segment_readers().len(), 1);
        let query = BooleanQuery::new(
            ["a", "b"]
                .into_iter()
                .map(|term| {
                    (
                        Occur::Should,
                        Box::new(TermQuery::new(
                            Term::from_field_text(field, term),
                            IndexRecordOption::WithFreqs,
                        )) as Box<dyn Query>,
                    )
                })
                .collect(),
        );
        let top = searcher.search(&query, &TopDocs::with_limit(1).order_by_score())?;
        assert_eq!(top.len(), 1);
        assert_eq!(
            top[0].1.doc_id, 1,
            "the later a-only document must not be discarded by OR-to-AND pruning"
        );
        Ok(())
    }

    fn posting_list(max_doc: u32) -> BoxedStrategy<Vec<(DocId, u32)>> {
        (1..max_doc + 1)
            .prop_flat_map(move |doc_freq| {
                (
                    proptest::bits::bitset::sampled(doc_freq as usize, 0..max_doc as usize),
                    proptest::collection::vec(1u32..MAX_TERM_FREQ, doc_freq as usize),
                )
            })
            .prop_map(|(docset, term_freqs)| {
                docset
                    .iter()
                    .map(|doc| doc as u32)
                    .zip(term_freqs.iter().cloned())
                    .collect::<Vec<_>>()
            })
            .boxed()
    }

    #[expect(clippy::type_complexity)]
    fn gen_term_scorers(num_scorers: usize) -> BoxedStrategy<(Vec<Vec<(DocId, u32)>>, Vec<u32>)> {
        (1u32..100u32)
            .prop_flat_map(move |max_doc: u32| {
                (
                    proptest::collection::vec(posting_list(max_doc), num_scorers),
                    proptest::collection::vec(2u32..10u32 * MAX_TERM_FREQ, max_doc as usize),
                )
            })
            .boxed()
    }

    fn test_block_wand_aux(posting_lists: &[Vec<(DocId, u32)>], fieldnorms: &[u32]) {
        // We virtually repeat all docs 64 times in order to emulate blocks of 2 documents
        // and surface blogs more easily.
        const REPEAT: usize = 64;
        let fieldnorms_expanded = fieldnorms
            .iter()
            .cloned()
            .flat_map(|fieldnorm| std::iter::repeat_n(fieldnorm, REPEAT))
            .collect::<Vec<u32>>();

        let postings_lists_expanded: Vec<Vec<(DocId, u32)>> = posting_lists
            .iter()
            .map(|posting_list| {
                posting_list
                    .iter()
                    .cloned()
                    .flat_map(|(doc, term_freq)| {
                        (0_u32..REPEAT as u32).map(move |offset| {
                            (
                                doc * (REPEAT as u32) + offset,
                                if offset == 0 { term_freq } else { 1 },
                            )
                        })
                    })
                    .collect::<Vec<(DocId, u32)>>()
            })
            .collect::<Vec<_>>();

        let total_fieldnorms: u64 = fieldnorms_expanded
            .iter()
            .cloned()
            .map(|fieldnorm| fieldnorm as u64)
            .sum();
        let average_fieldnorm = (total_fieldnorms as Score) / (fieldnorms_expanded.len() as Score);
        let max_doc = fieldnorms_expanded.len();

        let term_scorers: Vec<TermScorer> = postings_lists_expanded
            .iter()
            .map(|postings| {
                let bm25_weight = Bm25Weight::for_one_term(
                    postings.len() as u64,
                    max_doc as u64,
                    average_fieldnorm,
                );
                TermScorer::create_for_test(postings, &fieldnorms_expanded[..], bm25_weight)
            })
            .collect();
        for top_k in 1..4 {
            let checkpoints_for_each_pruning =
                compute_checkpoints_for_each_pruning(term_scorers.clone(), top_k, max_doc as u32);
            let checkpoints_manual =
                compute_checkpoints_manual(term_scorers.clone(), top_k, max_doc as u32);
            assert_eq!(checkpoints_for_each_pruning.len(), checkpoints_manual.len());
            for (&(left_doc, left_score), &(right_doc, right_score)) in checkpoints_for_each_pruning
                .iter()
                .zip(checkpoints_manual.iter())
            {
                assert_eq!(left_doc, right_doc);
                assert!(nearly_equals(left_score, right_score));
            }
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(500))]
        #[test]
        fn test_block_wand_two_term_scorers((posting_lists, fieldnorms) in gen_term_scorers(2)) {
            test_block_wand_aux(&posting_lists[..], &fieldnorms[..]);
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(500))]
        #[test]
        fn test_block_wand_single_term_scorer((posting_lists, fieldnorms) in gen_term_scorers(1)) {
            test_block_wand_aux(&posting_lists[..], &fieldnorms[..]);
        }
    }

    #[test]
    fn test_fn_reproduce_proptest() {
        let postings_lists = &[
            vec![
                (0, 1),
                (1, 1),
                (2, 1),
                (3, 1),
                (4, 1),
                (6, 1),
                (7, 7),
                (8, 1),
                (10, 1),
                (12, 1),
                (13, 1),
                (14, 1),
                (15, 1),
                (16, 1),
                (19, 1),
                (20, 1),
                (21, 1),
                (22, 1),
                (24, 1),
                (25, 1),
                (26, 1),
                (28, 1),
                (30, 1),
                (31, 1),
                (33, 1),
                (34, 1),
                (35, 1),
                (36, 95),
                (37, 1),
                (39, 1),
                (41, 1),
                (44, 1),
                (46, 1),
            ],
            vec![
                (0, 5),
                (2, 1),
                (4, 1),
                (5, 84),
                (6, 47),
                (7, 26),
                (8, 50),
                (9, 34),
                (11, 73),
                (12, 11),
                (13, 51),
                (14, 45),
                (15, 18),
                (18, 60),
                (19, 80),
                (20, 63),
                (23, 79),
                (24, 69),
                (26, 35),
                (28, 82),
                (29, 19),
                (30, 2),
                (31, 7),
                (33, 40),
                (34, 1),
                (35, 33),
                (36, 27),
                (37, 24),
                (38, 65),
                (39, 32),
                (40, 85),
                (41, 1),
                (42, 69),
                (43, 11),
                (45, 45),
                (47, 97),
            ],
            vec![
                (2, 1),
                (4, 1),
                (7, 94),
                (8, 1),
                (9, 1),
                (10, 1),
                (12, 1),
                (15, 1),
                (22, 1),
                (23, 1),
                (26, 1),
                (27, 1),
                (32, 1),
                (33, 1),
                (34, 1),
                (36, 96),
                (39, 1),
                (41, 1),
            ],
        ];
        let fieldnorms = &[
            685, 239, 780, 564, 664, 827, 5, 56, 930, 887, 263, 665, 167, 127, 120, 919, 292, 92,
            489, 734, 814, 724, 700, 304, 128, 779, 311, 877, 774, 15, 866, 368, 894, 371, 982,
            502, 507, 669, 680, 76, 594, 626, 578, 331, 170, 639, 665, 186,
        ][..];
        test_block_wand_aux(postings_lists, fieldnorms);
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(500))]
        #[ignore]
        #[test]
        #[ignore]
        fn test_block_wand_three_term_scorers((posting_lists, fieldnorms) in gen_term_scorers(3)) {
            test_block_wand_aux(&posting_lists[..], &fieldnorms[..]);
        }
    }
}
