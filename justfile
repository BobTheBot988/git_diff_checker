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

    echo -e "\n[post_hook shared libraries]:"
    readelf -d "{{ root_dir }}/hooks/target/release/post_hook" | grep -E 'NEEDED|Shared library' || echo "  (Statically linked / No dependencies)"

    echo -e "\n[pre_hook shared libraries]:"
    readelf -d "{{ root_dir }}/hooks/target/release/pre_hook" | grep -E 'NEEDED|Shared library' || echo "  (Statically linked / No dependencies)"

# Install the binaries safely
install: build
    @echo -e "\nBEGINNING INSTALL..."
    mkdir -p "{{ bin_dir }}"

    mv "{{ root_dir }}/target/release/git_diff_checker" "{{ bin_dir }}/"
    @echo "=> [1/3] Installed git_diff_checker"

    mv "{{ root_dir }}/hooks/target/release/post_hook" "{{ bin_dir }}/"
    @echo "=> [2/3] Installed post_hook"

    mv "{{ root_dir }}/hooks/target/release/pre_hook" "{{ bin_dir }}/"
    @echo "=> [3/3] Installed pre_hook"

    @echo "INSTALL COMPLETE!"
