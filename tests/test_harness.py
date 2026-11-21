import contextlib
import io
import json
import re
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import harness  # noqa: E402

SOURCE = Path(harness.__file__).read_text()


def run_main(argv):
    out = io.StringIO()
    err = io.StringIO()
    with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
        code = harness.main(argv)
    return code, out.getvalue(), err.getvalue()


class GateRegistryTest(unittest.TestCase):
    def test_suites_name_only_registered_gates_once(self):
        for suite in (harness.SUITE, harness.LINUX_SUITE):
            self.assertEqual(len(suite), len(set(suite)))
            self.assertEqual([name for name in suite if name not in harness.GATES], [])

    def test_gates_are_callables_taking_results_and_arguments(self):
        for name, handler in harness.GATES.items():
            self.assertTrue(callable(handler), name)
            self.assertEqual(handler.__code__.co_argcount, 2, name)

    def test_every_fixture_the_gates_name_is_in_the_manifest(self):
        manifest = json.loads(harness.MANIFEST_PATH.read_text())["fixtures"]
        named = set(re.findall(r'fixture_entry\("([^"]+)"\)', SOURCE))
        self.assertTrue(named)
        self.assertEqual(sorted(named - set(manifest)), [])


class ExitCodeTest(unittest.TestCase):
    def test_results_exit_zero_only_when_every_check_passed(self):
        passing = harness.Results()
        failing = harness.Results()
        with contextlib.redirect_stdout(io.StringIO()):
            passing.check("a", True)
            failing.check("a", True)
            failing.check("b", False, "why")
            self.assertEqual(passing.finish(), 0)
            self.assertEqual(failing.finish(), 1)

    def test_no_gate_prints_usage_and_exits_two(self):
        code, _, err = run_main(["run.sh"])
        self.assertEqual(code, 2)
        self.assertIn("usage: tests/run.sh <gate>", err)
        self.assertIn("suite", err)

    def test_unknown_gate_fails_the_run(self):
        code, out, _ = run_main(["run.sh", "no-such-gate"])
        self.assertEqual(code, 1)
        self.assertIn("FAIL no-such-gate: unknown gate", out)
        self.assertIn("TOTAL 0 passed, 1 failed", out)

    def test_a_gate_that_raises_is_a_failed_check_not_a_crash(self):
        def explode(results, args):
            raise harness.Failure("boom")

        def crash(results, args):
            raise KeyError("missing")

        saved = dict(harness.GATES)
        harness.GATES.update(explode=explode, crash=crash)
        try:
            for name, expected in (("explode", "FAIL explode: boom"), ("crash", "FAIL crash: KeyError")):
                with contextlib.redirect_stderr(io.StringIO()):
                    code, out, _ = run_main(["run.sh", name])
                self.assertEqual(code, 1)
                self.assertIn(expected, out)
        finally:
            harness.GATES.clear()
            harness.GATES.update(saved)


if __name__ == "__main__":
    unittest.main()
