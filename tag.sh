#!/bin/bash

set -e

if [ -z "$1" ]; then
  echo "Usage: ./tag.sh vX.Y.Z"
  exit 1
fi

VERSION=$1

if [[ ! $VERSION =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "Error: Version must be in vX.Y.Z format (e.g. v0.1.0)"
  exit 1
fi

RAW_VERSION=${VERSION#v}

echo "Updating versions to $RAW_VERSION..."
sed -i "0,/^version = \".*\"/s/^version = \".*\"/version = \"$RAW_VERSION\"/" Cargo.toml

echo "Updating Cargo.lock..."
cargo check

echo "Committing files..."
git add Cargo.toml Cargo.lock
git commit -m "$VERSION"

echo "Creating tag $VERSION..."
git tag "$VERSION"

echo "Pushing to origin..."
git push origin "$VERSION"
git push origin master

echo "Done!"
