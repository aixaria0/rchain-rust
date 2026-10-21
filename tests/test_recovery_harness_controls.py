import importlib.util
import unittest
from pathlib import Path

ROOT=Path(__file__).resolve().parents[1]
spec=importlib.util.spec_from_file_location("partition",ROOT/"tests/consensus-partition-rejoin.py")
m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)

class RecoveryHarnessTests(unittest.TestCase):
    def fixture(self):
        return {"Mounts":[
            {"Type":"volume","Name":"acceptance-validator-3-data","Source":"/vol/data",
             "Destination":"/var/lib/rnode","RW":True},
            {"Type":"bind","Source":"/tmp/genesis","Destination":"/genesis","RW":False}]}
    def test_mount_order_does_not_imply_a_new_state_volume(self):
        before=self.fixture();after=self.fixture();after["Mounts"].reverse()
        self.assertEqual(m.durable_state_volume(before),m.durable_state_volume(after))
    def test_actual_state_volume_replacement_is_detected(self):
        before=self.fixture();after=self.fixture();after["Mounts"][0]["Name"]="other"
        self.assertNotEqual(m.durable_state_volume(before),m.durable_state_volume(after))
    def test_missing_or_ambiguous_state_volume_is_refused(self):
        for mounts in [[],self.fixture()["Mounts"]*2]:
            with self.assertRaises(AssertionError):m.durable_state_volume({"Mounts":mounts})


class ActiveRecoveryWitnessTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        import importlib.util
        spec=importlib.util.spec_from_file_location("active_recovery",ROOT/"tests/lfs-passive-reactivation.py")
        cls.witness=importlib.util.module_from_spec(spec);spec.loader.exec_module(cls.witness)

    def test_successful_post_restore_sequence_is_recorded(self):
        log="\n".join(f"INFO proposed and added block #{q+1} (seq {q})" for q in range(5,12))
        self.assertEqual([r["seq"] for r in self.witness.active_proposal_witness(log)],list(range(5,12)))

    def test_key_enabled_and_passive_finality_cannot_substitute_for_proposal(self):
        with self.assertRaises(AssertionError):
            self.witness.active_proposal_witness("validator key enabled; LFB advanced; no errors")

    def test_sequence_reuse_or_regression_refuses_acceptance(self):
        for q in [4,5]:
            with self.assertRaises(AssertionError):
                self.witness.active_proposal_witness(f"proposed and added block #6 (seq 5)\nproposed and added block #7 (seq {q})")



class PartitionFailureEvidenceTests(unittest.TestCase):
    def test_failure_preserves_cycle_progress_and_primary_error(self):
        import json
        from tempfile import TemporaryDirectory
        from unittest.mock import Mock
        with TemporaryDirectory() as directory:
            out=Path(directory);rounds=[]
            row={"cycle":1,"phase":"healing","partition_progress_observed":True}
            capture=Mock(side_effect=RuntimeError("diagnostics unavailable"))
            primary=RuntimeError("healing timed out")
            with self.assertRaises(RuntimeError) as caught:
                with m.record_cycle(out,rounds,row,capture):
                    raise primary
            self.assertIs(caught.exception,primary)
            result=json.loads((out/"partition-result.json").read_text())
            self.assertEqual(result["failed_phase"],"healing")
            self.assertEqual(result["partition_cycles_completed"],0)
            self.assertTrue(result["rounds"][0]["partition_progress_observed"])
            self.assertEqual(result["rounds"][0]["capture_error"],"diagnostics unavailable")
            capture.assert_called_once_with("partition-1-healing-failed")
            self.assertEqual(json.loads((out/"partition-observations.json").read_text()),rounds)

    def test_successful_cycle_does_not_emit_a_campaign_pass(self):
        from tempfile import TemporaryDirectory
        from unittest.mock import Mock
        with TemporaryDirectory() as directory:
            out=Path(directory);rounds=[];capture=Mock()
            with m.record_cycle(out,rounds,{"cycle":1,"phase":"retention"},capture):
                pass
            self.assertEqual(rounds[0]["result"],"PASS")
            self.assertFalse((out/"partition-result.json").exists())
            capture.assert_not_called()

    def test_later_failure_retains_completed_cycle(self):
        import json
        from tempfile import TemporaryDirectory
        from unittest.mock import Mock
        with TemporaryDirectory() as directory:
            out=Path(directory);rounds=[{"cycle":1,"result":"PASS"}]
            with self.assertRaisesRegex(AssertionError,"online progress"):
                with m.record_cycle(out,rounds,{"cycle":2,"phase":"online-progress-verdict",
                                               "partition_progress_observed":False,
                                               "rejoined":[{"height":59}]},Mock()):
                    raise AssertionError("online progress")
            result=json.loads((out/"partition-result.json").read_text())
            self.assertEqual(result["partition_cycles_completed"],1)
            self.assertEqual(result["failed_cycle"],2)
            self.assertFalse(result["rounds"][1]["partition_progress_observed"])
            self.assertEqual(result["rounds"][1]["rejoined"],[{"height":59}])

    def test_main_captures_healing_timeout_before_cleanup(self):
        import json
        from tempfile import TemporaryDirectory
        from unittest.mock import Mock,patch
        with TemporaryDirectory() as directory:
            out=Path(directory)
            (out/"result.json").write_text(json.dumps({"initial_finalized":[]}))
            run=Mock(out=out,marker="marker",value=42)
            run.wait.side_effect=RuntimeError("healing timed out")
            captures=[]
            def capture(*args):
                captures.append(run.out.name)
            mount={"Mounts":[{"Type":"volume","Name":"state","Source":"/vol/state",
                              "Destination":"/var/lib/rnode","RW":True}]}
            def command(*args):
                return "{}" if "-f" in args else json.dumps([mount])
            with patch.dict(m.os.environ,{"GITHUB_ACTIONS":"true","RUNNER_ENVIRONMENT":"github-hosted",
                                          "DEVNET_PREFIX":"acceptance"}), \
                 patch("sys.argv",["partition","--validators","4","--out",directory]), \
                 patch.object(m,"WorkloadDrill",return_value=run), \
                 patch.object(m,"cmd",side_effect=command), \
                 patch.object(m.a,"get"),patch.object(m.a,"probe"), \
                 patch.object(m.a,"finalized_identity",side_effect=[(10,"a"),(13,"b"),(13,"b"),(13,"b")]), \
                 patch.object(m.a,"marker_present",return_value=True), \
                 patch.object(m.time,"monotonic",side_effect=[0,1,2]), \
                 patch.object(m.a.causal_trace,"capture",side_effect=capture), \
                 patch.object(m.subprocess,"run") as cleanup:
                with self.assertRaisesRegex(RuntimeError,"healing timed out"):
                    m.main()
                cleanup.assert_called_once()
                self.assertEqual(cleanup.call_args.kwargs["timeout"],25)
            self.assertEqual(captures,["before-partition","partition-1-before-healing","partition-1-healing-failed"])
            result=json.loads((out/"partition-result.json").read_text())
            self.assertEqual(result["failed_phase"],"healing")
            self.assertTrue(result["rounds"][0]["partition_progress_observed"])
            run.fixed_snapshots.assert_not_called()


if __name__=="__main__":unittest.main()
