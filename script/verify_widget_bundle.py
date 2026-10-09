#!/usr/bin/env python3
"""Verify an existing candidate without launching the GUI or hardware paths."""
import pathlib
import plistlib
import subprocess
import sys

app = pathlib.Path(sys.argv[1])
extension = app / 'Contents/PlugIns/FanControlWidget.appex'
bridge = app / 'Contents/Frameworks/FanControlWidgetBridge.dylib'
helper = app / 'Contents/Library/LaunchServices/com.local.fan-control.helper'
group = 'JFC5CWT3V6.com.local.fan-control.readings'
def info(bundle):
    return plistlib.loads((bundle / 'Contents/Info.plist').read_bytes())
def entitlements(target):
    data = subprocess.check_output(['codesign', '-d', '--entitlements', ':-', str(target)], stderr=subprocess.DEVNULL)
    return plistlib.loads(data) if data.strip() else {}

main = info(app)
widget = info(extension)
assert widget['CFBundleIdentifier'] == main['CFBundleIdentifier'] + '.widget'
for key in ('CFBundleVersion', 'CFBundleShortVersionString', 'LSMinimumSystemVersion'):
    assert widget[key] == main[key], key
assert widget['NSExtension']['NSExtensionPointIdentifier'] == 'com.apple.widgetkit-extension'
assert main['LSMinimumSystemVersion'] == '14.0'
assert main['CFBundleURLTypes'][0]['CFBundleURLSchemes'] == ['fancontrol']
assert entitlements(app) == {'com.apple.security.application-groups': [group]}
assert entitlements(extension) == {'com.apple.security.application-groups': [group], 'com.apple.security.app-sandbox': True}
assert not entitlements(helper)
for target in (app, extension, bridge, helper):
    subprocess.run(['codesign', '--verify', '--strict', str(target)], check=True)
for executable in (extension / 'Contents/MacOS/FanControlWidget', helper, bridge):
    assert subprocess.check_output(['lipo', '-archs', str(executable)], text=True).strip() == 'arm64'
dependencies = subprocess.check_output(['otool', '-L', str(helper)], text=True)
assert all(marker not in dependencies for marker in ('Sparkle.framework', '@rpath/', '@executable_path/', '@loader_path/'))
print('Widget bundle/version/architecture/signature/entitlements and independent helper verified')
