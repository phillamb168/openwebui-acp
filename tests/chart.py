"""Helm contract checks; no Kubernetes cluster, credentials or network required."""
import copy
import json
import os
from pathlib import Path
import subprocess
import tempfile
import tomllib
import unittest

ROOT = Path(__file__).resolve().parents[1]
BASE = {'openwebui': {'url': 'https://webui.example.org', 'model': 'example'},
        'buzz': {'relayUrl': 'wss://relay.example.org', 'allowedAuthors': ['1' * 64],
                 'allowedChannels': ['11111111-1111-4111-8111-111111111111']},
        'credentials': {'existingSecret': 'existing-credentials'}}


def render(values):
    with tempfile.TemporaryDirectory() as directory:
        file = Path(directory) / 'values.json'
        file.write_text(json.dumps(values))
        return subprocess.run([os.environ.get('HELM', 'helm'), 'template', 'example',
                               str(ROOT / 'deploy/chart'), '-f', str(file)],
                              capture_output=True, text=True, timeout=20)


class ChartTests(unittest.TestCase):
    def test_minimal_install_is_one_private_worker_and_retained_storage(self):
        result = render(BASE)
        self.assertEqual(result.returncode, 0, result.stderr)
        version = tomllib.loads((ROOT / 'Cargo.toml').read_text())['package']['version']
        chart = (ROOT / 'deploy/chart/Chart.yaml').read_text()
        self.assertIn('version: ' + version + '\n', chart)
        self.assertIn('appVersion: "' + version + '"', chart)
        self.assertIn('image: "openwebui-acp-sidecar:' + version + '"', result.stdout)
        for contract in ['replicas: 1', 'type: Recreate', 'automountServiceAccountToken: false',
                         'runAsUser: 10001', 'fsGroupChangePolicy: OnRootMismatch',
                         'command: ["/usr/local/bin/sidecar-launcher", "prepare"]',
                         'subPath: keys', 'subPath: state', 'helm.sh/resource-policy: keep']:
            self.assertIn(contract, result.stdout)
        self.assertEqual(result.stdout.count('readOnlyRootFilesystem: true'), 2)
        self.assertEqual(result.stdout.count('allowPrivilegeEscalation: false'), 2)
        for forbidden in ['kind: Service\n', 'kind: Ingress', 'kind: Secret', 'kind: Role', 'hostPath:']:
            self.assertNotIn(forbidden, result.stdout)

    def test_existing_claim_is_reused_without_a_new_claim(self):
        values = copy.deepcopy(BASE)
        values['persistence'] = {'existingClaim': 'saved-state'}
        result = render(values)
        self.assertEqual(result.returncode, 0)
        self.assertIn('claimName: "saved-state"', result.stdout)
        self.assertNotIn('kind: PersistentVolumeClaim', result.stdout)

    def test_digest_overrides_mutable_tag(self):
        values = copy.deepcopy(BASE)
        values['image'] = {'repository': 'registry.example.org/agent', 'digest': 'sha256:' + '2' * 64}
        result = render(values)
        self.assertEqual(result.returncode, 0)
        self.assertEqual(result.stdout.count('image: "registry.example.org/agent@sha256:' + '2' * 64 + '"'), 2)

    def test_missing_permissions_or_secret_are_rejected(self):
        for section, field, bad in [('buzz', 'allowedAuthors', []), ('buzz', 'allowedChannels', []),
                                    ('credentials', 'existingSecret', ''), ('openwebui', 'model', '')]:
            with self.subTest(field=field):
                values = copy.deepcopy(BASE)
                values[section][field] = bad
                self.assertNotEqual(render(values).returncode, 0)

    def test_bad_identities_and_unreviewed_settings_are_rejected(self):
        for section, field, value in [('buzz', 'allowedAuthors', ['*']), ('buzz', 'allowedChannels', ['general']),
                                      ('openwebui', 'timeoutSeconds', 0), ('credentials', 'apiKey', 'not-accepted')]:
            with self.subTest(field=field):
                values = copy.deepcopy(BASE)
                values[section][field] = value
                self.assertNotEqual(render(values).returncode, 0)
        values = dict(BASE, replicaCount=2)
        self.assertNotEqual(render(values).returncode, 0)

    def test_plaintext_transport_requires_explicit_opt_in(self):
        for section, field, url in [('openwebui', 'url', 'http://webui:8080'), ('buzz', 'relayUrl', 'ws://relay:3000')]:
            with self.subTest(field=field):
                values = copy.deepcopy(BASE)
                values[section][field] = url
                self.assertNotEqual(render(values).returncode, 0)
                values['openwebui']['allowInsecureHttp'] = True
                self.assertEqual(render(values).returncode, 0)


if __name__ == '__main__':
    unittest.main()
