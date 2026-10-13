#!/usr/bin/env python3
"""Package native builds and a matching signed-updater archive. No third-party Python modules."""
import argparse, hashlib, json, os, pathlib, shutil, subprocess, tarfile, zipfile
ROOT = pathlib.Path(__file__).resolve().parents[1]
DIST = ROOT / 'dist'
p = argparse.ArgumentParser()
p.add_argument('--platform', choices=['macos-universal', 'windows-x64', 'windows-arm64', 'linux-x86_64', 'linux-aarch64'], required=True)
p.add_argument('--version', default='0.1.4')
p.add_argument('--bin-dir', type=pathlib.Path, required=True)
a = p.parse_args()
DIST.mkdir(exist_ok=True)
stage = DIST / ('stage-' + a.platform)
if stage.exists(): shutil.rmtree(stage)
stage.mkdir()
windows = a.platform.startswith('windows')
mac = a.platform.startswith('macos')
app = stage / ('ToolboxCraft.app' if mac else 'ToolboxCraft')
binaries = app / 'Contents/MacOS' if mac else app
binaries.mkdir(parents=True)
for name in ['toolboxcraft', 'toolboxcraft-updater']:
    executable = name + ('.exe' if windows else '')
    shutil.copy2(a.bin_dir / executable, binaries / executable)
if windows:
    subprocess.run([os.sys.executable, str(ROOT / 'packaging/verify_windows_icons.py'), str(ROOT / 'assets/icons/toolboxcraft.ico'), *[str(binaries / name) for name in ['toolboxcraft.exe', 'toolboxcraft-updater.exe']]], check=True)
    shutil.copy2(ROOT / 'assets/icons/toolboxcraft.ico', app / 'toolboxcraft.ico')
if mac:
    for executable in binaries.iterdir():
        architectures = subprocess.check_output(['lipo', '-archs', str(executable)], text=True).split()
        if not {'arm64', 'x86_64'}.issubset(set(architectures)):
            raise RuntimeError('Universal macOS packages require both arm64 and x86_64 binaries')
    import plistlib
    resources = app / 'Contents/Resources'
    resources.mkdir()
    shutil.copy2(ROOT / 'LICENSE', resources)
    shutil.copy2(ROOT / 'ATTRIBUTION.md', resources)
    shutil.copytree(ROOT / 'assets/licenses', resources / 'licenses')
    for notice in (ROOT / 'assets/fonts').glob('*LICENSE.txt'): shutil.copy2(notice, resources / 'licenses')
    # Build an icns containing standard icon sizes using Apple's own tools.
    iconset = stage / 'ToolboxCraft.iconset'
    iconset.mkdir()
    for size in [16, 32, 128, 256, 512]:
        for suffix, multiplier in [('', 1), ('@2x', 2)]:
            subprocess.run(['sips', '-z', str(size * multiplier), str(size * multiplier), str(ROOT / 'assets/icons/toolboxcraft.png'), '--out', str(iconset / f'icon_{size}x{size}{suffix}.png')], check=True, stdout=subprocess.DEVNULL)
    subprocess.run(['iconutil', '-c', 'icns', str(iconset), '-o', str(resources / 'ToolboxCraft.icns')], check=True)
    with (app / 'Contents/Info.plist').open('wb') as f:
        plistlib.dump(dict(CFBundleIdentifier='dev.toolboxcraft.launcher', CFBundleName='ToolboxCraft', CFBundleExecutable='toolboxcraft', CFBundleIconFile='ToolboxCraft.icns', CFBundlePackageType='APPL', CFBundleShortVersionString=a.version, CFBundleVersion=a.version, LSMinimumSystemVersion='11.0', NSHighResolutionCapable=True), f)
    identity = os.environ.get('MACOS_SIGN_IDENTITY')
    if identity:
        for executable in binaries.iterdir(): subprocess.run(['codesign', '--force', '--options', 'runtime', '--timestamp', '--sign', identity, str(executable)], check=True)
        subprocess.run(['codesign', '--force', '--options', 'runtime', '--timestamp', '--sign', identity, str(app)], check=True)
    else:
        subprocess.run(['codesign', '--force', '--deep', '--sign', '-', str(app)], check=True)
    profile = os.environ.get('MACOS_NOTARY_PROFILE')
    if profile:
        notarize = stage / 'notarize.zip'
        subprocess.run(['ditto', '-c', '-k', '--keepParent', str(app), str(notarize)], check=True)
        subprocess.run(['xcrun', 'notarytool', 'submit', str(notarize), '--keychain-profile', profile, '--wait'], check=True)
        subprocess.run(['xcrun', 'stapler', 'staple', str(app)], check=True)
    dmgroot = stage / 'dmg'
    dmgroot.mkdir()
    shutil.copytree(app, dmgroot / app.name)
    (dmgroot / 'Applications').symlink_to('/Applications')
    subprocess.run(['hdiutil', 'create', '-volname', 'ToolboxCraft', '-srcfolder', str(dmgroot), '-ov', '-format', 'UDZO', str(DIST / f'toolboxcraft-{a.version}-{a.platform}.dmg')], check=True)
