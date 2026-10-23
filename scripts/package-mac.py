import pathlib,subprocess,shutil,plistlib,os,tempfile
root=pathlib.Path(__file__).resolve().parents[1]; app=pathlib.Path(tempfile.mkdtemp(prefix='pulse-helper-package-'))/'Pulse Helper.app'; mac=app/'Contents'/'MacOS';
if app.exists():shutil.rmtree(app)
mac.mkdir(parents=True,exist_ok=True)
shutil.copyfile(root/'native/target/release/pulse-media',mac/'pulse-media');(mac/'pulse-media').chmod(0o755)
plugins=mac/'plugins';plugins.mkdir(exist_ok=True)
license_roots=set()
def record_license_root(file):
    for parent in pathlib.Path(file).resolve().parents:
        if (parent/'INSTALL_RECEIPT.json').exists():
            license_roots.add(parent)
            break
prefix=pathlib.Path(subprocess.check_output(['brew','--prefix','gstreamer'],text=True).strip())
names='coreelements typefindfunctions app applemedia audioconvert audioresample opus rtp rtpmanager webrtc nice dtls srtp sctp videoparsersbad rsrtp'.split()
for name in names:
    dest=plugins/f'libgst{name}.dylib';shutil.copyfile((pathlib.Path('/opt/homebrew/opt/libnice-gstreamer/libexec/gstreamer-1.0') if name=='nice' else prefix/'lib/gstreamer-1.0')/dest.name,dest);dest.chmod(0o755)
record_license_root(prefix/'lib/libgstreamer-1.0.dylib')
record_license_root('/opt/homebrew/opt/libnice-gstreamer/libexec/gstreamer-1.0/libgstnice.dylib')
queue=[mac/'pulse-media',*plugins.glob('*.dylib')]; seen=set()
while queue:
    file=queue.pop()
    if file in seen:continue
    seen.add(file)
    deps=[l.strip().split(' (')[0] for l in subprocess.check_output(['otool','-L',str(file)],text=True).splitlines()[1:]]
    for dep in deps:
        if not dep.startswith('/opt/homebrew/'):continue
        record_license_root(dep)
        dest=mac/pathlib.Path(dep).name
        if not dest.exists():shutil.copyfile(dep,dest);dest.chmod(0o755);queue.append(dest)
        relative='@loader_path/'+('../' if file.parent==plugins else '')+dest.name
        subprocess.run(['install_name_tool','-change',dep,relative,str(file)],check=True,stderr=subprocess.DEVNULL)
    if file.suffix=='.dylib':subprocess.run(['install_name_tool','-id','@loader_path/'+file.name,str(file)],check=True,stderr=subprocess.DEVNULL)
licenses=app/'Contents/Resources/licenses'
for formula_root in sorted(license_roots):
    target=licenses/formula_root.parent.name
    target.mkdir(parents=True,exist_ok=True)
    for notice in formula_root.iterdir():
        if notice.is_file() and notice.name.upper().startswith(('LICENSE','COPYING','NOTICE','AUTHORS')):
            shutil.copyfile(notice,target/notice.name)
shutil.copyfile(root/'README.md', app/'Contents/Resources/README.txt')
source=root/'scripts/Launcher.swift'
subprocess.run(['swiftc','-target','arm64-apple-macos27.0',str(source),'-o',str(mac/'PulseHelper')],check=True)
with (app/'Contents/Info.plist').open('wb') as f:plistlib.dump({'CFBundleExecutable':'PulseHelper','CFBundleIdentifier':'com.to-unknown.pulse.capture','CFBundleName':'Pulse Helper','CFBundleVersion':'3','CFBundleShortVersionString':'0.1.2','CFBundlePackageType':'APPL','LSMinimumSystemVersion':'27.0','LSUIElement':True,'NSHighResolutionCapable':True,'NSScreenCaptureUsageDescription':'Share your screen and system audio in your Pulse call.','CFBundleURLTypes':[{'CFBundleURLName':'Pulse capture','CFBundleURLSchemes':['pulse-capture']}]},f)
# Reject incompatible executables/libraries before signing or publishing.
import re
for file in [*seen,mac/'PulseHelper']:
    metadata=subprocess.check_output(['xcrun','vtool','-show-build',str(file)],text=True)
    minimum=re.search(r'minos (\d+(?:\.\d+)*)',metadata)
    if minimum and tuple(map(int,minimum[1].split('.'))) > (27,0):
        raise RuntimeError(f'{file.name} requires macOS {minimum[1]}, above macOS 27')
subprocess.run(['xattr','-rd','com.apple.FinderInfo',str(app)],stderr=subprocess.DEVNULL)
subprocess.run(['xattr','-rd','com.apple.ResourceFork',str(app)],stderr=subprocess.DEVNULL)
identity=os.environ.get('PULSE_SIGN_IDENTITY','-')
for file in sorted(seen):subprocess.run(['codesign','--force','--sign',identity,str(file)],check=True,stderr=subprocess.DEVNULL)
subprocess.run(['codesign','--force','--sign',identity,str(app)],check=True)
(root/'release').mkdir(parents=True,exist_ok=True)
subprocess.run(['ditto','-c','-k','--sequesterRsrc','--keepParent',str(app),str(root/'release/Pulse-Helper-macOS-arm64.zip')],check=True)
print('Packaged',app)
