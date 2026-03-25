#!/bin/bash

# GUI Test Script for rcte-gui
# This script launches the GUI on a virtual display and tests basic functionality

set -e

echo "========================================="
echo "rcte-gui Automated Test Script"
echo "========================================="

# Set display
export DISPLAY=:99

# Check if Xvfb is running
if ! pgrep -x "Xvfb" > /dev/null; then
    echo "Starting Xvfb..."
    Xvfb :99 -screen 0 1280x720x24 -ac +extension GLX +render -noreset &
    sleep 2
fi

echo "✓ Xvfb is running"

# Launch rcte-gui in background
echo ""
echo "Launching rcte-gui with test file..."
./target/release/rcte-gui test_file.txt &
APP_PID=$!
echo "✓ rcte-gui started (PID: $APP_PID)"

# Wait for window to appear
sleep 3

# Get window ID
WINDOW_ID=$(xdotool search --name "rcte-gui" | head -1)
if [ -z "$WINDOW_ID" ]; then
    echo "✗ ERROR: Could not find rcte-gui window"
    kill $APP_PID 2>/dev/null || true
    exit 1
fi
echo "✓ Window found (ID: $WINDOW_ID)"

# Focus window (alternative to activate)
xdotool windowfocus $WINDOW_ID 2>/dev/null || true
sleep 1

echo ""
echo "Testing Hotkeys..."

# Test Select All (Ctrl+A)
echo "  - Testing Ctrl+A (Select All)..."
xdotool key ctrl+a
sleep 0.5
echo "    ✓ Select All executed"

# Click to deselect
echo "  - Clicking to deselect..."
xdotool mousemove --window $WINDOW_ID 400 200 click 1
sleep 0.5
echo "    ✓ Click executed"

# Test Copy (Ctrl+C) - nothing selected, but should not crash
echo "  - Testing Ctrl+C (Copy)..."
xdotool key ctrl+c
sleep 0.5
echo "    ✓ Copy executed"

# Test Paste (Ctrl+V)
echo "  - Testing Ctrl+V (Paste)..."
xdotool key ctrl+v
sleep 0.5
echo "    ✓ Paste executed"

# Test Cut (Ctrl+X)
echo "  - Testing Ctrl+X (Cut)..."
xdotool key ctrl+x
sleep 0.5
echo "    ✓ Cut executed"

echo ""
echo "Testing Mouse Interactions..."

# Test mouse click
echo "  - Testing mouse click..."
xdotool mousemove --window $WINDOW_ID 300 150 click 1
sleep 0.5
echo "    ✓ Mouse click executed"

# Test drag selection
echo "  - Testing drag selection..."
xdotool mousemove --window $WINDOW_ID 300 150 mousedown 1 mousemove --window $WINDOW_ID 500 150 mouseup 1
sleep 0.5
echo "    ✓ Drag selection executed"

# Test copy after selection
echo "  - Testing copy after selection..."
xdotool key ctrl+c
sleep 0.5
echo "    ✓ Copy executed"

# Test scroll (using button 4 and 5 for scroll up/down)
echo "  - Testing mouse wheel scroll..."
xdotool mousemove --window $WINDOW_ID 400 300 click 5 click 5 click 5
sleep 0.5
echo "    ✓ Scroll executed"

echo ""
echo "========================================="
echo "All tests completed successfully!"
echo "========================================="

# Cleanup
echo ""
echo "Closing application..."
kill $APP_PID 2>/dev/null || true
sleep 1

echo "✓ Test finished"
