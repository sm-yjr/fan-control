#!/usr/bin/env bash
# Local compilation, hosted credentials. Never print or export signing secrets.
set -euo pipefail
umask 077
export PYTHONDONTWRITEBYTECODE=1

: "${RELEASE_TAG:?}" "${SOURCE_SHA:?}" "${VALIDATED_CI_RUN:?}" "${CANDIDATE_SHA256:?}"
[[ "$RELEASE_TAG" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]
[[ "$SOURCE_SHA" =~ ^[0-9a-f]{40}$ ]]
[[ "$VALIDATED_CI_RUN" =~ ^[0-9]+$ ]]
[[ "$CANDIDATE_SHA256" =~ ^[0-9a-f]{64}$ ]]
GITHUB_REPOSITORY="${GITHUB_REPOSITORY:-sm-yjr/fan-control}"
[[ "$GITHUB_REPOSITORY" == sm-yjr/fan-control ]]
VERSION="${RELEASE_TAG#v}"
WORK="$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/fan-prebuilt.XXXXXX")"
export WORK VERSION SOURCE_SHA

gh api "repos/$GITHUB_REPOSITORY/actions/runs/$VALIDATED_CI_RUN" > "$WORK/ci.json"
gh api "repos/$GITHUB_REPOSITORY/actions/runs/$VALIDATED_CI_RUN/jobs?per_page=100" > "$WORK/jobs.json"
gh api "repos/$GITHUB_REPOSITORY/actions/workflows/ci.yml" > "$WORK/workflow.json"
gh release view "$RELEASE_TAG" --repo "$GITHUB_REPOSITORY" --json isDraft,targetCommitish > "$WORK/draft.json"
python3 - <<'PY'
import json, os, pathlib, subprocess
p = pathlib.Path(os.environ['WORK'])
run = json.loads((p/'ci.json').read_text())
workflow = json.loads((p/'workflow.json').read_text())
assert run['status'] == 'completed' and run['conclusion'] == 'success'
assert run['workflow_id'] == workflow['id']
required = {'build (macos-14)', 'build (macos-15)', 'Rust 1.85.1 minimum toolchain'}
jobs = json.loads((p/'jobs.json').read_text())['jobs']
assert required <= {j['name'] for j in jobs if j['conclusion'] == 'success'}
subprocess.run(['git', 'fetch', 'origin', run['head_sha'], os.environ['SOURCE_SHA']], check=True)
tree = lambda ref: subprocess.check_output(['git', 'rev-parse', ref+'^{tree}'], text=True).strip()
assert tree(os.environ['SOURCE_SHA']) == tree(run['head_sha']), 'CI source tree differs'
draft = json.loads((p/'draft.json').read_text())
assert draft['isDraft'] and draft['targetCommitish'] == os.environ['SOURCE_SHA']
PY
ARCHIVE="$WORK/FanControl-$VERSION-prebuilt.zip"
export ARCHIVE
gh release download "$RELEASE_TAG" --repo "$GITHUB_REPOSITORY" \
  --pattern "FanControl-$VERSION-prebuilt.zip" --dir "$WORK"
[[ "$(shasum -a 256 "$ARCHIVE" | awk '{print $1}')" == "$CANDIDATE_SHA256" ]]
python3 - <<'PY'
import os, pathlib, posixpath, stat, zipfile
with zipfile.ZipFile(os.environ['ARCHIVE']) as archive:
    for item in archive.infolist():
        path = pathlib.PurePosixPath(item.filename)
        assert not path.is_absolute() and '..' not in path.parts
        assert path.parts[0] in {'FanControl.app', '__MACOSX'}
        if stat.S_ISLNK(item.external_attr >> 16):
            target = archive.read(item).decode()
            resolved = posixpath.normpath(posixpath.join(str(path.parent), target))
            assert not target.startswith('/') and resolved.startswith('FanControl.app/')
PY
ditto -x -k "$ARCHIVE" "$WORK"
APP="$WORK/FanControl.app"
codesign --verify --deep --strict "$APP"
codesign -dvv "$APP" 2>&1 | grep -q 'TeamIdentifier=JFC5CWT3V6'
[[ "$(lipo -archs "$APP/Contents/MacOS/FanControl")" == arm64 ]]
if otool -L "$APP/Contents/Library/LaunchServices/com.local.fan-control.helper" | grep -Eq 'Sparkle\.framework|@rpath/|@executable_path/|@loader_path/'; then
  echo 'The helper has a bundle-only dependency.' >&2
  exit 1
