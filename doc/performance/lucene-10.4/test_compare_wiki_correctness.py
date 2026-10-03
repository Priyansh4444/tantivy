"""Small dump-fixture gate checks; never launches either benchmark engine."""
import copy
import unittest

from compare_wiki_correctness import compare_results, validate_tie_equivalent_top10


def row(docs):
    return {'count': len(docs), 'top100': docs,
            'count_matches_exhaustive': True, 'ranking_matches_exhaustive': True}


class ComparisonGateTests(unittest.TestCase):
    def setUp(self):
        self.baseline = row([{'id': str(i), 'score': 20.0 - i} for i in range(11)])

    def test_equal_results_and_empty_results_pass(self):
        compare_results('fixture', self.baseline, copy.deepcopy(self.baseline))
        compare_results('empty', row([]), row([]))

    def test_float_tie_reordering_passes(self):
        t = row([{'id': 'a', 'score': 1.0}, {'id': 'b', 'score': 1.0}])
        l = row([{'id': 'b', 'score': 1.0000001}, {'id': 'a', 'score': 1.0}])
        result = compare_results('tie', t, l)
        self.assertFalse(result['same_top10_id_order'])
        self.assertTrue(result['top10_order_matches_within_score_ties'])

    def test_duplicate_nan_infinite_and_truncated_dumps_fail(self):
        corruptions = []
        duplicate = copy.deepcopy(self.baseline)
        duplicate['top100'][-1]['id'] = duplicate['top100'][0]['id']
        corruptions.append(duplicate)
        for value in [float('nan'), float('inf'), float('-inf')]:
            corrupt = copy.deepcopy(self.baseline)
            corrupt['top100'][0]['score'] = value
            corruptions.append(corrupt)
        truncated = copy.deepcopy(self.baseline)
        truncated['top100'].pop()
        corruptions.append(truncated)
        for corrupt in corruptions:
            with self.subTest(corrupt=corrupt), self.assertRaises(ValueError):
                compare_results('corrupt', self.baseline, corrupt)

    def test_score_mismatch_and_false_internal_oracle_fail(self):
        score_mismatch = copy.deepcopy(self.baseline)
        score_mismatch['top100'][0]['score'] += 0.1
        with self.assertRaises(ValueError):
            compare_results('scores', self.baseline, score_mismatch)
        false_oracle = copy.deepcopy(self.baseline)
        false_oracle['ranking_matches_exhaustive'] = False
        with self.assertRaises(ValueError):
            compare_results('internal', false_oracle, self.baseline)

    def test_non_tie_order_inversion_fails(self):
        inverted = copy.deepcopy(self.baseline)
        inverted['top100'][:2] = inverted['top100'][:2][::-1]
        with self.assertRaises(ValueError):
            validate_tie_equivalent_top10(self.baseline, inverted)
        with self.assertRaises(ValueError):
            compare_results('inversion', self.baseline, inverted)

    def test_cutoff_replacement_requires_ties_in_both_models(self):
        t = copy.deepcopy(self.baseline)
        for doc in t['top100'][9:]:
            doc['score'] = 1.0
        l = copy.deepcopy(t)
        l['top100'][9:] = l['top100'][9:][::-1]
        validate_tie_equivalent_top10(t, l)  # Both boundary choices are genuine ties.
        with self.assertRaises(ValueError):
            compare_results('strict frozen IDs', t, l)  # Frozen suite also demands the exact set.
        l['top100'][9]['score'] = 1.1
        with self.assertRaises(ValueError):
            validate_tie_equivalent_top10(t, l)  # One scoring model resolves the cutoff.
        l['top100'].pop()
        with self.assertRaises(ValueError):
            validate_tie_equivalent_top10(t, l)  # Missing cross-engine boundary evidence.


if __name__ == '__main__':
    unittest.main()
