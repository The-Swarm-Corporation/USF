echo "🔨 Building release version..."
cargo build --release

echo "🧪 Running tests..."
cargo test

echo "📚 Generating documentation..."
cargo doc

echo "✨ Build, test and documentation complete!"
