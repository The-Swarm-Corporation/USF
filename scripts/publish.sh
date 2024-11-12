echo "🔑 Logging into crates.io..."
cargo login

echo "🧪 Running publish dry-run to verify package..."
cargo publish --dry-run

echo "📦 Publishing package to crates.io..."
cargo publish

echo "✨ Successfully published! Visit https://crates.io/crates/usf to view the package"