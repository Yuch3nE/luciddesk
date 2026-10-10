"""Guard test discovery so renamed/missing groups cannot silently pass CI."""
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("control_ci", Path(__file__).with_name("test-control-ci.py"))
control_ci = importlib.util.module_from_spec(spec)
spec.loader.exec_module(control_ci)


class SelectionTests(unittest.TestCase):
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
