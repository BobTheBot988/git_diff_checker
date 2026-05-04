#!/bin/bash
# Test script for git_diff_checker
# This script tests various scenarios with the git_diff_checker tool

set -e  # Exit on error

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
TEST_DIR="$SCRIPT_DIR/test1"

echo "=== git_diff_checker Test Suite ==="
echo ""

# Function to restore test file to original state
restore_file() {
    cd "$TEST_DIR" && git checkout hello_world.c > /dev/null 2>&1
}

# Test 1: No modifications
echo "Test 1: No modifications detected"
restore_file
cd "$PROJECT_DIR"
OUTPUT=$(cargo run --release 2>&1)
if echo "$OUTPUT" | grep -q "No modifications detected"; then
    echo "  PASS"
else
    echo "  FAIL"
    echo "  Output: $OUTPUT"
fi

# Test 2: Modified original lines (should be reverted)
echo "Test 2: Modified original lines detected and reverted"
restore_file
cd "$TEST_DIR"
sed -i 's/World/World!/' hello_world.c
cd "$PROJECT_DIR"
OUTPUT=$(cargo run --release 2>&1)
if echo "$OUTPUT" | grep -q "MODIFICATIONS DETECTED" && echo "$OUTPUT" | grep -q "Successfully reverted"; then
    # Verify file was reverted (check for original content)
    if grep -q "Hello, World!" test/test1/hello_world.c && ! grep -q "Hello, World!!" test/test1/hello_world.c; then
        echo "  PASS"
    else
        echo "  FAIL (file not reverted)"
        echo "  File content:"
        cat test/test1/hello_world.c
    fi
else
    echo "  FAIL"
    echo "  Output: $OUTPUT"
fi

# Test 3: Model-added lines only (should NOT be reverted)
echo "Test 3: Model-added lines preserved"
restore_file
cd "$TEST_DIR"
echo "// model added line" >> hello_world.c
cd "$PROJECT_DIR"
OUTPUT=$(cargo run --release 2>&1)
if echo "$OUTPUT" | grep -q "Model-added lines preserved"; then
    echo "  PASS"
else
    echo "  FAIL"
    echo "  Output: $OUTPUT"
fi

# Test 4: Mixed modifications (original modified + model added)
echo "Test 4: Mixed modifications (original modified + model added)"
restore_file
cd "$TEST_DIR"
sed -i 's/World/World!/' hello_world.c
echo "// model added line" >> hello_world.c
cd "$PROJECT_DIR"
OUTPUT=$(cargo run --release 2>&1)
if echo "$OUTPUT" | grep -q "MODIFICATIONS DETECTED" && echo "$OUTPUT" | grep -q "Successfully reverted"; then
    echo "  PASS"
else
    echo "  FAIL"
    echo "  Output: $OUTPUT"
fi

echo ""
echo "=== Test Suite Complete ==="
