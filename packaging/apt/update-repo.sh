#!/bin/bash
# Add .deb files to the apt repository served from the gh-pages branch, and sign it.
#   update-repo.sh REPO_DIR DEB...      (needs apt-utils and the signing key in gpg)
# Users: see the README ("Install with apt").
set -euo pipefail
REPO=$1; shift
POOL="$REPO/pool/main/t/turbodm"
mkdir -p "$POOL"
cp "$@" "$POOL/"
cd "$REPO"
for arch in amd64 arm64; do
    dir="dists/stable/main/binary-$arch"
    mkdir -p "$dir"
    apt-ftparchive --arch "$arch" packages pool > "$dir/Packages"
    gzip -9kf "$dir/Packages"
done
apt-ftparchive \
    -o APT::FTPArchive::Release::Origin=TurboDM \
    -o APT::FTPArchive::Release::Label=TurboDM \
    -o APT::FTPArchive::Release::Suite=stable \
    -o APT::FTPArchive::Release::Codename=stable \
    -o APT::FTPArchive::Release::Architectures="amd64 arm64" \
    -o APT::FTPArchive::Release::Components=main \
    -o APT::FTPArchive::Release::Description="TurboDM download manager" \
    release dists/stable > dists/stable/Release
gpg --batch --yes --clearsign -o dists/stable/InRelease dists/stable/Release
gpg --batch --yes -abs -o dists/stable/Release.gpg dists/stable/Release
gpg --export > turbodm-archive-keyring.gpg
touch .nojekyll # serve files as they are
