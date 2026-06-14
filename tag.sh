#!/bin/bash

set -e

if [ -z "$1" ]; then
  echo "Usage: ./tag.sh vX.Y.Z"
  exit 1
fi

VERSION=$1

if [[ $VERSION != v* ]]; then
  echo "Error: Version must start with 'v' (e.g. v0.1.0)"
  exit 1
fi

RAW_VERSION=${VERSION#v}

echo "Updating versions to $RAW_VERSION..."
sed -i "0,/\"version\": \".*\"/s/\"version\": \".*\"/\"version\": \"$RAW_VERSION\"/" package.json
sed -i "0,/\"version\": \".*\"/s/\"version\": \".*\"/\"version\": \"$RAW_VERSION\"/" src/package.json
sed -i "0,/\"version\": \".*\"/s/\"version\": \".*\"/\"version\": \"$RAW_VERSION\"/" src-tauri/tauri.conf.json
sed -i "0,/^version = \".*\"/s/^version = \".*\"/version = \"$RAW_VERSION\"/" src-tauri/Cargo.toml

echo "Updating package-lock.json..."
npm i

echo "Updating Cargo.lock..."
cd src-tauri
cargo check
cd ..

echo "Committing files..."
git add package.json src/package.json src-tauri/tauri.conf.json src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "$VERSION"

echo "Creating tag $VERSION..."
git tag "$VERSION"

echo "Pushing to origin..."
git push origin "$VERSION"
git push origin master

echo "Done!"
