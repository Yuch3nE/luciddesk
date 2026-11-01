"""Guard test discovery so renamed/missing groups cannot silently pass CI."""
import importlib.util
from pathlib import Path
import unittest
from types import SimpleNamespace

spec = importlib.util.spec_from_file_location("control_ci", Path(__file__).with_name("test-control-ci.py"))
control_ci = importlib.util.module_from_spec(spec)
spec.loader.exec_module(control_ci)


class SelectionTests(unittest.TestCase):
    def test_batches_preserve_native_isolation_and_all_selected_names(self):
        pure = [prefix + "case" for prefix in control_ci.BATCHED_FILTERS]
        native = ["pane::fixed_grid::tests::case", "pane::sorting::tests::case"]
        batches = control_ci.test_batches([native[0], *pure, native[1]])
        self.assertEqual(batches, [pure, [native[0]], [native[1]]])
        self.assertEqual(control_ci.test_batches(native), [[name] for name in native])
        self.assertEqual(control_ci.test_batches([]), [])

    def test_batch_result_rejects_partial_ignored_and_failed_runs(self):
        for code, passed, ignored, valid in [(0, 3, 0, True), (0, 2, 0, False),
                                            (0, 3, 1, False), (1, 3, 0, False), (0, 13, 0, False)]:
            result = SimpleNamespace(returncode=code,
                stdout=f"test result: ok. {passed} passed; 0 failed; {ignored} ignored;")
            with self.subTest(code=code, passed=passed, ignored=ignored):
                self.assertEqual(control_ci.batch_passed(result, 3), valid)

    def test_selects_tests_only_and_preserves_exact_names(self):
        listing = "group::one: test\ngroup::two: test\ngroup::bench: benchmark\nother::one: test\n3 tests, 1 benchmark"
        self.assertEqual(control_ci.select_tests(listing, ("group::",)), ["group::one", "group::two"])

    def test_missing_group_fails_even_when_other_groups_match(self):
        with self.assertRaisesRegex(RuntimeError, "missing::"):
            control_ci.select_tests("present::one: test", ("present::", "missing::"))

    def test_empty_or_benchmark_only_output_fails(self):
        for listing in ("", "group::bench: benchmark"):
            with self.subTest(listing=listing), self.assertRaises(RuntimeError):
                control_ci.select_tests(listing, ("group::",))


if __name__ == "__main__":
    unittest.main()
