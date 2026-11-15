import glob
import hashlib
import json
import os
import shutil
import tarfile

with open('scripts/ci-native-dependencies.json', encoding='utf-8') as source:
    spec = json.load(source)['sherpaLinux']
archive = glob.glob(os.path.join(os.environ['SHERPA_ONNX_ARCHIVE_DIR'], '*.tar.bz2'))
if len(archive) != 1:
    raise SystemExit('Expected one verified Sherpa archive')
with tarfile.open(archive[0], 'r:bz2') as source:
    source.extractall('/tmp/fono-runtime', filter='data')
for path in glob.glob('/tmp/fono-runtime/' + spec['directory'] + '/lib/*.so*'):
    # Materialize SONAME links; the final image is self-contained.
    shutil.copyfile(path, '/out/' + os.path.basename(path))
vad = 'src-tauri/resources/vad/silero_vad.onnx'
with open(vad, 'rb') as source:
    if hashlib.sha256(source.read()).hexdigest() != '9e2449e1087496d8d4caba907f23e0bd3f78d91fa552479bb9c23ac09cbb1fd6':
        raise SystemExit('Silero VAD SHA-256 mismatch')
shutil.copyfile(vad, '/out/silero_vad.onnx')
shutil.copyfile('LICENSE', '/out/LICENSE')
shutil.copyfile('licenses/offline-stt-NOTICE.md', '/out/offline-stt-NOTICE.md')
shutil.copyfile('licenses/server-doctor-NOTICE.md', '/out/server-doctor-NOTICE.md')
for path in glob.glob('apps/fono-server/resources/doctor-*-LICENSE.txt'):
    shutil.copyfile(path, '/out/' + os.path.basename(path))
shutil.copyfile('licenses/dependency-notices.txt', '/out/dependency-notices.txt')
for name in ('sherpa-onnx-1.13.4-APACHE-2.0.txt', 'onnxruntime-1.27.0-MIT.txt',
             'onnxruntime-1.27.0-ThirdPartyNotices.txt', 'silero-vad-MIT.txt'):
    shutil.copyfile('licenses/upstream/' + name, '/out/' + name)
