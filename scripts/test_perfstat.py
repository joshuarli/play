import json
import subprocess
import sys
import tempfile
import time
import unittest
import unittest.mock
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import perfstat  # noqa: E402


class ProcessCpuTests(unittest.TestCase):
    def test_busy_child_reports_cpu(self):
        child = subprocess.Popen(
            [sys.executable, "-I", "-c", "while True: pass"],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        )
        try:
            time.sleep(0.1)
            used = perfstat.process_cpu_seconds(child.pid)
        finally:
            child.kill()
            child.wait()
        self.assertGreater(used, 0.05)

    def test_sleeping_child_reports_nothing(self):
        child = subprocess.Popen(["sleep", "5"])
        try:
            time.sleep(0.1)
            used = perfstat.process_cpu_seconds(child.pid)
        finally:
            child.kill()
            child.wait()
        self.assertLess(used, 0.02)

    def test_sampling_resolves_below_ten_milliseconds(self):
        child = subprocess.Popen(
            [sys.executable, "-I", "-c",
             "import time\nt=time.monotonic()\nwhile time.monotonic()-t<1.0: pass"],
        )
        try:
            time.sleep(0.2)
            first = perfstat.process_cpu_seconds(child.pid)
            time.sleep(0.12)
            second = perfstat.process_cpu_seconds(child.pid)
        finally:
            child.kill()
            child.wait()
        self.assertGreater(second - first, 0.05)
        self.assertLess(second - first, 0.3)


class MetricFloorTests(unittest.TestCase):
    def test_floors_hold_a_noisy_candidate_to_unchanged(self):
        baseline = [1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0]
        candidate = [1.2, 1.1, 1.3, 1.0, 1.2, 1.1, 1.2]
        outcome = perfstat.compare_metric("cpu_percent", baseline, candidate)
        self.assertEqual(outcome["verdict"], "unchanged")
        self.assertAlmostEqual(outcome["tolerance"], 0.5)

    def test_floors_apply_to_memory_and_latency(self):
        self.assertAlmostEqual(perfstat.metric_floor("rss_peak_mb"), 8.0)
        self.assertAlmostEqual(perfstat.metric_floor("rss_steady_mb"), 8.0)
        self.assertAlmostEqual(perfstat.metric_floor("latency_ms"), 5.0)
        self.assertAlmostEqual(perfstat.metric_floor("error_s"), 0.01)
        self.assertAlmostEqual(perfstat.metric_floor("cpu_per_media_second"), 0.01)


class HostStateTests(unittest.TestCase):
    def test_gate_reaches_sysctl_when_the_parent_path_cannot(self):
        if sys.platform != "darwin":
            self.skipTest("sysctl lives in /usr/sbin only on macOS")
        tool = perfstat.shutil.which("perf-gate-ready")
        if tool is None:
            self.skipTest("perf-gate-ready is not installed")
        bare = f"{Path(tool).parent}:/usr/bin:/bin"
        direct = subprocess.run([tool], capture_output=True, env={"PATH": bare})
        self.assertNotEqual(direct.returncode, 0)
        if subprocess.run([tool], capture_output=True,
                          env={"PATH": f"{bare}:/usr/sbin:/sbin"}).returncode != 0:
            self.skipTest("host is not quiet")
        with unittest.mock.patch.dict(perfstat.os.environ, {"PATH": bare}):
            self.assertEqual(perfstat.host_state(), perfstat.HOST_QUIET)


class CpuSecondsTests(unittest.TestCase):
    def test_zero(self):
        self.assertEqual(perfstat.cpu_seconds("00:00:00"), 0.0)

    def test_minutes_seconds(self):
        self.assertAlmostEqual(perfstat.cpu_seconds("0:01.50"), 1.5)
        self.assertAlmostEqual(perfstat.cpu_seconds("12:34.56"), 754.56)

    def test_hours_minutes_seconds(self):
        self.assertAlmostEqual(perfstat.cpu_seconds("01:02:03"), 3723.0)

    def test_days(self):
        self.assertAlmostEqual(perfstat.cpu_seconds("2-01:02:03"), 2 * 86400 + 3723.0)

    def test_large_minutes(self):
        self.assertAlmostEqual(perfstat.cpu_seconds("123:45.67"), 123 * 60 + 45.67)