else:
    shutil.copy2(ROOT / 'README.md', app)
    shutil.copy2(ROOT / 'LICENSE', app)
    shutil.copy2(ROOT / 'ATTRIBUTION.md', app)
    shutil.copytree(ROOT / 'assets/licenses', app / 'licenses')
    for notice in (ROOT / 'assets/fonts').glob('*LICENSE.txt'): shutil.copy2(notice, app / 'licenses')
    if windows and os.environ.get('WINDOWS_SIGN_PFX'):
        for executable in binaries.glob('*.exe'):
            result = subprocess.run(['signtool', 'sign', '/f', os.environ['WINDOWS_SIGN_PFX'], '/p', os.environ.get('WINDOWS_SIGN_PASSWORD', ''), '/fd', 'SHA256', '/tr', 'http://timestamp.digicert.com', '/td', 'SHA256', str(executable)])
            if result.returncode: raise RuntimeError('Windows executable signing failed')
    if not windows:
        tarpath = DIST / f'toolboxcraft-{a.version}-{a.platform}.tar.gz'
        with tarfile.open(tarpath, 'w:gz') as tf: tf.add(app, arcname='ToolboxCraft')
        # AppImage contains both executables, so the updater helper is available at runtime.
        appdir = stage / 'AppDir'
        (appdir / 'usr/bin').mkdir(parents=True)
        for name in ['toolboxcraft', 'toolboxcraft-updater']:
            shutil.copy2(binaries / name, appdir / 'usr/bin' / name)
        share = appdir / 'usr/share/toolboxcraft'
        share.mkdir(parents=True)
        shutil.copytree(app / 'licenses', share / 'licenses')
        for name in ['LICENSE', 'ATTRIBUTION.md', 'README.md']: shutil.copy2(app / name, share / name)
        shutil.copy2(ROOT / 'assets/icons/toolboxcraft.png', appdir / 'toolboxcraft.png')
        (appdir / 'toolboxcraft.desktop').write_text('[Desktop Entry]\nName=ToolboxCraft\nExec=toolboxcraft\nIcon=toolboxcraft\nType=Application\nCategories=Utility;\n')
        run = appdir / 'AppRun'
        run.write_text('#!/bin/sh\nSELF=$(dirname "$(readlink -f "$0")")\nexec "$SELF/usr/bin/toolboxcraft" "$@"\n')
        run.chmod(0o755)
        appimage = DIST / f'toolboxcraft-{a.version}-{a.platform}.AppImage'
        subprocess.run(['appimagetool', str(appdir), str(appimage)], check=True, env={**os.environ, 'ARCH': 'aarch64' if a.platform.endswith('aarch64') else 'x86_64', 'APPIMAGE_EXTRACT_AND_RUN': '1'})
        shutil.copy2(appimage, stage / 'ToolboxCraft.AppImage')
# The updater uses a ZIP for every target. macOS keeps the complete signed bundle.
update = DIST / f'toolboxcraft-{a.version}-{a.platform}-update.zip'
with zipfile.ZipFile(update, 'w', zipfile.ZIP_DEFLATED) as z:
    for file in app.rglob('*'):
        if file.is_file(): z.write(file, file.relative_to(stage))
    if not windows and not mac: z.write(stage / 'ToolboxCraft.AppImage', 'ToolboxCraft.AppImage')
if windows:
    shutil.copy2(update, DIST / f'toolboxcraft-{a.version}-{a.platform}-portable.zip')
    subprocess.run(['makensis', f'/DVERSION={a.version}', f'/DARCH={a.platform}', f'/DSOURCE={app}', f'/DOUT={DIST}', str(ROOT / 'packaging/windows.nsi')], check=True)
    if os.environ.get('WINDOWS_SIGN_PFX'):
        installer = DIST / f'toolboxcraft-{a.version}-{a.platform}-setup.exe'
        result = subprocess.run(['signtool', 'sign', '/f', os.environ['WINDOWS_SIGN_PFX'], '/p', os.environ.get('WINDOWS_SIGN_PASSWORD', ''), '/fd', 'SHA256', '/tr', 'http://timestamp.digicert.com', '/td', 'SHA256', str(installer)])
        if result.returncode: raise RuntimeError('Windows installer signing failed')
package = dict(file=update.name, sha256=hashlib.sha256(update.read_bytes()).hexdigest(), size=update.stat().st_size)
(DIST / f'metadata-{a.platform}.json').write_text(json.dumps(dict(version=a.version, target=a.platform, package=package)))
shutil.rmtree(stage)
