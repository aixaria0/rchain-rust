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
if __name__=="__main__":unittest.main()
