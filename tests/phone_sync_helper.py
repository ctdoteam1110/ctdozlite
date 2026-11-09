"""Offline helper contract: fake zproto, fake keys, no user files or network."""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
NODE = shutil.which('node')


@unittest.skipUnless(NODE, 'Node.js is required for the phone helper contract')
class PhoneSyncHelperContract(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='ctdo-helper-test-')
        self.addCleanup(self.temp.cleanup)
        self.directory = Path(self.temp.name)
        shutil.copy(ROOT / 'sync_v2_helper.js', self.directory)
        (self.directory / 'fake.wasm').write_bytes(b'fixture')
        (self.directory / 'zproto_lib.js').write_text('''
exports.b = async () => {};
exports.e = () => {
  const fs = require('fs');
  const count = (fs.existsSync('mock-counter') ? Number(fs.readFileSync('mock-counter', 'utf8')) : 0) + 1;
  fs.writeFileSync('mock-counter', String(count));
  return {publicKey: Buffer.concat([Buffer.from([5]), Buffer.alloc(32, count)]), privateKey: Buffer.alloc(32, count), free() {}};
};
exports.j = (data) => {
  if (data.toString() !== 'metadata') throw Error('wrong metadata framing');
  return {sessionRecord: Buffer.from('record'), plaintext: Buffer.from('first'), free() {}};
};
exports.i = (data, session) => {
  if (data.toString() !== 'message' || session.toString() !== 'record') throw Error('wrong session');
  return {plaintext: Buffer.from('next'), free() {}};
};
''')

    def run_helper(self, command, data=None):
        import os
        arguments = [NODE, 'sync_v2_helper.js', command]
        if data is not None:
            (self.directory / 'input.enc').write_bytes(data)
            arguments.append('input.enc')
        result = subprocess.run(arguments, cwd=self.directory, capture_output=True, check=True,
                                env={**os.environ, 'ZPROTO_WASM_PATH': str(self.directory / 'fake.wasm')},
                                timeout=10)
        return json.loads(result.stdout)

    def test_embedded_metadata_keeps_first_payload_and_retains_session_for_next_batch(self):
        public = self.run_helper('gen-keys')
        self.assertEqual(set(public), {'ik_pub', 'ek_pub'})
        keys_file = self.directory / 'sync_v2_keys.json'
        self.assertIn('ik_priv', json.loads(keys_file.read_text()))
        metadata = self.run_helper('decrypt-meta', b'metadata')
        self.assertEqual(metadata, {'plainLen': 5})
        self.assertEqual((self.directory / 'sync_v2_message.bin').read_bytes(), b'first')
        self.assertEqual((self.directory / 'sync_v2_session.bin').read_bytes(), b'record')
        self.run_helper('decrypt-message', b'message')
        self.assertEqual((self.directory / 'sync_v2_message.bin').read_bytes(), b'next')
        self.assertTrue(keys_file.exists())
        self.assertTrue((self.directory / 'sync_v2_session.bin').exists())
        for filename in ['sync_v2_keys.json', 'sync_v2_session.bin', 'sync_v2_message.bin']:
            self.assertEqual((self.directory / filename).stat().st_mode & 0o777, 0o600)

    def test_next_request_preserves_identity_and_rotates_ephemeral_key(self):
        self.run_helper('gen-keys')
        key_file = self.directory / 'sync_v2_keys.json'
        first = json.loads(key_file.read_text())
        public = self.run_helper('gen-keys-reuse-identity')
        second = json.loads(key_file.read_text())
        self.assertEqual(set(public), {'ik_pub', 'ek_pub'})
        self.assertEqual(first['ik_pub'], second['ik_pub'])
        self.assertEqual(first['ik_priv'], second['ik_priv'])
        self.assertNotEqual(first['ek_pub'], second['ek_pub'])
        self.assertNotEqual(first['ek_priv'], second['ek_priv'])
        self.run_helper('gen-keys-reuse-identity')
        third = json.loads(key_file.read_text())
        self.assertEqual(second['ik_pub'], third['ik_pub'])
        self.assertNotEqual(second['ek_pub'], third['ek_pub'])
        self.assertEqual(key_file.stat().st_mode & 0o777, 0o600)

    def test_reuse_does_not_silently_create_or_replace_missing_identity(self):
        with self.assertRaises(subprocess.CalledProcessError):
            self.run_helper('gen-keys-reuse-identity')
        self.assertFalse((self.directory / 'sync_v2_keys.json').exists())

    def test_reuse_rejects_corrupt_identity_before_new_key_is_written(self):
        self.run_helper('gen-keys')
        key_file = self.directory / 'sync_v2_keys.json'
        data = json.loads(key_file.read_text())
        data['ik_pub'] = 'invalid'
        key_file.write_text(json.dumps(data))
        with self.assertRaises(subprocess.CalledProcessError):
            self.run_helper('gen-keys-reuse-identity')
        self.assertEqual(json.loads(key_file.read_text())['ik_pub'], 'invalid')


if __name__ == '__main__':
    unittest.main()
