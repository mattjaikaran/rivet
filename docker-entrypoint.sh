#!/bin/sh
set -e

# If we detect an app.py in the current working directory, run `rivet build`
if [ -f "/app/app.py" ] || [ -f "/app/src/app.py" ]; then
    echo "📦 Detected Rivet application. Running build..."
    rivet build --release
fi

# Execute the generated binary if it exists, otherwise run the CLI
if [ -f "/app/generated/target/release/app" ]; then
    echo "🚀 Starting Rivet application..."
    exec /app/generated/target/release/app "$@"
else
    echo "🛠️  Starting Rivet CLI..."
    exec rivet "$@"
fi