fi
"$APP/Contents/MacOS/FanControl" --package-info > "$WORK/package.json"
git show "$SOURCE_SHA:Cargo.toml" > "$WORK/Cargo.toml"
python3 - <<'PY'
import json, os, pathlib, plistlib, re
p = pathlib.Path(os.environ['WORK'])
package = json.loads((p/'package.json').read_text())
assert package['app']['version'] == os.environ['VERSION']
assert package['helper_protocol'] == 9 and package['architecture'] == 'aarch64'
section = (p/'Cargo.toml').read_text().split('[workspace.package]', 1)[1].split('[', 1)[0]
assert re.search(r'^version\s*=\s*"([^"]+)"', section, re.M).group(1) == os.environ['VERSION']
info = plistlib.loads((p/'FanControl.app/Contents/Info.plist').read_bytes())
assert info['LSMinimumSystemVersion'] == '14.0'
assert info['SUPublicEDKey'] == '/hvZor9jnQzV8BJYBLdNoyEAps0epZ1MvtMu8wq5ikg='
PY
python3 script/verify_widget_bundle.py "$APP"
"$APP/Contents/MacOS/FanControl" --check-updater-runtime
if [[ "${VERIFY_ONLY:-0}" == 1 ]]; then
  echo "Candidate and exact-tree CI verified: $WORK"
  exit 0
fi

for name in MACOS_CERTIFICATE_P12_BASE64 MACOS_CERTIFICATE_PASSWORD APPLE_ID APPLE_TEAM_ID APPLE_APP_SPECIFIC_PASSWORD SPARKLE_PRIVATE_KEY; do
  [[ -n "${!name:-}" ]] || { echo "Missing credential: $name" >&2; exit 1; }
done
KEYCHAIN="$WORK/signing.keychain-db"
KEYCHAIN_PASSWORD="$(uuidgen)"
trap 'security delete-keychain "$KEYCHAIN" >/dev/null 2>&1 || true' EXIT
printf '%s' "$MACOS_CERTIFICATE_P12_BASE64" | base64 --decode > "$WORK/certificate.p12"
security create-keychain -p "$KEYCHAIN_PASSWORD" "$KEYCHAIN"
security set-keychain-settings -lut 21600 "$KEYCHAIN"
security unlock-keychain -p "$KEYCHAIN_PASSWORD" "$KEYCHAIN"
security import "$WORK/certificate.p12" -P "$MACOS_CERTIFICATE_PASSWORD" -A -t cert -f pkcs12 -k "$KEYCHAIN"
rm "$WORK/certificate.p12"
security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$KEYCHAIN_PASSWORD" "$KEYCHAIN" >/dev/null
security list-keychains -d user -s "$KEYCHAIN" login.keychain
IDENTITY="$(security find-identity -v -p codesigning "$KEYCHAIN" | awk -F '"' '/Developer ID Application/ {print $2; exit}')"
[[ -n "$IDENTITY" ]]
xcrun notarytool store-credentials fan-prebuilt --keychain "$KEYCHAIN" \
  --apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" --password "$APPLE_APP_SPECIFIC_PASSWORD"
xcrun notarytool submit "$ARCHIVE" --keychain-profile fan-prebuilt --keychain "$KEYCHAIN" --wait
xcrun stapler staple "$APP"
xcrun stapler validate "$APP"
DMG="$WORK/FanControl-$VERSION.dmg"
./script/package_dmg.sh "$APP" "$DMG" "Fan Control $VERSION"
codesign --force --timestamp --sign "$IDENTITY" "$DMG"
xcrun notarytool submit "$DMG" --keychain-profile fan-prebuilt --keychain "$KEYCHAIN" --wait
xcrun stapler staple "$DMG"
xcrun stapler validate "$DMG"
hdiutil verify "$DMG"
spctl --assess --type open --context context:primary-signature --verbose=4 "$DMG"

curl --fail --location --silent --show-error \
  https://github.com/sparkle-project/Sparkle/releases/download/2.9.2/Sparkle-for-Swift-Package-Manager.zip --output "$WORK/Sparkle.zip"
[[ "$(shasum -a 256 "$WORK/Sparkle.zip" | awk '{print $1}')" == b83e37436774556ed055e0244b297ef2c790e0737393bf65bf495fcbba6eed65 ]]
ditto -x -k "$WORK/Sparkle.zip" "$WORK/sparkle"
mkdir "$WORK/updates"
ditto "$DMG" "$WORK/updates/$(basename "$DMG")"
printf '%s' "$SPARKLE_PRIVATE_KEY" | "$WORK/sparkle/bin/generate_appcast" --ed-key-file - \
  --download-url-prefix "https://github.com/$GITHUB_REPOSITORY/releases/download/$RELEASE_TAG/" \
  --full-release-notes-url "https://github.com/$GITHUB_REPOSITORY/releases/tag/$RELEASE_TAG" \
  --link "https://github.com/$GITHUB_REPOSITORY" "$WORK/updates"
grep -q 'sparkle:edSignature=' "$WORK/updates/appcast.xml"
gh release upload "$RELEASE_TAG" --repo "$GITHUB_REPOSITORY" "$DMG" "$WORK/updates/appcast.xml" --clobber
git show "$SOURCE_SHA:docs/releases/$VERSION.md" > "$WORK/notes.md"
gh release edit "$RELEASE_TAG" --repo "$GITHUB_REPOSITORY" --draft=false --latest \
  --title "Fan Control $VERSION" --notes-file "$WORK/notes.md"
gh release delete-asset "$RELEASE_TAG" --repo "$GITHUB_REPOSITORY" "FanControl-$VERSION-prebuilt.zip" --yes
