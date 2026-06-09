alias b := build
alias i := install

root_dir := justfile_dir()
bin_dir := home_dir() / ".local" / "bin"

# Build all binaries, dynamically choosing between lld and ld
build:
    #!/usr/bin/env bash
    if command -v lld >/dev/null 2>&1; then
        echo "✅ lld found! Compiling with LLVM LLD Linker..."
        export RUSTFLAGS="-C link-arg=-fuse-ld=lld"
    else
        echo "⚠️  lld NOT found! Falling back to default system linker (ld)..."
        export RUSTFLAGS=""
    fi

    echo "Building hooks..."
    cargo build --release --manifest-path "{{ root_dir }}/Cargo.toml"

    echo -e "\n--- VERIFYING DEPENDENCIES VIA READELF ---"

    echo -e "\n[post_hook shared libraries]:"
    readelf -d "{{ root_dir }}/target/release/post_hook" | grep -E 'NEEDED|Shared library' || echo "  (Statically linked / No dependencies)"

    echo -e "\n[post_hook_for_stopping shared libraries]:"
    readelf -d "{{ root_dir }}/target/release/post_hook_for_stopping" | grep -E 'NEEDED|Shared library' || echo "  (Statically linked / No dependencies)"

    echo -e "\n[stop_hook shared libraries]:"
    readelf -d "{{ root_dir }}/target/release/stop_hook" | grep -E 'NEEDED|Shared library' || echo "  (Statically linked / No dependencies)"


    echo -e "\n[pre_hook shared libraries]:"
    readelf -d "{{ root_dir }}/target/release/pre_hook" | grep -E 'NEEDED|Shared library' || echo "  (Statically linked / No dependencies)"

# Install the binaries safely with md5sum checks
install: build
    #!/usr/bin/env bash
    echo -e "\nBEGINNING INSTALL..."
    mkdir -p "{{ bin_dir }}"

    # 1. Handle post_hook
    SRC_POST="{{ root_dir }}/target/release/post_hook"
    DEST_POST="{{ bin_dir }}/post_hook"

    if [ -f "$DEST_POST" ] && [ "$(md5sum < "$SRC_POST")" = "$(md5sum < "$DEST_POST")" ]; then
        echo "[1/4] post_hook is already up-to-date. Skipping."
    else
        cp "$SRC_POST" "$DEST_POST"
        echo "=> [1/4] Installed/Updated post_hook"
    fi
    # 2. Handle post_hook_for_stopping
    SRC_POST="{{ root_dir }}/target/release/post_hook_for_stopping"
    DEST_POST="{{ bin_dir }}/post_hook_for_stopping"

    if [ -f "$DEST_POST" ] && [ "$(md5sum < "$SRC_POST")" = "$(md5sum < "$DEST_POST")" ]; then
        echo "[2/4] post_hook_for_stopping is already up-to-date. Skipping."
    else
        cp "$SRC_POST" "$DEST_POST"
        echo "=> [2/4] Installed/Updated post_hook_for_stopping"
    fi

    # 3. Handle pre_hook
    SRC_PRE="{{ root_dir }}/target/release/pre_hook"
    DEST_PRE="{{ bin_dir }}/pre_hook"

    if [ -f "$DEST_PRE" ] && [ "$(md5sum < "$SRC_PRE")" = "$(md5sum < "$DEST_PRE")" ]; then
        echo "[3/4] pre_hook is already up-to-date. Skipping."
    else
        cp "$SRC_PRE" "$DEST_PRE"
        echo "=> [3/4] Installed/Updated pre_hook"
    fi

    # 4. Handle stop_hook
    SRC_PRE="{{ root_dir }}/target/release/stop_hook"
    DEST_PRE="{{ bin_dir }}/stop_hook"

    if [ -f "$DEST_PRE" ] && [ "$(md5sum < "$SRC_PRE")" = "$(md5sum < "$DEST_PRE")" ]; then
        echo "[4/4] stop_hook is already up-to-date. Skipping."
    else
        cp "$SRC_PRE" "$DEST_PRE"
        echo "=> [4/4] Installed/Updated stop_hook"
    fi

    echo "INSTALL COMPLETE!"
