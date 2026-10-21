#!/usr/bin/env python3
"""The alternate workload is explicit, isolated and never the canonical default."""
import importlib.util
from pathlib import Path
from tempfile import TemporaryDirectory
from types import SimpleNamespace
from unittest.mock import patch
import unittest


PATH = Path(__file__).resolve().parent / "consensus-partition-rejoin.py"
SPEC = importlib.util.spec_from_file_location("partition_diagnostic", PATH)
m = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(m)


class WorkloadProfileTests(unittest.TestCase):
    def test_default_still_uses_the_original_driver(self):
        with TemporaryDirectory() as output:
            d = m.WorkloadDrill(SimpleNamespace(validators=8, out=output))
            self.assertFalse(d.rotate_partition_work)
            with patch.object(m.a.Drill, "drive") as canonical:
                d.drive()
                canonical.assert_called_once_with()

    def test_rotation_visits_only_the_seven_connected_peers(self):
        with TemporaryDirectory() as output:
            d = m.WorkloadDrill(SimpleNamespace(validators=8, out=output))
            d.rotate_partition_work = True
            clock = [0]
            with patch.object(m.time, "monotonic", side_effect=lambda: clock[0]), \
                 patch.object(d, "deploy") as deploy:
                for i in range(15):
                    clock[0] = 6 * i
                    d.drive()
                self.assertEqual([c.kwargs["target_node"] for c in deploy.call_args_list],
                                 list(range(7)) * 2 + [0])
                self.assertTrue(all(c.args == ("Nil",) for c in deploy.call_args_list))

    def test_deploy_target_refuses_out_of_campaign_index_before_side_effects(self):
        with TemporaryDirectory() as output:
            d = m.WorkloadDrill(SimpleNamespace(validators=4, out=output))
            with patch.object(m.a, "get") as get:
                with self.assertRaises(ValueError):
                    d.deploy("Nil", target_node=4)
                get.assert_not_called()


if __name__ == "__main__":
    unittest.main()
