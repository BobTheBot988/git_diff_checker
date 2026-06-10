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

    echo "Building git_diff_checker..."
    cargo build --release --manifest-path "{{ root_dir }}/Cargo.toml"

    echo "Building hooks..."
    cargo build --release --manifest-path "{{ root_dir }}/hooks/Cargo.toml"

    echo -e "\n--- VERIFYING DEPENDENCIES VIA READELF ---"

    echo "[git_diff_checker shared libraries]:"
    readelf -d "{{ root_dir }}/target/release/git_diff_checker" | grep -E 'NEEDED|Shared library' || echo "  (Statically linked / No dependencies)"

# Install the binaries safely with md5sum checks
install: build
    #!/usr/bin/env bash
    echo -e "\nBEGINNING INSTALL..."
    mkdir -p "{{ bin_dir }}"

    # 1. Handle git_diff_checker
    SRC_GDC="{{ root_dir }}/target/release/git_diff_checker"
    DEST_GDC="{{ bin_dir }}/git_diff_checker"

    if [ -f "$DEST_GDC" ] && [ "$(md5sum < "$SRC_GDC")" = "$(md5sum < "$DEST_GDC")" ]; then
        echo "[1/5] git_diff_checker is already up-to-date. Skipping."
    else
        cp "$SRC_GDC" "$DEST_GDC"
        echo "=> [1/5] Installed/Updated git_diff_checker"
    fi
