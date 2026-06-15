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

PKG_FILE="cefdetector-${RAW_VERSION}-1-x86_64.pkg.tar.zst"
PKG_URL="https://github.com/Tobiichi-Origuchi/CefDetectorLinux/releases/download/v${RAW_VERSION}/${PKG_FILE}"

echo "Calculating sha256sum..."
LOCAL_PKG="../src-tauri/target/release/packager/${PKG_FILE}"
if [ ! -f "$LOCAL_PKG" ]; then
    echo "Error: Local pacman package not found at $LOCAL_PKG"
    exit 1
fi
SHA256=$(sha256sum "$LOCAL_PKG" | awk '{print $1}')

echo "Generating PKGBUILD..."
cat <<EOF > PKGBUILD
pkgname=cefdetector-bin
pkgver=${RAW_VERSION}
pkgrel=1
pkgdesc="Check how many CEFs are on your Linux."
arch=('x86_64')
url="https://github.com/Tobiichi-Origuchi/CefDetectorLinux"
license=('MIT')
provides=('cefdetector')
conflicts=('cefdetector')
source=("\${pkgname}-\${pkgver}.pkg.tar.zst::${PKG_URL}")
sha256sums=('${SHA256}')
noextract=("\${pkgname}-\${pkgver}.pkg.tar.zst")

package() {
    bsdtar -xf "\${srcdir}/\${pkgname}-\${pkgver}.pkg.tar.zst" -C "\$pkgdir/"
    rm -f "\${pkgdir}/.MTREE" "\${pkgdir}/.PKGINFO" "\${pkgdir}/.BUILDINFO"
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
	provides = cefdetector
	conflicts = cefdetector
	source = ${PKG_FILE}::${PKG_URL}
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
