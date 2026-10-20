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

if __name__=="__main__":unittest.main()