class ComparePairedTests(unittest.TestCase):
    def test_wide_candidate_is_inconclusive(self):
        baseline = [99, 100, 101, 99, 100, 101, 100]
        candidate = [95, 135, 115, 95, 135, 115, 115]
        outcome = perfstat.compare_paired(baseline, candidate)
        self.assertEqual(outcome["verdict"], "inconclusive")
        self.assertAlmostEqual(outcome["tolerance"], 5.0)
        self.assertEqual(outcome["lo"], -4)
        self.assertEqual(outcome["hi"], 35)

    def test_uniform_slowdown_is_regressed(self):
        baseline = [0.180, 0.181, 0.179, 0.180, 0.182, 0.178, 0.180]
        candidate = [value * 1.15 for value in baseline]
        outcome = perfstat.compare_paired(baseline, candidate)
        self.assertEqual(outcome["verdict"], "regressed")
        self.assertGreater(outcome["lo"], outcome["tolerance"])

    def test_failed_trial_is_invalid(self):
        baseline = [100, None, 100, 100, 100, 100, 100]
        candidate = [100, 100, 100, 100, 100, 100, 100]
        self.assertEqual(perfstat.compare_paired(baseline, candidate)["verdict"], "invalid")
        self.assertEqual(perfstat.compare_paired(candidate, baseline)["verdict"], "invalid")

    def test_identical_samples_are_unchanged(self):
        samples = [10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0]
        outcome = perfstat.compare_paired(samples, list(samples))
        self.assertEqual(outcome["verdict"], "unchanged")
        self.assertEqual(outcome["lo"], 0)
        self.assertEqual(outcome["hi"], 0)

    def test_few_pairs_use_extremes(self):
        baseline = [100, 100, 100]
        candidate = [100, 120, 100]
        outcome = perfstat.compare_paired(baseline, candidate)
        self.assertEqual(outcome["verdict"], "inconclusive")

    def test_higher_is_better(self):
        baseline = [10, 10, 10, 10, 10, 10, 10]
        candidate = [12, 12, 12, 12, 12, 12, 12]
        outcome = perfstat.compare_paired(
            baseline, candidate, rel_tolerance=0.0, lower_is_better=False
        )
        self.assertEqual(outcome["verdict"], "improved")


class ResultWritingTests(unittest.TestCase):
    def payload(self):
        return {
            "players": {
                "candidate": {
                    "path": "build/mpv",
                    "profile": "dev",
                    "version": "player mpv-compatible 0.42.2",
                    "arguments": ["build/mpv", "--mute=yes"],
                }
            },
            "samples": {"latency_ms": [10.0, 11.0]},
            "trials": [{"outcome": "ok"}, {"outcome": "failed", "reason": "timeout"}],
        }

    def test_writes_once_and_never_overwrites(self):
        with tempfile.TemporaryDirectory() as directory:
            first = perfstat.write_result("seek", self.payload(), root=directory)
            second = perfstat.write_result("seek", self.payload(), root=directory)
            self.assertNotEqual(first, second)
            self.assertTrue(first.exists())
            self.assertTrue(second.exists())
            written = json.loads(first.read_text())
            self.assertEqual(written["mode"], "seek")
            self.assertEqual(written["players"]["candidate"]["profile"], "dev")

    def test_rejects_incomplete_payload(self):
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaises(ValueError):
                perfstat.write_result("seek", {"samples": {}}, root=directory)


class RangeStatsTests(unittest.TestCase):
    def test_stat_tracks_min_and_max(self):
        result = perfstat.stat([1.0, 2.0, 6.0], 1)
        self.assertEqual(result["median"], 2.0)
        self.assertEqual(result["min"], 1.0)
        self.assertEqual(result["max"], 6.0)
        self.assertEqual(result["spread"], 5.0)

    def test_profile_labels(self):
        self.assertEqual(perfstat.profile_label(perfstat.ROOT / "build" / "mpv"), "dev")
        self.assertEqual(
            perfstat.profile_label(perfstat.ROOT / "dist" / "Player.app" / "Contents"
                                   / "MacOS" / "player"),
            "dist",
        )


if __name__ == "__main__":
    unittest.main()
