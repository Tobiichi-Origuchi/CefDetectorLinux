#!/bin/bash
set -e

VERSION=$1
if [ -z "$VERSION" ]; then
    echo "Usage: ./publish-aur.sh <version>"
    exit 1
fi

RAW_VERSION=${VERSION#v}

echo "Configuring git for AUR..."
git config --global user.name "github-actions[bot]"
git config --global user.email "github-actions[bot]@users.noreply.github.com"

mkdir -p ~/.ssh
echo "$AUR_SSH_PRIVATE_KEY" > ~/.ssh/aur_key
chmod 600 ~/.ssh/aur_key
ssh-keyscan aur.archlinux.org >> ~/.ssh/known_hosts

cat <<EOF > ~/.ssh/config
Host aur.archlinux.org
  IdentityFile ~/.ssh/aur_key
  User aur
EOF

git clone ssh://aur@aur.archlinux.org/cefdetector-bin.git aur-repo
cd aur-repo

DEB_FILE="CefDetector_${RAW_VERSION}_amd64.deb"
DEB_URL="https://github.com/Tobiichi-Origuchi/CefDetectorLinux/releases/download/v${RAW_VERSION}/${DEB_FILE}"

echo "Calculating sha256sum..."
LOCAL_DEB="../src-tauri/target/release/bundle/deb/CefDetector_${RAW_VERSION}_amd64.deb"
if [ ! -f "$LOCAL_DEB" ]; then
    exit 1
fi
SHA256=$(sha256sum "$LOCAL_DEB" | awk '{print $1}')

echo "Generating PKGBUILD..."
cat <<EOF > PKGBUILD
pkgname=cefdetector-bin
pkgver=${RAW_VERSION}
pkgrel=1
pkgdesc="Check how many CEFs are on your Linux."
arch=('x86_64')
url="https://github.com/Tobiichi-Origuchi/CefDetectorLinux"
license=('MIT')
depends=('webkit2gtk-4.1' 'fd')
provides=('cefdetector')
conflicts=('cefdetector')
source=("${DEB_FILE}::${DEB_URL}")
sha256sums=('${SHA256}')

package() {
    bsdtar -xf data.tar.gz -C "\$pkgdir/"
}
EOF

echo "Generating .SRCINFO..."
cat <<EOF > .SRCINFO
pkgbase = cefdetector-bin
	pkgdesc = Check how many CEFs are on your Linux.
	pkgver = ${RAW_VERSION}
	pkgrel = 1
	url = https://github.com/Tobiichi-Origuchi/CefDetectorLinux
	arch = x86_64
	license = MIT
	depends = webkit2gtk-4.1
	depends = fd
	provides = cefdetector
	conflicts = cefdetector
	source = ${DEB_FILE}::${DEB_URL}
	sha256sums = ${SHA256}

pkgname = cefdetector-bin
EOF

git add PKGBUILD .SRCINFO
if ! git diff-index --quiet HEAD; then
    echo "Pushing new version to AUR..."
    git commit -m "Update to v${RAW_VERSION}"
    git push origin master
else
    echo "No changes to commit for AUR."
fi
