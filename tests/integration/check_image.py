"""Check private state initialization in an image and a fresh named volume.

Uses a synthetic key, no prompts, and no network. No operator credentials,
existing containers or existing volumes are used.
"""
import argparse
import subprocess
import uuid


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--image', default='openwebui-acp-sidecar:local')
    image = parser.parse_args().image
    prefix = 'openwebui-acp-image-check-' + uuid.uuid4().hex
    volume = prefix + '-state'
    subprocess.run(['docker', 'volume', 'create', volume], check=True, capture_output=True)
    try:
        for label, mount in [('image', []), ('volume', ['--volume', volume + ':/state'])]:
            container = prefix + '-' + label
            command = ['docker', 'run', '--rm', '--name', container, '--network', 'none',
                       '--env', 'OPENWEBUI_API_KEY=synthetic-image-check',
                       '--entrypoint', '/usr/local/bin/openwebui-acp', *mount, image,
                       '--url', 'https://fixture.invalid', '--model', 'fixture',
                       '--state-dir', '/state', '--buzz-auto-publish', 'false']
            try:
                # EOF exits after Harness::new has validated and initialized the
                # private journal. No ACP prompt or remote request is submitted.
                result = subprocess.run(command, input='', text=True, capture_output=True, timeout=30)
                assert result.returncode == 0, label + ' state startup failed: ' + result.stderr[-2000:]
                assert not result.stdout, 'Unexpected non-protocol stdout during image startup'
            finally:
                subprocess.run(['docker', 'rm', '-f', container], capture_output=True)
    finally:
        subprocess.run(['docker', 'volume', 'rm', volume], check=True, capture_output=True)
    print('Private state startup: image directory and fresh named volume: PASS')


if __name__ == '__main__':
    main()
